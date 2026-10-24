//! End-to-end engine tests on the fake provider: capture -> VAD -> segment -> Opus + SQLite.
//! Real-model tests are opt-in (they need downloaded weights):
//!   TULDMI_TEST_MODEL=<path to ggml-*.en.bin> cargo test -p tuldmi-engine --test engine -- --ignored

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tuldmi_engine::capture::fake::{FakeProvider, FakeSource};
use tuldmi_engine::engine::*;
use tuldmi_engine::types::*;

#[derive(Default)]
struct Collect(Mutex<Vec<EngineEvent>>);
impl EventSink for Collect {
    fn emit(&self, e: EngineEvent) {
        self.0.lock().push(e);
    }
}
impl Collect {
    fn wait_for(&self, timeout: Duration, mut f: impl FnMut(&EngineEvent) -> bool) -> Option<EngineEvent> {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if let Some(e) = self.0.lock().iter().find(|e| f(e)).cloned() {
                return Some(e);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
    fn count(&self, mut f: impl FnMut(&EngineEvent) -> bool) -> usize {
        self.0.lock().iter().filter(|e| f(e)).count()
    }
}

fn fixture() -> Vec<f32> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/speech.wav");
    let mut r = hound::WavReader::open(p).unwrap();
    assert_eq!(r.spec().sample_rate, 16_000);
    r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

/// Low-level noise floor so the energy VAD has a background to adapt to.
fn noise(n: usize, amp: f32) -> Vec<f32> {
    let mut x: u32 = 12345;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

fn speech_clip() -> Vec<f32> {
    let mut v = noise(32_000, 0.002);
    let mut s = fixture();
    let n = noise(s.len(), 0.002);
    for (a, b) in s.iter_mut().zip(n) {
        *a += b;
    }
    v.extend(s);
    v.extend(noise(48_000, 0.002));
    v
}

fn config(dir: &std::path::Path) -> EngineConfig {
    EngineConfig { data_dir: dir.to_path_buf(), vad_model_path: None, suppress_autostart: true }
}

fn onboarded(engine: &Engine, mic: bool, system: bool) {
    let mut s = engine.settings();
    s.onboarding_complete = true;
    s.mic_enabled = mic;
    s.system_enabled = system;
    engine.update_settings(s).unwrap();
}

#[test]
fn records_speech_from_mic_into_store_and_audio_file() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    let provider = Arc::new(FakeProvider::new().with_source(Source::Mic, FakeSource::mono_16k(speech_clip())));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    onboarded(&engine, true, false);
    assert_eq!(engine.status().state, CaptureState::Stopped);

    let st = engine.set_paused(false).unwrap();
    assert!(matches!(st.state, CaptureState::Starting | CaptureState::Recording));
    assert!(st.session_id.is_some());

    let ev = sink
        .wait_for(Duration::from_secs(20), |e| matches!(e, EngineEvent::EventUpserted(ev) if ev.source == Source::Mic))
        .expect("a speech event should be emitted");
    let EngineEvent::EventUpserted(ev) = ev else { unreachable!() };
    assert_eq!(ev.status, EventStatus::Queued, "no model installed: event waits in the queue");
    let audio = ev.audio.clone().expect("audio retained");
    assert!(PathBuf::from(&audio.path).exists());
    assert!(PathBuf::from(&audio.path).starts_with(dir.path().join("audio")));
    // The fixture is ~6.5 s of speech; allow VAD trimming/merging slop but reject nonsense.
    assert!((3_000..=12_000).contains(&audio.duration_ms), "duration {}", audio.duration_ms);
    assert!(ev.ended_at > ev.started_at);
    assert!(!audio.peaks.is_empty());

    assert!(sink.count(|e| matches!(e, EngineEvent::Levels(l) if l.iter().any(|f| f.source == Source::Mic && f.rms > 0.01))) > 0, "levels must reflect real signal");
    let st = engine.status();
    assert_eq!(st.state, CaptureState::Recording);
    assert_eq!(st.transcriber, TranscriberState::NoModel);
    assert!(st.backlog >= 1);

    let paused = engine.set_paused(true).unwrap();
    assert_eq!(paused.state, CaptureState::Paused);
    assert!(engine.list_events(None, 10).unwrap().iter().any(|e| e.id == ev.id));
    engine.shutdown();

    // Data survives restart; the queued job is still queued.
    let sink2 = Arc::new(Collect::default());
    let engine2 = Engine::start_with_provider(config(dir.path()), sink2, Arc::new(FakeProvider::new())).unwrap();
    let again = engine2.list_events(None, 10).unwrap();
    assert!(again.iter().any(|e| e.id == ev.id && e.status == EventStatus::Queued));
    engine2.shutdown();
}

#[test]
fn monitoring_emits_levels_but_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    let provider = Arc::new(FakeProvider::new().with_source(Source::Mic, FakeSource::mono_16k(speech_clip())));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    engine.set_monitoring(true).unwrap();
    assert!(sink.wait_for(Duration::from_secs(10), |e| matches!(e, EngineEvent::Levels(l) if l.iter().any(|f| f.rms > 0.01))).is_some());
    std::thread::sleep(Duration::from_millis(1500));
    engine.set_monitoring(false).unwrap();
    assert!(engine.list_events(None, 10).unwrap().is_empty(), "monitoring must never persist audio");
    assert_eq!(sink.count(|e| matches!(e, EngineEvent::EventUpserted(_))), 0);
    let audio_dir = dir.path().join("audio");
    let files = walk(&audio_dir);
    assert!(files.is_empty(), "no audio files while monitoring: {files:?}");
    engine.shutdown();
}

#[test]
fn shutdown_flushes_in_progress_speech() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    // Speech with no trailing silence: the segment is still open when we shut down.
    let mut clip = noise(32_000, 0.002);
    clip.extend(fixture());
    let src = FakeSource { realtime: true, ..FakeSource::mono_16k(clip) };
    let provider = Arc::new(FakeProvider::new().with_source(Source::Mic, src));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    onboarded(&engine, true, false);
    engine.set_paused(false).unwrap();
    std::thread::sleep(Duration::from_millis(4_500)); // mid-utterance (2 s noise + 2.5 s speech)
    engine.shutdown();
    let store = tuldmi_engine::storage::Store::open(dir.path()).unwrap();
    let events = store.list_events(None, 10).unwrap();
    assert_eq!(events.len(), 1, "in-progress speech must be persisted on shutdown");
}

