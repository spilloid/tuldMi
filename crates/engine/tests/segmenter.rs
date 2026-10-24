use tuldmi_engine::segmenter::*;

const MS_PER_FRAME: u32 = (FRAME_SAMPLES as u32 * 1000) / SAMPLE_RATE; // 32
const FRAME: u64 = FRAME_SAMPLES as u64;

fn ms_to_samples(ms: u32) -> u64 {
    ms as u64 * SAMPLE_RATE as u64 / 1000
}

/// Drives a segmenter with a probability schedule. Each sample's value encodes its absolute
/// index so we can check exactly which audio a segment contains.
struct Harness {
    seg: Segmenter,
    pos: u64,
    out: Vec<SpeechSegment>,
}

impl Harness {
    fn new(cfg: SegmenterConfig) -> Self {
        Harness { seg: Segmenter::new(cfg), pos: 0, out: vec![] }
    }
    fn run(&mut self, ms: u32, prob: f32) {
        let frames = ms.div_ceil(MS_PER_FRAME);
        for _ in 0..frames {
            let frame: Vec<f32> = (0..FRAME).map(|i| encode(self.pos + i)).collect();
            self.out.extend(self.seg.push_frame(&frame, prob));
            self.pos += FRAME;
        }
    }
    fn flush(&mut self) {
        self.out.extend(self.seg.flush());
    }
}

/// Sample value carrying its index (exact in f32 up to 2^24 = ~17 minutes at 16 kHz).
fn encode(i: u64) -> f32 {
    (i % (1 << 24)) as f32
}

fn assert_contiguous_from_start(s: &SpeechSegment) {
    for (k, v) in s.samples.iter().enumerate() {
        assert_eq!(*v, encode(s.start_sample + k as u64), "sample {k} of segment is not the audio at its claimed position");
    }
}

fn approx(actual: u64, expected: u64, tol_samples: u64) -> bool {
    actual.abs_diff(expected) <= tol_samples
}

#[test]
fn silence_produces_nothing_and_memory_stays_bounded() {
    let mut h = Harness::new(SegmenterConfig::default());
    h.run(60_000, 0.02);
    h.flush();
    assert!(h.out.is_empty());
    assert!(h.seg.buffered_samples() as u64 <= ms_to_samples(HISTORY_MS) + FRAME);
    assert!(!h.seg.in_speech());
    assert_eq!(h.seg.position(), h.pos);
}

#[test]
fn single_utterance_gets_pre_and_post_roll() {
    let cfg = SegmenterConfig::default();
    let mut h = Harness::new(cfg.clone());
    h.run(3_008, 0.05); // 94 frames of silence
    let speech_start = h.pos;
    h.run(1_504, 0.9); // 47 frames of speech
    let speech_end = h.pos;
    h.run(3_008, 0.05);
    assert_eq!(h.out.len(), 1, "expected exactly one segment");
    let s = &h.out[0];
    assert_eq!(s.end_reason, EndReason::Silence);
    let tol = FRAME;
    assert!(approx(s.start_sample, speech_start - ms_to_samples(cfg.pre_roll_ms), tol), "start {} vs {}", s.start_sample, speech_start - ms_to_samples(cfg.pre_roll_ms));
    assert!(approx(s.end_sample(), speech_end + ms_to_samples(cfg.post_roll_ms), tol), "end {} vs {}", s.end_sample(), speech_end + ms_to_samples(cfg.post_roll_ms));
    assert!(s.mean_prob > 0.8);
    assert_contiguous_from_start(s);
}

#[test]
fn speech_shorter_than_min_speech_is_discarded() {
    let mut h = Harness::new(SegmenterConfig::default()); // min_speech 250 ms
    h.run(2_000, 0.0);
    h.run(128, 0.95); // 4 frames
    h.run(3_000, 0.0);
    h.flush();
    assert!(h.out.is_empty(), "a 128 ms blip must not become a segment");
}

#[test]
fn short_pause_does_not_split_but_long_pause_does() {
    let cfg = SegmenterConfig::default(); // min_silence 700, post_roll 700
    let mut h = Harness::new(cfg.clone());
    h.run(1_000, 0.0);
    h.run(800, 0.9);
    h.run(384, 0.1); // below max(min_silence, post_roll)
    h.run(800, 0.9);
    h.run(2_000, 0.0);
    assert_eq!(h.out.len(), 1, "a 384 ms pause stays inside one segment");

    let mut h2 = Harness::new(cfg);
    h2.run(1_000, 0.0);
    h2.run(800, 0.9);
    h2.run(1_500, 0.0);
    h2.run(800, 0.9);
    h2.run(2_000, 0.0);
    assert_eq!(h2.out.len(), 2, "a 1.5 s pause splits");
    assert!(h2.out[1].start_sample >= h2.out[0].end_sample(), "segments must not overlap outside a forced split");
    for s in &h2.out {
        assert_contiguous_from_start(s);
    }
}

