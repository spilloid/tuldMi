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
    // Implementation-defined.
}

impl Segmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        let _ = config;
        todo!()
    }

    /// Push exactly one frame (`FRAME_SAMPLES` samples) and its speech probability.
    /// Returns any segments completed by this frame (0, 1, or rarely 2).
    pub fn push_frame(&mut self, frame: &[f32], prob: f32) -> Vec<SpeechSegment> {
        let _ = (frame, prob);
        todo!()
    }

    /// True while inside a speech region (drives `LevelFrame::speaking`).
    pub fn in_speech(&self) -> bool {
        todo!()
    }

    /// Absolute sample index of the next frame to be pushed.
    pub fn position(&self) -> u64 {
        todo!()
    }

    /// Samples currently buffered (history + in-progress region). For memory-bound tests.
    pub fn buffered_samples(&self) -> usize {
        todo!()
    }

    pub fn flush(&mut self) -> Option<SpeechSegment> {
        todo!()
    }

    /// Discard everything and restart positions at `start_sample` (used after a stream restart).
    pub fn reset(&mut self, start_sample: u64) {
        let _ = start_sample;
        todo!()
    }
}
