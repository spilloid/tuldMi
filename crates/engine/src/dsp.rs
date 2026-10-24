//! Format conversion and metering.

/// Converts interleaved device audio (any rate, any channel count) to 16 kHz mono f32.
/// Downmix is the channel mean. Resampling uses `rubato` (sinc, high quality, fixed input
/// chunking internally); `process` accepts any input length and returns whatever output is
/// ready, carrying the remainder, so no samples are dropped or duplicated across calls.
/// Output length over a long run must equal input_frames * 16000 / in_rate within one chunk.
pub struct ToMono16k {
    // Implementation-defined.
}

impl ToMono16k {
    pub fn new(in_rate: u32, channels: u16) -> crate::Result<Self> {
        let _ = (in_rate, channels);
        todo!()
    }
    pub fn in_rate(&self) -> u32 {
        todo!()
    }
    pub fn channels(&self) -> u16 {
        todo!()
    }
    /// `interleaved.len()` must be a multiple of `channels`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        let _ = (interleaved, out);
        todo!()
    }
    /// Emit any buffered tail (zero-padded internally, trimmed to the true length).
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let _ = out;
        todo!()
    }
}

/// Windowed RMS/peak meter over 16 kHz mono. `push` returns a reading every `window` samples.
pub struct LevelMeter {
    window: usize,
    acc_sq: f64,
    peak: f32,
    n: usize,
}

impl LevelMeter {
    /// ~33 ms windows => ~30 readings/second.
    pub fn new() -> Self {
        LevelMeter { window: 533, acc_sq: 0.0, peak: 0.0, n: 0 }
    }

    /// Returns (rms, peak) readings completed by these samples.
    pub fn push(&mut self, samples: &[f32], out: &mut Vec<(f32, f32)>) {
        for &s in samples {
            let a = s.abs();
            self.acc_sq += (s as f64) * (s as f64);
            if a > self.peak {
                self.peak = a;
            }
            self.n += 1;
            if self.n == self.window {
                let rms = (self.acc_sq / self.n as f64).sqrt() as f32;
                out.push((rms.min(1.0), self.peak.min(1.0)));
                self.acc_sq = 0.0;
                self.peak = 0.0;
                self.n = 0;
            }
        }
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}
