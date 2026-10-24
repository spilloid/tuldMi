//! Regression coverage for lifecycle and persistence boundaries not covered by IPC tests.
use super::*;
use crate::capture::fake::{FakeProvider, FakeSource};

#[derive(Default)]
struct Collect(Mutex<Vec<EngineEvent>>);
impl EventSink for Collect {
    fn emit(&self, event: EngineEvent) {
        self.0.lock().push(event);
    }
}
fn engine(dir: &Path, provider: Arc<dyn AudioCaptureProvider>) -> Result<Engine> {
    Engine::start_with_provider(
        EngineConfig {
            data_dir: dir.into(),
            vad_model_path: None,
            suppress_autostart: true,
        },
        Arc::new(Collect::default()),
        provider,
    )
}
#[test]
fn missing_model_preserves_job_attempts() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let engine = engine(dir.path(), Arc::new(FakeProvider::new()))?;
    let session = engine.store().start_session(100)?;
    let event = engine.store().insert_event(&NewEvent {
        session_id: session.id,
        source: Source::Mic,
        started_at: 100,
        ended_at: 200,
        audio: None,
    })?;
    let mut settings = engine.settings();
    settings.model_id = Some("not-installed".into());
    engine.update_settings(settings)?;
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(engine.status().transcriber, TranscriberState::NoModel);
    assert_eq!(
        engine.store().get_event(event.id)?.map(|e| e.status),
        Some(EventStatus::Queued)
    );
    let job = engine
        .store()
        .next_job()?
        .ok_or_else(|| Error::Other("missing job".into()))?;
    assert_eq!(job.attempts, 1);
    engine.shutdown();
    Ok(())
}
#[test]
fn resume_exits_monitoring_and_reopens_devices() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let provider = Arc::new(
        FakeProvider::new().with_source(Source::Mic, FakeSource::mono_16k(vec![0.0; 1600])),
    );
    let engine = engine(dir.path(), provider.clone())?;
    let mut settings = engine.settings();
    settings.system_enabled = false;
    engine.update_settings(settings)?;
    engine.set_monitoring(true)?;
    assert_eq!(provider.open_count(Source::Mic), 1);
    assert!(engine.status().session_id.is_none());
    engine.set_paused(false)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while provider.open_count(Source::Mic) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(provider.open_count(Source::Mic), 2);
    assert!(engine.status().session_id.is_some());
    engine.set_monitoring(false)?;
    assert_eq!(engine.status().state, CaptureState::Recording);
    engine.set_paused(true)?;
    assert!(engine.list_events(None, 10)?.is_empty());
    engine.shutdown();
    Ok(())
}
#[test]
fn filenames_match_event_ids_and_retention_preserves_failed_audio() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let engine = engine(dir.path(), Arc::new(FakeProvider::new()))?;
    let now = chrono::Local::now().timestamp_millis();
    let session = engine.store().start_session(now)?;
    let samples = vec![0.02; 16000];
    let failed = persist_segment(
        &engine.shared,
        session.id,
        Source::Mic,
        now,
        now + 1000,
        &samples,
    )?;
    let done = persist_segment(
        &engine.shared,
        session.id,
        Source::System,
        now + 100,
        now + 1100,
        &samples,
    )?;
    let failed_path = failed
        .audio
        .as_ref()
        .ok_or_else(|| Error::Other("missing audio".into()))?
        .path
        .clone();
    let done_path = done
        .audio
        .as_ref()
        .ok_or_else(|| Error::Other("missing audio".into()))?
        .path
        .clone();
    assert_eq!(
        Path::new(&failed_path).file_stem().and_then(|s| s.to_str()),
        Some(failed.id.to_string().as_str())
    );
    engine.store().next_job()?;
    engine
        .store()
        .fail_job(failed.id, "injected failure", false)?;
    engine
        .store()
        .save_transcript(done.id, &[], Some("en"), "test")?;
    let mut settings = engine.settings();
    settings.retention = Retention::DeleteAfterTranscription;
    engine.update_settings(settings)?;
    assert!(Path::new(&failed_path).is_file());
    assert!(!Path::new(&done_path).exists());
    assert!(
        engine
            .store()
            .get_event(done.id)?
            .is_some_and(|e| e.audio_expired)
    );
    engine.shutdown();
    Ok(())
}
#[test]
fn interrupted_filename_finalization_is_recovered() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let store = Store::open(dir.path())?;
    let session = store.start_session(100)?;
    let old = dir.path().join("audio/old.ogg");
    std::fs::create_dir_all(dir.path().join("audio"))?;
    std::fs::write(&old, b"OggS")?;
    let event = store.insert_event(&NewEvent {
        session_id: session.id,
        source: Source::Mic,
        started_at: 100,
        ended_at: 200,
        audio: Some(NewAudio {
            path: old.clone(),
            sample_rate: 16000,
            duration_ms: 100,
            bytes: 4,
            peaks: vec![],
        }),
    })?;
    let final_path = old.with_file_name(format!("{}.ogg", event.id));
    std::fs::rename(&old, &final_path)?;
    recover_audio_names(&store)?;
    assert!(
        store
            .get_event(event.id)?
            .and_then(|e| e.audio)
            .is_some_and(|a| Path::new(&a.path) == final_path)
    );
    assert!(store.orphan_audio_files()?.is_empty());
    Ok(())
}

