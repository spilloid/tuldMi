//! Turns a 16 kHz mono sample stream plus per-frame speech probabilities into discrete
//! speech segments, with pre-roll/post-roll and bounded memory.
//!
//! Contract (see `tests/segmenter.rs`):
//! - Input arrives as whole frames of `FRAME_SAMPLES` samples, each with one probability.
//! - Positions are absolute sample indices since the stream started (`u64`), so callers can map
//!   them to time via `clock::StreamClock` without the segmenter knowing about time.
//! - Speech *starts* at the first frame with `prob >= threshold`. Speech *continues* while
//!   `prob >= threshold - HYSTERESIS`. A run of continuing-speech frames whose total speech
//!   duration is below `min_speech_ms` before the region ends is discarded.
//! - A region *ends* when non-speech frames have lasted `max(min_silence_ms, post_roll_ms)`.
//!   The emitted segment spans `[first_speech - pre_roll, last_speech_end + post_roll]`,
//!   clamped to the stream start and to the end of the previously emitted segment (segments
//!   never re-emit audio, except the deliberate split overlap below).
//! - If a region reaches `max_segment_ms`, it is split: the cut point is the lowest-probability
//!   frame within the last `SPLIT_SEARCH_MS` of the segment; the segment ends there (reason
//!   `MaxDuration`) and the next segment begins `SPLIT_OVERLAP_MS` before the cut, so a word
//!   straddling the cut is present in both. Speech continues uninterrupted across the split.
//! - While idle, only `HISTORY_MS` of audio is retained (for pre-roll). Memory is bounded by
//!   `max_segment_ms + HISTORY_MS` at all times.
//! - `flush()` (pause/stop/device loss) emits any in-progress region (reason `Flush`) if it
//!   meets `min_speech_ms`, and resets.

use crate::types::VadSettings;
use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 16_000;
/// 32 ms frames: Silero's native window at 16 kHz.
pub const FRAME_SAMPLES: usize = 512;
pub const HYSTERESIS: f32 = 0.15;
pub const HISTORY_MS: u32 = 2_000;
pub const SPLIT_SEARCH_MS: u32 = 3_000;
pub const SPLIT_OVERLAP_MS: u32 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Silence,
    MaxDuration,
    Flush,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeechSegment {
    /// Absolute sample index of `samples[0]`.
    pub start_sample: u64,
    pub samples: Vec<f32>,
    pub end_reason: EndReason,
    /// Mean speech probability over frames classified as speech.
    pub mean_prob: f32,
}

impl SpeechSegment {
    pub fn end_sample(&self) -> u64 {
        self.start_sample + self.samples.len() as u64
    }
    pub fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1000 / SAMPLE_RATE as u64
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SegmenterConfig {
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub pre_roll_ms: u32,
    pub post_roll_ms: u32,
    pub min_silence_ms: u32,
    pub max_segment_ms: u32,
}

impl From<&VadSettings> for SegmenterConfig {
    fn from(v: &VadSettings) -> Self {
        SegmenterConfig {
            threshold: v.threshold,
            min_speech_ms: v.min_speech_ms,
            pre_roll_ms: v.pre_roll_ms,
            post_roll_ms: v.post_roll_ms,
            min_silence_ms: v.min_silence_ms,
            max_segment_ms: v.max_segment_ms,
        }
    }
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        (&VadSettings::default()).into()
    }
}

pub struct Segmenter {
    config: SegmenterConfig,
    audio: VecDeque<f32>,
    frames: VecDeque<Frame>,
    buffer_start: u64,
    position: u64,
    emitted_end: u64,
    region: Option<Region>,
}

struct Frame {
    start: u64,
    end: u64,
    prob: f32,
    speech: bool,
}

struct Region {
    start: u64,
    last_speech_end: u64,
    // Keep qualification across forced splits, including a short final continuation.
    speech_samples: u64,
}

fn samples(ms: u32) -> u64 {
    u64::from(ms) * u64::from(SAMPLE_RATE) / 1000
}