#[test]
fn hysteresis_keeps_dipping_speech_together() {
    let cfg = SegmenterConfig::default(); // threshold 0.5 -> continue while >= 0.35
    let mut h = Harness::new(cfg);
    h.run(1_000, 0.0);
    for _ in 0..10 {
        h.run(160, 0.8);
        h.run(160, 0.4); // above threshold - HYSTERESIS
    }
    h.run(2_000, 0.0);
    assert_eq!(h.out.len(), 1);
}

#[test]
fn pre_roll_clamps_at_stream_start() {
    let mut h = Harness::new(SegmenterConfig::default());
    h.run(96, 0.0);
    h.run(1_000, 0.9);
    h.run(2_000, 0.0);
    assert_eq!(h.out.len(), 1);
    assert_eq!(h.out[0].start_sample, 0);
    assert_contiguous_from_start(&h.out[0]);
}

#[test]
fn long_speech_is_split_with_overlap_and_no_gaps() {
    let cfg = SegmenterConfig { max_segment_ms: 10_000, ..SegmenterConfig::default() };
    let mut h = Harness::new(cfg.clone());
    h.run(1_000, 0.0);
    let speech_start = h.pos;
    // 35 s of speech with a soft dip every ~2 s that the splitter should prefer.
    for i in 0..35 {
        h.run(960, 0.9);
        h.run(32, if i % 2 == 0 { 0.4 } else { 0.9 });
    }
    let speech_end = h.pos;
    h.run(2_000, 0.0);

    assert!(h.out.len() >= 4, "35 s at a 10 s max should give at least 4 segments, got {}", h.out.len());
    let max_len = ms_to_samples(cfg.max_segment_ms) + FRAME;
    for s in &h.out {
        assert!(s.samples.len() as u64 <= max_len, "segment of {} samples exceeds max", s.samples.len());
        assert_contiguous_from_start(s);
    }
    for w in h.out.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        assert_eq!(a.end_reason, EndReason::MaxDuration);
        assert!(b.start_sample <= a.end_sample(), "gap between split segments loses audio");
        let overlap = a.end_sample() - b.start_sample;
        assert!(overlap <= ms_to_samples(SPLIT_OVERLAP_MS) + FRAME, "overlap {overlap} too large");
        assert!(overlap > 0, "split segments should overlap deliberately");
    }
    assert!(h.out[0].start_sample <= speech_start);
    assert!(h.out.last().unwrap().end_sample() >= speech_end);
    assert_eq!(h.out.last().unwrap().end_reason, EndReason::Silence);
    assert!(h.seg.buffered_samples() as u64 <= ms_to_samples(cfg.max_segment_ms + HISTORY_MS) + FRAME);
}

#[test]
fn split_prefers_low_probability_frame() {
    let cfg = SegmenterConfig { max_segment_ms: 10_000, ..SegmenterConfig::default() };
    let mut h = Harness::new(cfg);
    h.run(320, 0.0);
    h.run(8_000, 0.95);
    let dip_at = h.pos;
    h.run(32, 0.36); // the only dip, inside the last 3 s before the 10 s cap
    h.run(6_000, 0.95);
    h.run(2_000, 0.0);
    assert!(h.out.len() >= 2);
    let cut = h.out[0].end_sample();
    assert!(approx(cut, dip_at, 2 * FRAME), "cut at {cut}, expected near the dip at {dip_at}");
}

#[test]
fn flush_emits_in_progress_speech_and_resets() {
    let mut h = Harness::new(SegmenterConfig::default());
    h.run(500, 0.0);
    h.run(1_200, 0.9);
    assert!(h.seg.in_speech());
    h.flush();
    assert_eq!(h.out.len(), 1);
    assert_eq!(h.out[0].end_reason, EndReason::Flush);
    assert!(!h.seg.in_speech());
    assert_contiguous_from_start(&h.out[0]);
    // Flushing twice emits nothing new.
    h.flush();
    assert_eq!(h.out.len(), 1);
}

#[test]
fn reset_restarts_positions() {
    let mut h = Harness::new(SegmenterConfig::default());
    h.run(1_000, 0.9);
    h.seg.reset(1_000_000);
    assert_eq!(h.seg.position(), 1_000_000);
    assert!(!h.seg.in_speech());
    assert_eq!(h.seg.buffered_samples(), 0);
}

#[test]
fn post_roll_longer_than_min_silence_is_honored() {
    let cfg = SegmenterConfig { post_roll_ms: 1_000, min_silence_ms: 300, ..SegmenterConfig::default() };
    let mut h = Harness::new(cfg.clone());
    h.run(1_000, 0.0);
    h.run(1_000, 0.9);
    let speech_end = h.pos;
    h.run(3_000, 0.0);
    assert_eq!(h.out.len(), 1);
    assert!(approx(h.out[0].end_sample(), speech_end + ms_to_samples(1_000), FRAME));
}