#[cfg(feature = "whisper")]
#[test]
#[ignore = "needs TULDMI_TEST_MODEL"]
fn installed_model_drains_queue_and_shutdown_abandons_inflight_job() -> Result<()> {
    let model = std::env::var_os("TULDMI_TEST_MODEL")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Other("set TULDMI_TEST_MODEL".into()))?;
    let dir = tempfile::tempdir()?;
    let engine = engine(dir.path(), Arc::new(FakeProvider::new()))?;
    let session = engine.store().start_session(100)?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/speech.wav");
    let mut reader = hound::WavReader::open(fixture).map_err(|e| Error::Other(e.to_string()))?;
    let samples = reader
        .samples::<i16>()
        .map(|s| s.map(|s| f32::from(s) / 32768.0))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Other(e.to_string()))?;
    let event = persist_segment(
        &engine.shared,
        session.id,
        Source::Mic,
        100,
        100 + samples.len() as i64 / 16,
        &samples,
    )?;
    let installed_path = dir.path().join("models/test.bin");
    std::fs::create_dir_all(dir.path().join("models"))?;
    std::fs::copy(&model, &installed_path)?;
    engine
        .store()
        .record_model(&crate::storage::InstalledModel {
            model_id: "test-model".into(),
            path: installed_path,
            bytes: std::fs::metadata(&model)?.len(),
            sha256: String::new(),
        })?;
    let mut settings = engine.settings();
    settings.model_id = Some("test-model".into());
    settings.language = "en".into();
    engine.update_settings(settings)?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let transcribed = loop {
        if let Some(event) = engine.store().get_event(event.id)? {
            if event.status == EventStatus::Done {
                break event;
            }
            assert_ne!(event.status, EventStatus::Failed);
        }
        assert!(Instant::now() < deadline, "transcription timed out");
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = transcribed
        .segments
        .iter()
        .map(|s| s.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("budget") && text.contains("thursday"));
    let long = samples.repeat(20);
    let pending = persist_segment(
        &engine.shared,
        session.id,
        Source::System,
        200,
        200 + long.len() as i64 / 16,
        &long,
    )?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while engine
        .store()
        .get_event(pending.id)?
        .is_none_or(|e| e.status != EventStatus::Transcribing)
    {
        assert!(Instant::now() < deadline, "job was not claimed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = Instant::now();
    engine.shutdown();
    assert!(started.elapsed() < Duration::from_millis(3100));
    assert_eq!(engine.store().recover_interrupted_jobs()?, 1);
    assert_eq!(
        engine.store().get_event(pending.id)?.map(|e| e.status),
        Some(EventStatus::Queued)
    );
    Ok(())
}

#[test]
fn energy_vad_splits_fixture_at_real_silence() -> Result<()> {
    fn noise(n: usize) -> Vec<f32> {
        let mut x = 12345u32;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.002
            })
            .collect()
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/speech.wav");
    let mut reader = hound::WavReader::open(path).map_err(|e| Error::Other(e.to_string()))?;
    let speech = reader
        .samples::<i16>()
        .map(|s| s.map(|v| f32::from(v) / 32768.0))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::Other(e.to_string()))?;
    let mut clip = noise(32000);
    clip.extend(speech.iter().zip(noise(speech.len())).map(|(s, n)| s + n));
    clip.extend(noise(48000));
    let mut detector = crate::vad::EnergyDetector::new();
    let probabilities = detector.push(&clip);
    let mut segmenter = Segmenter::new(SegmenterConfig::default());
    let mut events = Vec::new();
    for (frame, probability) in clip
        .as_chunks::<FRAME_SAMPLES>()
        .0
        .iter()
        .zip(probabilities)
    {
        events.extend(segmenter.push_frame(frame, probability));
    }
    if let Some(event) = segmenter.flush() {
        events.push(event);
    }
    let durations: Vec<_> = events.iter().map(SpeechSegment::duration_ms).collect();
    // The fixture's ~864 ms pause exceeds the configured 700 ms minimum.
    assert_eq!(events.len(), 2, "speech split into durations {durations:?}");
    Ok(())
}
