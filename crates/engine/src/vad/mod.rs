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
pub(crate) mod native;
#[cfg(feature = "whisper")]
pub mod silero;

pub trait SpeechDetector: Send {
    fn name(&self) -> &'static str;
    /// Push whole frames; returns one probability per frame, in order. Implementations may
    /// buffer internally but must return exactly `frames.len() / FRAME_SAMPLES` values per
    /// call (lookahead is not allowed; the segmenter handles timing).
    fn push(&mut self, frames: &[f32]) -> Vec<f32>;
    fn reset(&mut self);
}

pub struct EnergyDetector {
    floor_db: f32,
}
impl EnergyDetector {
    pub fn new() -> Self {
        Self { floor_db: -65.0 }
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
        frames
            .as_chunks::<{ crate::segmenter::FRAME_SAMPLES }>()
            .0
            .iter()
            .map(|frame| {
                let energy = frame
                    .iter()
                    .map(|&s| {
                        if s.is_finite() {
                            (s as f64).powi(2)
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>()
                    / frame.len() as f64;
                let db = (10.0 * energy.max(1e-12).log10()) as f32;
                let alpha = if db < self.floor_db { 0.25 } else { 0.0005 };
                self.floor_db = (self.floor_db + alpha * (db - self.floor_db)).clamp(-100.0, -20.0);
                let relative = 1.0 / (1.0 + (-(db - self.floor_db - 8.0) / 3.0).exp());
                let audible = 1.0 / (1.0 + (-(db + 56.0) / 3.0).exp());
                relative * audible
            })
            .collect()
    }
    fn reset(&mut self) {
        *self = Self::new();
    }
}
pub fn best_detector(model: Option<&std::path::Path>) -> Box<dyn SpeechDetector> {
    #[cfg(feature = "whisper")]
    if let Some(path) = model {
        match silero::SileroDetector::load(path) {
            Ok(detector) => return Box::new(detector),
            Err(_) => tracing::warn!("Silero unavailable; using energy VAD"),
        }
    }
    #[cfg(not(feature = "whisper"))]
    let _ = model;
    Box::new(EnergyDetector::new())
}
