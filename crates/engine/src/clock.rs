//! Maps per-stream sample positions onto one monotonic clock, then onto wall-clock time.
//!
//! Each capture stream gets a `StreamClock` anchored when its first buffer arrives:
//! (`Instant` of arrival minus the buffer's duration, sample position 0). Sample positions are
//! converted via the nominal 16 kHz rate (post-resample). Wall-clock time is derived from a
//! single `(Instant, SystemTime)` pair taken at engine start, so mic and system timestamps share
//! one timeline even if the wall clock is adjusted while recording. Drift between the device
//! clock and `Instant` is corrected by `observe()`: if the position implied by real elapsed time
//! diverges from the counted samples by more than `MAX_DRIFT_MS`, the anchor is re-based
//! (logged), so long sessions don't accumulate skew.

use std::time::{Instant, SystemTime};

pub const MAX_DRIFT_MS: i64 = 250;

#[derive(Debug, Clone, Copy)]
pub struct WallAnchor {
    pub instant: Instant,
    pub unix_ms: i64,
}

impl WallAnchor {
    pub fn now() -> Self {
        let unix_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        WallAnchor { instant: Instant::now(), unix_ms }
    }

    pub fn to_unix_ms(&self, t: Instant) -> i64 {
        if t >= self.instant {
            self.unix_ms + (t - self.instant).as_millis() as i64
        } else {
            self.unix_ms - (self.instant - t).as_millis() as i64
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamClock {
    wall: WallAnchor,
    sample_rate: u32,
    /// Instant corresponding to `base_sample`.
    base_instant: Instant,
    base_sample: u64,
}

impl StreamClock {
    /// `first_buffer_at` is when the first buffer arrived; `first_buffer_samples` its length
    /// (at `sample_rate`), so sample 0 is placed at the start of that buffer.
    pub fn new(wall: WallAnchor, sample_rate: u32, first_buffer_at: Instant, first_buffer_samples: u64) -> Self {
        let dur = std::time::Duration::from_micros(first_buffer_samples * 1_000_000 / sample_rate as u64);
        let base_instant = first_buffer_at.checked_sub(dur).unwrap_or(first_buffer_at);
        StreamClock { wall, sample_rate, base_instant, base_sample: 0 }
    }

    pub fn instant_of(&self, sample: u64) -> Instant {
        let delta = sample as i128 - self.base_sample as i128;
        let micros = delta * 1_000_000 / self.sample_rate as i128;
        if micros >= 0 {
            self.base_instant + std::time::Duration::from_micros(micros as u64)
        } else {
            self.base_instant - std::time::Duration::from_micros((-micros) as u64)
        }
    }

    pub fn unix_ms_of(&self, sample: u64) -> i64 {
        self.wall.to_unix_ms(self.instant_of(sample))
    }

    /// Explicitly place `sample` at `at` (a stream resuming after the device sent nothing).
    pub fn rebase(&mut self, sample: u64, at: Instant) {
        self.base_instant = at;
        self.base_sample = sample;
    }

    /// Called when a buffer ending at `end_sample` arrives at `now`. Re-bases if drift exceeds
    /// `MAX_DRIFT_MS`. Returns the drift that was observed, in ms (positive = samples late).
    pub fn observe(&mut self, end_sample: u64, now: Instant) -> i64 {
        let expected = self.instant_of(end_sample);
        let drift_ms = if now >= expected {
            (now - expected).as_millis() as i64
        } else {
            -((expected - now).as_millis() as i64)
        };
        if drift_ms.abs() > MAX_DRIFT_MS {
            tracing::debug!(drift_ms, "stream clock re-based");
            self.base_instant = now;
            self.base_sample = end_sample;
        }
        drift_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn maps_samples_to_wall_time() {
        let wall = WallAnchor::now();
        let t0 = wall.instant + Duration::from_millis(1_000);
        let c = StreamClock::new(wall, 16_000, t0, 1_600); // first 100 ms buffer arrives at +1 s
        assert_eq!(c.unix_ms_of(0), wall.unix_ms + 900);
        assert_eq!(c.unix_ms_of(16_000), wall.unix_ms + 1_900);
    }

    #[test]
    fn rebases_on_large_drift_only() {
        let wall = WallAnchor::now();
        let mut c = StreamClock::new(wall, 16_000, wall.instant, 0);
        let small = c.observe(16_000, wall.instant + Duration::from_millis(1_100));
        assert_eq!(small, 100);
        assert_eq!(c.unix_ms_of(16_000), wall.unix_ms + 1_000, "small drift tolerated");
        let big = c.observe(32_000, wall.instant + Duration::from_millis(3_000));
        assert_eq!(big, 1_000);
        assert_eq!(c.unix_ms_of(32_000), wall.unix_ms + 3_000, "large drift re-based");
    }
}
