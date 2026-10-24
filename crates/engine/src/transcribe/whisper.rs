//! CPU whisper.cpp backend; all printing of recognized content is disabled.
use super::{Transcript, TranscriptionProvider};
use crate::{Error, Result, storage::NewSegment};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
pub struct WhisperProvider {
    context: WhisperContext,
    id: String,
    cancel: Arc<AtomicBool>,
}
impl WhisperProvider {
    pub fn load(model: &Path, model_id: &str) -> Result<Self> {
        crate::vad::native::quiet_logging();
        let path = model
            .to_str()
            .filter(|s| !s.contains('\0'))
            .ok_or_else(|| Error::Model("invalid model path".into()))?;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let context = WhisperContext::new_with_params(path, params)
            .map_err(|e| Error::Model(e.to_string()))?;
        Ok(Self {
            context,
            id: model_id.into(),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
    pub(crate) fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
    }
}
impl TranscriptionProvider for WhisperProvider {
    fn model_id(&self) -> &str {
        &self.id
    }
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<Transcript> {
        let language = language.filter(|s| *s != "auto");
        if language.is_some_and(|s| s.contains('\0') || whisper_rs::get_lang_id(s).is_none()) {
            return Err(Error::Transcription("unsupported language".into()));
        }
        if samples.iter().any(|s| !s.is_finite()) {
            return Err(Error::Transcription("non-finite audio".into()));
        }
        let rms = (samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>()
            / samples.len().max(1) as f64)
            .sqrt();
        if samples.len() < 1600 || rms < 0.002 {
            return Ok(Transcript {
                segments: Vec::new(),
                language: language.map(str::to_owned),
            });
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism()
            // Half the cores (max 6): leaves headroom for the real-time capture threads —
            // saturating every core caused WASAPI xruns in live testing.
            .map_or(1, |n| (n.get() / 2).clamp(1, 6));
        params.set_n_threads(threads as i32);
        params.set_no_context(true);
        params.set_no_timestamps(false);
        params.set_language(language);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_suppress_nst(true);
        let cancel = self.cancel.clone();
        // whisper-rs 0.16's closure helper boxes a trait object but its trampoline
        // casts that allocation to F, which can spuriously abort inference. Use
        // the raw callback with an explicitly owned, stable AtomicBool instead.
        unsafe extern "C" fn abort(data: *mut std::ffi::c_void) -> bool {
            // SAFETY: data points to the AtomicBool in `cancel`, held alive below
            // until synchronous state.full returns. Concurrent loads are atomic.
            unsafe { (&*data.cast::<AtomicBool>()).load(Ordering::Relaxed) }
        }
        // SAFETY: cancel's Arc allocation is stable and lives across state.full;
        // whisper.cpp finishes all callback invocations before full returns.
        unsafe {
            params.set_abort_callback(Some(abort));
            params.set_abort_callback_user_data(Arc::as_ptr(&cancel).cast_mut().cast());
        }
        let mut state = self
            .context
            .create_state()
            .map_err(|e| Error::Transcription(e.to_string()))?;
        state
            .full(params, samples)
            .map_err(|e| Error::Transcription(e.to_string()))?;
        let duration = samples.len() as i64 / 16;
        let mut segments = Vec::new();
        for segment in state.as_iter() {
            let text = segment
                .to_str_lossy()
                .map_err(|e| Error::Transcription(e.to_string()))?
                .trim()
                .to_owned();
            let normalized = text.to_lowercase();
            let phrase = matches!(
                normalized.trim_matches(|c: char| !c.is_alphanumeric()),
                "thank you" | "you" | "thanks for watching"
            ) || normalized.contains("subtitles")
                || normalized.contains("amara.org");
            let start_ms = (segment.start_timestamp() * 10).clamp(0, duration);
            let end_ms = (segment.end_timestamp() * 10).clamp(start_ms, duration);
            let part = &samples[(start_ms as usize * 16).min(samples.len())
                ..(end_ms as usize * 16).min(samples.len())];
            let local_rms = (part.iter().map(|&s| (s as f64).powi(2)).sum::<f64>()
                / part.len().max(1) as f64)
                .sqrt();
            if text.is_empty()
                || segment.no_speech_probability() > 0.6
                || (phrase && local_rms < 0.015)
            {
                continue;
            }
            let probs: Vec<_> = (0..segment.n_tokens())
                .filter_map(|i| segment.get_token(i))
                .map(|t| t.token_probability())
                .filter(|p| p.is_finite())
                .collect();
            let confidence = (!probs.is_empty())
                .then(|| (probs.iter().sum::<f32>() / probs.len() as f32).clamp(0.0, 1.0));
            segments.push(NewSegment {
                start_ms,
                end_ms,
                text,
                confidence,
            });
        }
        Ok(Transcript {
            segments,
            language: whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_owned),
        })
    }
}
