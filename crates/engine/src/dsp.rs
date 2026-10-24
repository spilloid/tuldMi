//! Format conversion and metering.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters};

/// Converts interleaved device audio (any rate, any channel count) to 16 kHz mono f32.
/// Downmix is the channel mean. Resampling uses `rubato` (sinc, high quality, fixed input
/// chunking internally); `process` accepts any input length and returns whatever output is
/// ready, carrying the remainder, so no samples are dropped or duplicated across calls.
/// Output length over a long run must equal input_frames * 16000 / in_rate within one chunk.
pub struct ToMono16k {
    in_rate: u32,
    channels: u16,
    resampler: Option<Async<f32>>,
    input: Vec<f32>,
    output: Vec<f32>,
    channel_sum: f64,
    channel_count: u16,
    input_frames: u64,
    output_frames: u64,
    delay_left: usize,
}

impl ToMono16k {
    pub fn new(in_rate: u32, channels: u16) -> crate::Result<Self> {
        if in_rate == 0 || channels == 0 {
            return Err(crate::Error::Codec(
                "sample rate and channels must be nonzero".into(),
            ));
        }
        let resampler = if in_rate == 16_000 {
            None
        } else {
            Some(
                Async::<f32>::new_sinc(
                    16_000.0 / f64::from(in_rate),
                    1.0,
                    &SincInterpolationParameters::default(),
                    1024,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|err| crate::Error::Codec(err.to_string()))?,
            )
        };
        let delay_left = resampler.as_ref().map_or(0, Resampler::output_delay);
        let output_len = resampler.as_ref().map_or(0, Resampler::output_frames_max);
        Ok(Self {
            in_rate,
            channels,
            resampler,
            input: Vec::with_capacity(1024),
            output: vec![0.0; output_len],
            channel_sum: 0.0,
            channel_count: 0,
            input_frames: 0,
            output_frames: 0,
            delay_left,
        })
    }
    pub fn in_rate(&self) -> u32 {
        self.in_rate
    }
    pub fn channels(&self) -> u16 {
        self.channels
    }
    /// Partial channel frames are carried into the next call as well as partial resampler chunks.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) -> crate::Result<()> {
        for &sample in interleaved {
            // Accumulate in f64 so even large finite channel values cannot overflow the mean.
            self.channel_sum += if sample.is_finite() {
                f64::from(sample)
            } else {
                0.0
            };
            self.channel_count += 1;
            if self.channel_count != self.channels {
                continue;
            }
            let mono = (self.channel_sum / f64::from(self.channels)) as f32;
            self.channel_sum = 0.0;
            self.channel_count = 0;
            self.input_frames = self
                .input_frames
                .checked_add(1)
                .ok_or_else(|| crate::Error::Codec("input sample count overflow".into()))?;
            if let Some(resampler) = &self.resampler {
                self.input.push(mono);
                if self.input.len() == resampler.input_frames_next() {
                    self.process_chunk(out)?;
                }
            } else {
                out.push(mono);
                self.output_frames = self.input_frames;
            }
        }
        Ok(())
    }
    /// Emit any buffered tail (zero-padded internally, trimmed to the true length).
    pub fn flush(&mut self, out: &mut Vec<f32>) -> crate::Result<()> {
        if self.channel_count != 0 {
            return Err(crate::Error::Codec(
                "incomplete interleaved channel frame".into(),
            ));
        }
        let target = self.target_len();
        if self.resampler.is_some() {
            // One partial chunk may not drain the filter, particularly for very short clips.
            while self.output_frames < target {
                self.process_chunk(out)?;
            }
            if let Some(resampler) = &mut self.resampler {
                resampler.reset();
                self.delay_left = resampler.output_delay();
            }
        }
        self.input.clear();
        self.input_frames = 0;
        self.output_frames = 0;
        Ok(())
    }

    fn target_len(&self) -> u64 {
        // Integer arithmetic avoids cumulative rounding at chunk boundaries.
        (u128::from(self.input_frames) * 16_000)
            .div_ceil(u128::from(self.in_rate))
            .min(u128::from(u64::MAX)) as u64
    }

    fn process_chunk(&mut self, out: &mut Vec<f32>) -> crate::Result<()> {
        let target = self.target_len();
        let Some(resampler) = &mut self.resampler else {
            return Ok(());
        };
        let input = InterleavedSlice::new(&self.input, 1, self.input.len())
            .map_err(|err| crate::Error::Codec(err.to_string()))?;
        let output_len = self.output.len();
        let mut output = InterleavedSlice::new_mut(&mut self.output, 1, output_len)
            .map_err(|err| crate::Error::Codec(err.to_string()))?;
        let indexing = rubato::Indexing::new().partial_len(self.input.len());
        let (_, written) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|err| crate::Error::Codec(err.to_string()))?;
        self.input.clear();
        let skip = self.delay_left.min(written);
        self.delay_left -= skip;
        let count = (written - skip).min(target.saturating_sub(self.output_frames) as usize);
        out.extend(
            self.output[skip..skip + count]
                .iter()
                .map(|&sample| if sample.is_finite() { sample } else { 0.0 }),
        );
        self.output_frames += count as u64;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_channel_boundaries_match_one_call() -> crate::Result<()> {
        for rate in [8_000, 16_000, 22_050, 44_100, 48_000, 96_000] {
            let input: Vec<f32> = (0..6_001)
                .flat_map(|index| {
                    let sample = (index as f32 * 0.17).sin() * 0.4;
                    [sample, sample * 0.5]
                })
                .collect();
            let mut whole = ToMono16k::new(rate, 2)?;
            let mut split = ToMono16k::new(rate, 2)?;
            let (mut a, mut b) = (vec![0.25], vec![0.25]);
            whole.process(&input, &mut a)?;
            for chunk in input.chunks(7) {
                split.process(chunk, &mut b)?;
            }
            whole.flush(&mut a)?;
            split.flush(&mut b)?;
            assert_eq!(a, b, "chunking changed samples at {rate} Hz");
            assert_eq!(
                a.len() - 1,
                (6_001u64 * 16_000).div_ceil(u64::from(rate)) as usize
            );
            split.flush(&mut b)?;
            assert_eq!(a, b, "second flush duplicated the tail");
        }
        Ok(())
    }

    #[test]
    fn short_clips_have_exact_lengths_and_resampler_can_be_reused() -> crate::Result<()> {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let mut resampler = ToMono16k::new(rate, 1)?;
            for len in [0usize, 1, 2, 127, 1_023, 1_024, 1_025, 8_193] {
                let mut out = Vec::new();
                resampler.process(&vec![0.25; len], &mut out)?;
                resampler.flush(&mut out)?;
                assert_eq!(
                    out.len(),
                    (len as u64 * 16_000).div_ceil(u64::from(rate)) as usize
                );
                assert!(out.iter().all(|sample| sample.is_finite()));
            }
        }
        Ok(())
    }

    #[test]
    fn impulse_has_no_startup_delay_and_fast_path_sanitizes_nan() -> crate::Result<()> {
        let mut resampler = ToMono16k::new(48_000, 1)?;
        let mut input = vec![0.0; 4_800];
        input[2_400] = 1.0;
        let mut out = Vec::new();
        resampler.process(&input, &mut out)?;
        resampler.flush(&mut out)?;
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(index, _)| index);
        assert!(peak.is_some_and(|index| index.abs_diff(800) <= 1));

        let mut mono = ToMono16k::new(16_000, 2)?;
        out.clear();
        mono.process(&[f32::NAN], &mut out)?;
        assert!(mono.flush(&mut out).is_err());
        mono.process(&[0.5, -0.5, 0.5], &mut out)?;
        mono.flush(&mut out)?;
        assert_eq!(out, [0.25, 0.0]);
        assert!(ToMono16k::new(0, 1).is_err());
        assert!(ToMono16k::new(16_000, 0).is_err());
        Ok(())
    }
}
