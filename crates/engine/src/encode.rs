//! Ogg Opus encoding of speech segments (16 kHz mono in, voice-tuned), waveform peaks, and
//! decoding back to PCM for transcription.
//!
//! Files are written atomically: encode to `<final>.part` in the same directory, fsync, then
//! rename. A crash never leaves a truncated file at the final path.
//!
//! The stream is RFC 7845 compliant (OpusHead with correct pre-skip, OpusTags, granule
//! positions at 48 kHz) so WebView2/Chromium `<audio>` can play and seek it directly.

use std::path::Path;

use crate::Result;

pub const OPUS_BITRATE: i32 = 24_000;
/// 20 ms Opus frames at 16 kHz.
pub const OPUS_FRAME_SAMPLES: usize = 320;

#[derive(Debug, Clone, PartialEq)]
pub struct EncodedAudio {
    pub bytes: u64,
    pub duration_ms: i64,
    /// `types::PEAKS_PER_SECOND` values per second of audio, each the max |sample| in its
    /// window scaled to 0..=255. Length = ceil(duration_s * PEAKS_PER_SECOND).
    pub peaks: Vec<u8>,
}

/// Encode 16 kHz mono f32 samples (-1..=1; out-of-range values are clamped) to `path`.
pub fn write_ogg_opus(path: &Path, samples_16k: &[f32]) -> Result<EncodedAudio> {
    let _ = (path, samples_16k);
    todo!()
}

/// Decode an Ogg Opus file written by `write_ogg_opus` back to 16 kHz mono f32, with
/// pre-skip removed and the end trimmed to the original length (from the final granule).
pub fn read_ogg_opus_16k(path: &Path) -> Result<Vec<f32>> {
    let _ = path;
    todo!()
}

/// Peak envelope as described on `EncodedAudio::peaks`.
pub fn peaks(samples_16k: &[f32]) -> Vec<u8> {
    let _ = samples_16k;
    todo!()
}