#[test]
fn device_loss_is_reported_and_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    let src = FakeSource { fail_after: Some(50), ..FakeSource::mono_16k(noise(16_000 * 20, 0.01)) };
    let provider = Arc::new(FakeProvider::new().with_source(Source::Mic, src));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider.clone()).unwrap();
    onboarded(&engine, true, false);
    engine.set_paused(false).unwrap();
    assert!(
        sink.wait_for(Duration::from_secs(10), |e| matches!(e, EngineEvent::Status(s) if s.sources.iter().any(|x| x.source == Source::Mic && matches!(x.state, SourceState::Unavailable | SourceState::Error)))).is_some(),
        "losing the device must be visible in status"
    );
    assert!(sink.wait_for(Duration::from_secs(5), |e| matches!(e, EngineEvent::Notice { .. })).is_some(), "and surfaced as a notice");
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let st = engine.status();
        if st.sources.iter().any(|x| x.source == Source::Mic && x.state == SourceState::Active) && provider.open_count(Source::Mic) >= 2 {
            break;
        }
        assert!(Instant::now() < end, "engine should reopen the device: {st:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
    engine.shutdown();
}

#[test]
fn delete_and_retention_remove_files_but_keep_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    let provider = Arc::new(FakeProvider::new().with_source(Source::Mic, FakeSource::mono_16k(speech_clip())));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    onboarded(&engine, true, false);
    engine.set_paused(false).unwrap();
    let EngineEvent::EventUpserted(ev) = sink.wait_for(Duration::from_secs(20), |e| matches!(e, EngineEvent::EventUpserted(_))).unwrap() else { unreachable!() };
    engine.set_paused(true).unwrap();
    let path = PathBuf::from(&ev.audio.as_ref().unwrap().path);
    assert!(path.exists());
    engine.delete_event_audio(ev.id).unwrap();
    assert!(!path.exists());
    let e = engine.store().get_event(ev.id).unwrap().unwrap();
    assert!(e.audio.is_none() && e.audio_expired);
    engine.delete_event(ev.id).unwrap();
    assert!(engine.store().get_event(ev.id).unwrap().is_none());
    assert!(sink.wait_for(Duration::from_secs(2), |x| matches!(x, EngineEvent::EventDeleted(id) if *id == ev.id)).is_some());
    // The fixture has an ~860 ms pause between its two sentences, longer than the 700 ms
    // silence threshold, so it may legitimately produce two events. Only the deleted one's
    // file must be gone; the count must match the events that still hold audio.
    let with_audio = engine.list_events(None, 50).unwrap().iter().filter(|e| e.audio.is_some()).count() as u64;
    let stats = engine.storage_stats().unwrap();
    assert_eq!(stats.audio_files, with_audio);
    engine.shutdown();
}