impl Segmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        let mut config = config;
        config.threshold = if config.threshold.is_finite() {
            config.threshold.clamp(0.0, 1.0)
        } else {
            0.5
        };
        Self {
            config,
            audio: VecDeque::new(),
            frames: VecDeque::new(),
            buffer_start: 0,
            position: 0,
            emitted_end: 0,
            region: None,
        }
    }

    /// Push exactly one frame (`FRAME_SAMPLES` samples) and its speech probability.
    /// Returns any segments completed by this frame (0, 1, or rarely 2).
    pub fn push_frame(&mut self, frame: &[f32], prob: f32) -> Vec<SpeechSegment> {
        let mut out = Vec::new();
        // Invalid frames do not advance the stream. In particular, never index a short frame.
        if frame.len() != FRAME_SAMPLES {
            return out;
        }
        let Some(end) = self.position.checked_add(FRAME_SAMPLES as u64) else {
            return out;
        };
        let prob = if prob.is_nan() {
            0.0
        } else {
            prob.clamp(0.0, 1.0)
        };
        let max_len = samples(self.config.max_segment_ms).max(FRAME_SAMPLES as u64);
        if self.region.is_none() && prob >= self.config.threshold {
            // A pre-roll larger than history (or the cap) cannot be retained safely.
            let pre = samples(self.config.pre_roll_ms)
                .min(samples(HISTORY_MS))
                .min(max_len.saturating_sub(FRAME_SAMPLES as u64));
            self.region = Some(Region {
                start: self
                    .position
                    .saturating_sub(pre)
                    .max(self.buffer_start)
                    .max(self.emitted_end),
                last_speech_end: self.position,
                speech_samples: 0,
            });
        }
        let speech = self.region.is_some() && prob >= self.config.threshold - HYSTERESIS;
        if let Some(region) = &mut self.region
            && speech
        {
            region.last_speech_end = end;
            region.speech_samples = region.speech_samples.saturating_add(FRAME_SAMPLES as u64);
        }
        let keep_from = self.region.as_ref().map_or_else(
            || end.saturating_sub(samples(HISTORY_MS)),
            |region| region.start,
        );
        self.discard_before(keep_from);
        self.audio
            .extend(frame.iter().map(|&s| if s.is_nan() { 0.0 } else { s }));
        self.frames.push_back(Frame {
            start: self.position,
            end,
            prob,
            speech,
        });
        self.position = end;

        while let Some(region) = &self.region {
            if self.position - region.start < max_len {
                break;
            }
            let limit = region.start.saturating_add(max_len);
            // Short caps need proportionately shorter overlap to guarantee forward progress.
            let overlap = samples(SPLIT_OVERLAP_MS).min(max_len / 4);
            let earliest = limit
                .saturating_sub(samples(SPLIT_SEARCH_MS))
                .max(region.start.saturating_add(overlap).saturating_add(1));
            let mut cut = limit;
            let mut lowest = f32::INFINITY;
            for info in &self.frames {
                if info.end >= earliest && info.end <= limit && info.prob <= lowest {
                    // On equal probabilities prefer the latest cut, avoiding needless splits.
                    lowest = info.prob;
                    cut = info.end;
                }
            }
            if let Some(segment) = self.segment(cut, EndReason::MaxDuration) {
                self.emitted_end = cut;
                out.push(segment);
            }
            if let Some(region) = &mut self.region {
                region.start = cut.saturating_sub(overlap).max(region.start + 1);
                let keep_from = region.start;
                self.discard_before(keep_from);
            }
        }

        if self.region.as_ref().is_some_and(|region| {
            self.position - region.last_speech_end
                >= samples(self.config.min_silence_ms.max(self.config.post_roll_ms))
                && !speech
        }) && let Some(segment) = self.finish(EndReason::Silence)
        {
            self.emitted_end = segment.end_sample();
            out.push(segment);
        }
        out
    }

    /// True while inside a speech region (drives `LevelFrame::speaking`).
    pub fn in_speech(&self) -> bool {
        self.region.is_some()
    }

    /// Absolute sample index of the next frame to be pushed.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Samples currently buffered (history + in-progress region). For memory-bound tests.
    pub fn buffered_samples(&self) -> usize {
        self.audio.len()
    }

    pub fn flush(&mut self) -> Option<SpeechSegment> {
        let result = self.finish(EndReason::Flush);
        // Preserve the absolute clock, but prevent pre-roll from crossing a pause/device loss.
        self.reset(self.position);
        result
    }

    /// Discard everything and restart positions at `start_sample` (used after a stream restart).
    pub fn reset(&mut self, start_sample: u64) {
        self.audio.clear();
        self.frames.clear();
        self.buffer_start = start_sample;
        self.position = start_sample;
        self.emitted_end = start_sample;
        self.region = None;
    }

    fn discard_before(&mut self, start: u64) {
        let count = start
            .saturating_sub(self.buffer_start)
            .min(self.audio.len() as u64) as usize;
        self.audio.drain(..count);
        self.buffer_start += count as u64;
        while self
            .frames
            .front()
            .is_some_and(|frame| frame.end <= self.buffer_start)
        {
            self.frames.pop_front();
        }
    }

    fn segment(&self, end: u64, reason: EndReason) -> Option<SpeechSegment> {
        let region = self.region.as_ref()?;
        if region.speech_samples < samples(self.config.min_speech_ms) || end <= region.start {
            return None;
        }
        let start = region.start.max(self.buffer_start);
        let end = end.min(self.position);
        if end <= start {
            return None;
        }
        let mut weight = 0u64;
        let mut probability = 0.0f64;
        for frame in &self.frames {
            if frame.speech {
                let n = frame.end.min(end).saturating_sub(frame.start.max(start));
                weight += n;
                probability += f64::from(frame.prob) * n as f64;
            }
        }
        Some(SpeechSegment {
            start_sample: start,
            samples: self
                .audio
                .iter()
                .skip((start - self.buffer_start) as usize)
                .take((end - start) as usize)
                .copied()
                .collect(),
            end_reason: reason,
            mean_prob: if weight == 0 {
                0.0
            } else {
                (probability / weight as f64) as f32
            },
        })
    }

    fn finish(&mut self, reason: EndReason) -> Option<SpeechSegment> {
        let end = self
            .region
            .as_ref()?
            .last_speech_end
            .saturating_add(samples(self.config.post_roll_ms))
            .min(self.position);
        let segment = self.segment(end, reason);
        self.region = None;
        self.discard_before(self.position.saturating_sub(samples(HISTORY_MS)));
        segment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_hold_on_every_frame_including_small_caps() {
        for cap in [0, 1, 31, 32, 128, 999, 2_000] {
            let mut segmenter = Segmenter::new(SegmenterConfig {
                max_segment_ms: cap,
                pre_roll_ms: u32::MAX,
                post_roll_ms: u32::MAX,
                min_silence_ms: u32::MAX,
                ..SegmenterConfig::default()
            });
            for index in 0..600 {
                let probability = if index < 100 { 0.0 } else { 0.9 };
                segmenter.push_frame(&[0.0; FRAME_SAMPLES], probability);
                assert!(segmenter.buffered_samples() as u64 <= samples(cap) + samples(HISTORY_MS));
            }
        }
    }

    #[test]
    fn qualified_speech_keeps_short_tail_after_split() {
        let mut segmenter = Segmenter::new(SegmenterConfig {
            min_speech_ms: 500,
            pre_roll_ms: 0,
            post_roll_ms: 0,
            min_silence_ms: 32,
            max_segment_ms: 1_000,
            ..SegmenterConfig::default()
        });
        let mut emitted = Vec::new();
        for _ in 0..32 {
            emitted.extend(segmenter.push_frame(&[0.5; FRAME_SAMPLES], 0.9));
        }
        emitted.extend(segmenter.push_frame(&[0.0; FRAME_SAMPLES], 0.0));
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[1].end_reason, EndReason::Silence);
        assert_eq!(emitted[1].end_sample(), 32 * FRAME_SAMPLES as u64);
        assert!(emitted[1].duration_ms() < 500);
    }

    #[test]
    fn invalid_frames_nan_and_position_overflow_are_safe() {
        let mut segmenter = Segmenter::new(SegmenterConfig::default());
        assert!(segmenter.push_frame(&[], 1.0).is_empty());
        assert_eq!(segmenter.position(), 0);
        segmenter.push_frame(&[f32::NAN; FRAME_SAMPLES], f32::NAN);
        assert!(!segmenter.in_speech());
        assert!(segmenter.audio.iter().all(|&sample| sample == 0.0));
        segmenter.reset(u64::MAX - 10);
        assert!(segmenter.push_frame(&[0.0; FRAME_SAMPLES], 1.0).is_empty());
        assert_eq!(segmenter.position(), u64::MAX - 10);
    }
}
