//! Causal Silero VAD. whisper.cpp resets recurrent state for every detect call, so each call
//! replays up to `CONTEXT_FRAMES` (512 ms) of preceding audio before the new frames and keeps
//! only the new frames' probabilities. The engine pushes `runtime::VAD_BLOCK_FRAMES` frames at a
//! time, so inference cost is (context + block) / block, about 3x real frames, instead of
//! replaying the context for every single 32 ms frame (which was ~33x and fell behind real time
//! under transcription load). No lookahead; bounded memory and work. Reset discards history.
use super::SpeechDetector;
use crate::{Error, Result, segmenter::FRAME_SAMPLES};
use std::path::Path;
use whisper_rs::{WhisperVadContext, WhisperVadContextParams};

const CONTEXT_FRAMES: usize = 16;

pub struct SileroDetector {
    context: WhisperVadContext,
    /// Last `CONTEXT_FRAMES` frames of already-scored audio.
    history: Vec<f32>,
    scratch: Vec<f32>,
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
            history: Vec::with_capacity(CONTEXT_FRAMES * FRAME_SAMPLES),
            scratch: Vec::new(),
        })
    }
}
impl SpeechDetector for SileroDetector {
    fn name(&self) -> &'static str {
        "silero"
    }
    fn push(&mut self, frames: &[f32]) -> Vec<f32> {
        let n = frames.len() / FRAME_SAMPLES;
        if n == 0 {
            return Vec::new();
        }
        let new = &frames[..n * FRAME_SAMPLES];
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.history);
        self.scratch
            .extend(new.iter().map(|&v| if v.is_finite() { v } else { 0.0 }));
        let context_frames = self.history.len() / FRAME_SAMPLES;
        let out = match self.context.detect_speech(&self.scratch) {
            Ok(()) => {
                let probs = self.context.probabilities();
                (0..n)
                    .map(|i| probs.get(context_frames + i).copied().unwrap_or(0.0))
                    .map(|p| if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 })
                    .collect()
            }
            Err(_) => {
                tracing::error!("Silero inference failed");
                vec![0.0; n]
            }
        };
        let keep = (CONTEXT_FRAMES * FRAME_SAMPLES).min(self.scratch.len());
        self.history.clear();
        self.history
            .extend_from_slice(&self.scratch[self.scratch.len() - keep..]);
        out
    }
    fn reset(&mut self) {
        self.history.clear();
    }
}