#[test]
fn export_writes_markdown_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::start_with_provider(config(dir.path()), Arc::new(Collect::default()), Arc::new(FakeProvider::new())).unwrap();
    let store = engine.store();
    let sess = store.start_session(1_700_000_000_000).unwrap();
    let ev = store
        .insert_event(&tuldmi_engine::storage::NewEvent { session_id: sess.id, source: Source::System, started_at: 1_700_000_001_000, ended_at: 1_700_000_003_000, audio: None })
        .unwrap();
    store
        .save_transcript(ev.id, &[tuldmi_engine::storage::NewSegment { start_ms: 0, end_ms: 1_000, text: "Quarterly numbers look good.".into(), confidence: None }], Some("en"), "m")
        .unwrap();
    let out = dir.path().join("exports");
    let md = engine.export(ExportFormat::Markdown, None, None, &out).unwrap();
    let md_text = std::fs::read_to_string(&md).unwrap();
    assert!(md_text.contains("Quarterly numbers look good."));
    assert!(md_text.contains("Remote"));
    let js = engine.export(ExportFormat::Json, None, None, &out).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(js).unwrap()).unwrap();
    assert!(v.to_string().contains("Quarterly numbers look good."));
    engine.shutdown();
}

#[test]
fn catalog_is_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::start_with_provider(config(dir.path()), Arc::new(Collect::default()), Arc::new(FakeProvider::new())).unwrap();
    let models = engine.list_models().unwrap();
    assert!(models.len() >= 3);
    assert_eq!(models.iter().filter(|m| m.recommended).count(), 1);
    assert!(models.iter().all(|m| !m.installed));
    assert!(engine.download_model("no-such-model").is_err());
    engine.shutdown();
}

#[cfg(feature = "whisper")]
#[test]
#[ignore = "needs TULDMI_TEST_MODEL"]
fn transcribes_real_speech_with_whisper() {
    use tuldmi_engine::transcribe::TranscriptionProvider;
    let model = std::env::var("TULDMI_TEST_MODEL").expect("set TULDMI_TEST_MODEL");
    let mut w = tuldmi_engine::transcribe::whisper::WhisperProvider::load(std::path::Path::new(&model), "test-model").unwrap();
    let t = w.transcribe(&fixture(), Some("en")).unwrap();
    let text: String = t.segments.iter().map(|s| s.text.to_lowercase()).collect::<Vec<_>>().join(" ");
    assert!(text.contains("budget"), "{text}");
    assert!(text.contains("thursday"), "{text}");
    assert!(t.segments.iter().all(|s| s.end_ms >= s.start_ms));
    // Silence must not hallucinate.
    let silent = w.transcribe(&noise(16_000 * 5, 0.001), Some("en")).unwrap();
    let st: String = silent.segments.iter().map(|s| s.text.trim().to_string()).collect();
    assert!(st.is_empty(), "hallucinated on silence: {st:?}");
}

#[cfg(feature = "whisper")]
#[test]
fn silero_separates_speech_from_silence() {
    use tuldmi_engine::segmenter::FRAME_SAMPLES;
    let model = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/ggml-silero-v6.2.0.bin");
    let mut d = tuldmi_engine::vad::best_detector(Some(&model));
    assert_eq!(d.name(), "silero");
    let clip = speech_clip();
    let usable = clip.len() / FRAME_SAMPLES * FRAME_SAMPLES;
    let probs = d.push(&clip[..usable]);
    assert_eq!(probs.len(), usable / FRAME_SAMPLES);
    let fr = |ms: usize| ms * 16 / FRAME_SAMPLES; // frame index at ms
    let lead: f32 = probs[..fr(1_800)].iter().copied().fold(0.0, f32::max);
    let mid = &probs[fr(3_000)..fr(6_000)];
    let mid_mean = mid.iter().sum::<f32>() / mid.len() as f32;
    assert!(lead < 0.3, "leading noise looked like speech: {lead}");
    assert!(mid_mean > 0.5, "speech not detected: {mid_mean}");
}

