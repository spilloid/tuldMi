//! whisper.cpp backend. See `transcribe` module docs.

use std::path::Path;

use super::{Transcript, TranscriptionProvider};
use crate::Result;

pub struct WhisperProvider {
    // Implementation-defined.
}

impl WhisperProvider {
    /// Load a ggml model file. Slow (hundreds of ms to seconds); call off the capture path.
    pub fn load(model: &Path, model_id: &str) -> Result<Self> {
        let _ = (model, model_id);
        todo!()
    }
}

impl TranscriptionProvider for WhisperProvider {
    fn model_id(&self) -> &str {
        todo!()
    }
    fn transcribe(&mut self, samples_16k: &[f32], language: Option<&str>) -> Result<Transcript> {
        let _ = (samples_16k, language);
        todo!()
    }
}
