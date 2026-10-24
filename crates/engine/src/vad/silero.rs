//! Causal Silero VAD. whisper.cpp resets recurrent state for every detect call.
//! Replay at most 32 preceding frames (1024 ms) plus the current 32 ms frame and
//! take the final probability. No lookahead or frame delay; bounded memory and
//! work, at the cost of replaying context. Reset discards the history.
use super::SpeechDetector;
use crate::{Error, Result, segmenter::FRAME_SAMPLES};
use std::path::Path;
use whisper_rs::{WhisperVadContext, WhisperVadContextParams};
pub struct SileroDetector {
    context: WhisperVadContext,
    history: Vec<f32>,
}
impl SileroDetector {
    pub fn load(model: &Path) -> Result<Self> {
        super::native::quiet_logging();
        let path = model
            .to_str()
            .filter(|s| !s.contains('\0'))
            .ok_or_else(|| Error::Model("invalid VAD model path".into()))?;
        let mut params = WhisperVadContextParams::new();
        params.set_n_threads(1);
        params.set_use_gpu(false);
        let context =
            WhisperVadContext::new(path, params).map_err(|e| Error::Model(e.to_string()))?;
        Ok(Self {
            context,
            history: Vec::with_capacity(33 * FRAME_SAMPLES),
        })
    }
}
impl SpeechDetector for SileroDetector {
    fn name(&self) -> &'static str {
        "silero"
    }
    fn push(&mut self, frames: &[f32]) -> Vec<f32> {
        frames
            .as_chunks::<FRAME_SAMPLES>()
            .0
            .iter()
            .map(|frame| {
                self.history
                    .extend(frame.iter().map(|&v| if v.is_finite() { v } else { 0.0 }));
                let probability = match self.context.detect_speech(&self.history) {
                    Ok(()) => self.context.probabilities().last().copied().unwrap_or(0.0),
                    Err(_) => {
                        tracing::error!("Silero inference failed");
                        0.0
                    }
                };
                if self.history.len() > 32 * FRAME_SAMPLES {
                    self.history.drain(..FRAME_SAMPLES);
                }
                if probability.is_finite() {
                    probability.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect()
    }
    fn reset(&mut self) {
        self.history.clear();
    }
}
