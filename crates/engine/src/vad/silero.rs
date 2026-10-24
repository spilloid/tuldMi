//! Silero VAD v6 via whisper.cpp's ggml VAD context. See `vad` module docs.

use std::path::Path;

use super::SpeechDetector;
use crate::Result;

pub struct SileroDetector {
    // Implementation-defined.
}

impl SileroDetector {
    pub fn load(model: &Path) -> Result<Self> {
        let _ = model;
        todo!()
    }
}

impl SpeechDetector for SileroDetector {
    fn name(&self) -> &'static str {
        "silero"
    }
    fn push(&mut self, frames: &[f32]) -> Vec<f32> {
        let _ = frames;
        todo!()
    }
    fn reset(&mut self) {
        todo!()
    }
}
