//! Local transcription seam.
//!
//! `whisper::WhisperProvider` (feature `whisper`) runs whisper.cpp through `whisper-rs` on CPU:
//! greedy decoding, `no_context`, timestamps per segment, language from settings ("auto" =
//! detect), threads = available parallelism minus one (min 1, max 8). Segment confidence = mean
//! token probability. Guard against Whisper's well-known silence hallucinations ("Thank you.",
//! "you", subtitle credits): drop segments whose no-speech probability is high or whose text is a
//! known hallucination phrase on low-energy audio. Model load is lazy and happens on the
//! transcription thread, never the capture path.

#[cfg(feature = "whisper")]
pub mod whisper;

use crate::storage::NewSegment;
use crate::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub segments: Vec<NewSegment>,
    /// Detected or configured language (ISO-639-1).
    pub language: Option<String>,
}

/// RMS below which audio is treated as silence and not sent to the model at all.
pub const MIN_TRANSCRIBE_RMS: f64 = 0.002;
/// Shortest clip (100 ms at 16 kHz) worth running inference on.
pub const MIN_TRANSCRIBE_SAMPLES: usize = 1_600;

/// True when `samples_16k` is too short or too quiet to run inference on. Providers return an
/// empty transcript for such audio *without* inference, so callers must not treat that result
/// as "transcribed" (e.g. delete-after-transcription keeps such audio).
pub fn below_energy_gate(samples_16k: &[f32]) -> bool {
    let rms = (samples_16k.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>()
        / samples_16k.len().max(1) as f64)
        .sqrt();
    samples_16k.len() < MIN_TRANSCRIBE_SAMPLES || rms < MIN_TRANSCRIBE_RMS
}

pub trait TranscriptionProvider: Send {
    /// Catalog id of the loaded model, e.g. "whisper-base.en".
    fn model_id(&self) -> &str;
    /// 16 kHz mono f32. `language`: None or "auto" = detect.
    fn transcribe(&mut self, samples_16k: &[f32], language: Option<&str>) -> Result<Transcript>;
}
