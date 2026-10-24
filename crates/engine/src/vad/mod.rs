//! Speech probability per 32 ms frame (`segmenter::FRAME_SAMPLES` at 16 kHz).
//!
//! - `SileroDetector` (feature `whisper`): Silero VAD v6 through whisper.cpp's ggml VAD
//!   (`whisper_rs::WhisperVadContext`). whisper.cpp evaluates a whole buffer per call, so the
//!   detector keeps a short context window and evaluates in small blocks, returning the
//!   probabilities for the newest frames; latency is bounded by the block size (document it).
//! - `EnergyDetector`: dependency-free fallback used when the Silero model is unavailable and in
//!   tests. Adaptive noise floor (slow-rising, fast-falling minimum-tracking of frame energy) and
//!   a smooth logistic mapping of (frame dB - floor dB) to 0..=1. It detects *loud-relative-to-
//!   background*, not speech specifically; the UI labels which detector is active.

#[cfg(feature = "whisper")]
pub mod silero;

pub trait SpeechDetector: Send {
    fn name(&self) -> &'static str;
    /// Push whole frames; returns one probability per frame, in order. Implementations may
    /// buffer internally but must return exactly `frames.len() / FRAME_SAMPLES` values per
    /// call (lookahead is not allowed — the segmenter handles timing).
    fn push(&mut self, frames: &[f32]) -> Vec<f32>;
    fn reset(&mut self);
}

pub struct EnergyDetector {
    // Implementation-defined.
}

impl EnergyDetector {
    pub fn new() -> Self {
        todo!()
    }
}

impl Default for EnergyDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechDetector for EnergyDetector {
    fn name(&self) -> &'static str {
        "energy"
    }
    fn push(&mut self, frames: &[f32]) -> Vec<f32> {
        let _ = frames;
        todo!()
    }
    fn reset(&mut self) {
        todo!()
    }
}

/// Best available detector: Silero if the feature is on and `model` loads, else energy.
pub fn best_detector(model: Option<&std::path::Path>) -> Box<dyn SpeechDetector> {
    let _ = model;
    todo!()
}
