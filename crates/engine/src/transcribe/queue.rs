//! Bounded, monotonic observations of service time and incoming speech on BOTH tracks.
use crate::types::QueueStatus;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(crate) struct QueueMeter {
    started: Instant,
    arrivals: VecDeque<(Instant, i64)>,
    service: VecDeque<(i64, f64)>,
    recording: bool,
}
impl Default for QueueMeter {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            arrivals: VecDeque::new(),
            service: VecDeque::new(),
            recording: false,
        }
    }
}
impl QueueMeter {
    pub fn arrival(&mut self, audio_ms: i64) {
        let now = Instant::now();
        self.arrivals.push_back((now, audio_ms.max(0)));
        // 5 minutes, with a hard cap independent of capture rate.
        while self.arrivals.len() > 4096
            || self
                .arrivals
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(300))
        {
            self.arrivals.pop_front();
        }
    }
    pub fn completed(&mut self, audio_ms: i64, elapsed: Duration) {
        if audio_ms > 0 {
            self.service
                .push_back((audio_ms, elapsed.as_secs_f64() * 1000.0));
            if self.service.len() > 32 {
                self.service.pop_front();
            }
        }
    }
    pub fn reset_service(&mut self) {
        self.service.clear();
    }
    pub fn estimate(&mut self, queue: &mut QueueStatus, recording: bool, available: bool) {
        if recording != self.recording {
            self.recording = recording;
            self.started = Instant::now();
            self.arrivals.clear();
        }
        queue.processing_ms = None;
        queue.catch_up_ms = None;
        queue.falling_behind = false;
        if queue.audio_ms <= 0 || !available || self.service.len() < 3 {
            return;
        }
        let audio: i64 = self.service.iter().map(|s| s.0).sum();
        let elapsed: f64 = self.service.iter().map(|s| s.1).sum();
        if audio <= 0 || elapsed <= 0.0 {
            return;
        }
        let cost = elapsed / audio as f64;
        let work = queue.audio_ms as f64 * cost;
        // Includes the full running clip, deliberately conservative about partial progress.
        queue.processing_ms = Some(work.ceil() as i64);
        if !recording {
            queue.catch_up_ms = queue.processing_ms;
            return;
        }
        let now = Instant::now();
        let window = now.duration_since(self.started).as_secs_f64().min(300.0);
        if window < 30.0 {
            return;
        }
        let incoming: i64 = self
            .arrivals
            .iter()
            .filter(|(at, _)| now.duration_since(*at).as_secs() < 300)
            .map(|s| s.1)
            .sum();
        let utilization = incoming as f64 / (window * 1000.0) * cost;
        queue.falling_behind = utilization >= 1.0;
        if !queue.falling_behind {
            queue.catch_up_ms = Some((work / (1.0 - utilization)).ceil() as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn meter() -> QueueMeter {
        let mut m = QueueMeter {
            started: Instant::now() - Duration::from_secs(60),
            recording: true,
            ..Default::default()
        };
        for _ in 0..3 {
            m.completed(10_000, Duration::from_secs(5));
        }
        m
    }
    #[test]
    fn both_tracks_can_outpace_faster_than_realtime_inference() {
        let mut m = meter();
        m.arrival(130_000);
        let mut q = QueueStatus {
            audio_ms: 60_000,
            ..Default::default()
        };
        m.estimate(&mut q, true, true);
        assert!(q.falling_behind);
        assert_eq!(q.catch_up_ms, None);
        assert_eq!(q.processing_ms, Some(30_000));
        m.estimate(&mut q, false, true);
        assert!(!q.falling_behind);
        assert_eq!(q.catch_up_ms, Some(30_000));
    }
    #[test]
    fn ongoing_arrivals_extend_eta_and_warmup_or_blocked_work_has_none() {
        let mut m = meter();
        m.arrival(60_000);
        let mut q = QueueStatus {
            audio_ms: 60_000,
            ..Default::default()
        };
        m.estimate(&mut q, true, true);
        assert!((59_000..=61_000).contains(&q.catch_up_ms.unwrap()));
        m.estimate(&mut q, true, false);
        assert_eq!(q.processing_ms, None);
        m.reset_service();
        m.estimate(&mut q, false, true);
        assert_eq!(q.catch_up_ms, None);
    }
}