fn walk(p: &std::path::Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

// --- regressions from the 2026-10-24 release-gate review (docs/dev-process.md, RG1) ---

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// WASAPI loopback delivers *no buffers* while nothing plays. Speech that ends right before
/// playback stops must still be closed, persisted and correctly timestamped.
#[test]
fn source_going_quiet_still_closes_and_stamps_the_segment() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    let mut clip = noise(32_000, 0.002); // 2 s
    // Speech runs right up to the moment playback stops (trailing quiet trimmed), then the
    // device sends no buffers at all.
    let speech = fixture();
    let last_loud = speech.iter().rposition(|s| s.abs() > 0.02).unwrap_or(speech.len());
    clip.extend_from_slice(&speech[..last_loud + 1]);
    let src = FakeSource { realtime: true, silence_when_exhausted: false, ..FakeSource::mono_16k(clip) };
    let provider = Arc::new(FakeProvider::new().with_source(Source::System, src));
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    onboarded(&engine, false, true);
    let t0 = now_ms();
    engine.set_paused(false).unwrap();
    // The *last* utterance runs up to the moment the device goes quiet. It must be closed
    // within min-silence/post-roll plus slack, not "whenever audio resumes".
    let speech_end_offset = 2_000 + (last_loud as i64) / 16;
    let ev = sink.wait_for(Duration::from_secs(16), |e| {
        matches!(e, EngineEvent::EventUpserted(ev) if ev.source == Source::System && ev.ended_at - t0 >= speech_end_offset - 1_500)
    });
    let Some(EngineEvent::EventUpserted(ev)) = ev else { panic!("the utterance that ran up to the device going quiet was never closed") };
    let end_offset = ev.ended_at - t0;
    assert!(end_offset <= speech_end_offset + 2_500, "event end stamped {end_offset} ms; speech ended at ~{speech_end_offset} ms (timestamps shifted by the gap?)");
    let first = engine.list_events(None, 10).unwrap().into_iter().map(|e| e.started_at - t0).min().unwrap();
    assert!((500..=4_000).contains(&first), "first event stamped {first} ms after start; speech began ~2 s in");
    engine.shutdown();
}

/// A sink that blocks (e.g. the shell's tray during process exit) must never stop the engine
/// from persisting in-progress speech or from shutting down promptly.
#[test]
fn blocking_sink_cannot_lose_speech_or_hang_shutdown() {
    struct Blocking {
        armed: std::sync::atomic::AtomicBool,
        events: Mutex<Vec<EngineEvent>>,
    }
    impl EventSink for Blocking {
        fn emit(&self, e: EngineEvent) {
            if matches!(e, EngineEvent::Status(_)) && self.armed.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_secs(3600));
            }
            self.events.lock().push(e);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Blocking { armed: false.into(), events: Mutex::new(vec![]) });
    let mut clip = noise(32_000, 0.002);
    clip.extend(fixture());
    let provider = Arc::new(
        FakeProvider::new()
            .with_source(Source::Mic, FakeSource { realtime: true, ..FakeSource::mono_16k(clip.clone()) })
            .with_source(Source::System, FakeSource { realtime: true, ..FakeSource::mono_16k(clip) }),
    );
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), provider).unwrap();
    onboarded(&engine, true, true);
    engine.set_paused(false).unwrap();
    std::thread::sleep(Duration::from_millis(4_500)); // both sources mid-utterance
    sink.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let t = Instant::now();
    engine.shutdown();
    assert!(t.elapsed() < Duration::from_millis(2_500), "shutdown took {:?} with a blocked sink", t.elapsed());
    drop(engine);
    let store = tuldmi_engine::storage::Store::open(dir.path()).unwrap();
    let events = store.list_events(None, 10).unwrap();
    assert_eq!(events.len(), 2, "both in-progress utterances must be persisted on shutdown");
}

#[test]
fn stale_partial_downloads_are_swept_at_start() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    let part = dir.path().join("models").join("ggml-tiny.en.bin.part");
    std::fs::write(&part, vec![0u8; 1024]).unwrap();
    let engine = Engine::start_with_provider(config(dir.path()), Arc::new(Collect::default()), Arc::new(FakeProvider::new())).unwrap();
    assert!(!part.exists(), "a .part left by a previous run must be removed at start");
    engine.shutdown();
}

#[test]
fn missing_device_notices_once_not_every_retry() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collect::default());
    // No Mic source configured: every open attempt fails.
    let engine = Engine::start_with_provider(config(dir.path()), sink.clone(), Arc::new(FakeProvider::new())).unwrap();
    onboarded(&engine, true, false);
    engine.set_paused(false).unwrap();
    std::thread::sleep(Duration::from_secs(8)); // >= 2 retry cycles
    let n = sink.count(|e| matches!(e, EngineEvent::Notice { .. }));
    assert!(n <= 1, "{n} notices for one unavailable device; notify on state change only");
    let st = engine.status();
    assert_ne!(st.state, CaptureState::Recording, "nothing is being captured, so the status must not say Recording");
    engine.shutdown();
}
