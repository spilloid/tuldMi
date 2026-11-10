//! End to end: "Separate past recordings" through the real worker process and models.
//!
//! Needs the two speaker models and a 16 kHz mono recording with several speakers:
//!   TULDMI_SPEAKER_MODELS=<dir with pyannote-segmentation-3.0.onnx and nemo-titanet-small.onnx>
//!   TULDMI_SPEAKER_WAV=<two or more speakers, e.g. sherpa-onnx's 3-two-speakers-en.wav>
//! Without them the test only says it was skipped.

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tuldmi_engine::engine::{Engine, EngineConfig, EventSink};
use tuldmi_engine::models::SPEAKER_CATALOG;
use tuldmi_engine::storage::{InstalledModel, NewAudio, NewEvent, NewSegment, Store};
use tuldmi_engine::types::*;

#[derive(Default)]
struct Collect(Mutex<Vec<EngineEvent>>);
impl EventSink for Collect {
    fn emit(&self, e: EngineEvent) {
        self.0.lock().push(e);
    }
}

#[test]
fn past_conversation_gets_separated_speakers() {
    let (Some(models), Some(wav)) = (std::env::var_os("TULDMI_SPEAKER_MODELS"), std::env::var_os("TULDMI_SPEAKER_WAV")) else {
        eprintln!("skipped: set TULDMI_SPEAKER_MODELS and TULDMI_SPEAKER_WAV to run");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let mut reader = hound::WavReader::open(PathBuf::from(wav)).unwrap();
    assert_eq!(reader.spec().sample_rate, 16_000);
    let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();

    let t0 = 1_800_000_000_000i64;
    let mut events = Vec::new();
    {
        let store = Store::open(&data).unwrap();
        for entry in SPEAKER_CATALOG {
            let path = data.join("models").join(entry.file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::copy(PathBuf::from(&models).join(entry.file), &path).unwrap();
            store
                .record_model(&InstalledModel { model_id: entry.id.into(), path, bytes: entry.size_bytes, sha256: entry.sha256.into() })
                .unwrap();
        }
        let mut settings = store.load_settings().unwrap();
        settings.speakers_enabled = true;
        store.save_settings(&settings).unwrap();
        let session = store.start_session(t0).unwrap();
        // Cut the recording into VAD-like events: pauses of at least 400 ms, at most 20 s.
        let (mut start, mut quiet) = (None, 0usize);
        let mut cuts = Vec::new();
        for (i, block) in audio.chunks(512).enumerate() {
            let rms = (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt();
            let at = i * 512;
            if rms > 0.01 { quiet = 0; start.get_or_insert(at); } else { quiet += 1; }
            if let Some(s) = start && (quiet * 512 >= 6_400 || at - s >= 20 * 16_000) {
                cuts.push((s, at + 512));
                start = None;
                quiet = 0;
            }
        }
        if let Some(s) = start { cuts.push((s, audio.len())); }
        std::fs::create_dir_all(data.join("audio")).unwrap();
        for (n, &(s, e)) in cuts.iter().enumerate() {
            let samples = &audio[s..e.min(audio.len())];
            let path = data.join("audio").join(format!("event-{n}.ogg"));
            let encoded = tuldmi_engine::encode::write_ogg_opus(&path, samples).unwrap();
            let event = store
                .insert_event(&NewEvent {
                    session_id: session.id,
                    source: Source::System,
                    started_at: t0 + (s / 16) as i64,
                    ended_at: t0 + (e / 16) as i64,
                    audio: Some(NewAudio { path, sample_rate: 16_000, duration_ms: encoded.duration_ms, bytes: encoded.bytes as i64, peaks: encoded.peaks }),
                })
                .unwrap();
            // Two segments per event, so a voice change inside an event can show.
            let mid = encoded.duration_ms / 2;
            let segments = [
                NewSegment { start_ms: 0, end_ms: mid, text: format!("first half {n}"), confidence: None, speaker_id: None },
                NewSegment { start_ms: mid, end_ms: encoded.duration_ms, text: format!("second half {n}"), confidence: None, speaker_id: None },
            ];
            store.save_transcript(event.id, &segments, Some("en"), "test").unwrap();
            events.push(event.id);
        }
    }
    assert!(events.len() >= 3, "the recording should make several events");

    let sink = Arc::new(Collect::default());
    let engine = Engine::start(
        EngineConfig {
            data_dir: data.clone(),
            vad_model_path: None,
            suppress_autostart: true,
            speaker_worker: Some(env!("CARGO_BIN_EXE_tuldmi-speakers").into()),
        },
        sink.clone(),
    )
    .unwrap();
    assert_eq!(engine.speaker_models().unwrap().iter().filter(|m| m.installed).count(), 2);
    assert_eq!(engine.separate_past_recordings().unwrap() as usize, events.len());

    let deadline = Instant::now() + Duration::from_secs(600);
    let done = loop {
        let notice = sink.0.lock().iter().find_map(|e| match e {
            EngineEvent::Notice { message, .. } if message.contains("past conversation") => Some(message.clone()),
            _ => None,
        });
        if let Some(message) = notice {
            break message;
        }
        assert!(Instant::now() < deadline, "backfill did not finish");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(done, "Separated the speakers in 1 past conversation");
    assert_eq!(engine.status().separation.backfill_remaining, None);

    let store = engine.store();
    let mut speakers = std::collections::BTreeMap::new();
    for id in &events {
        let event = store.get_event(*id).unwrap().unwrap();
        for segment in &event.segments {
            if let Some(speaker) = &segment.speaker {
                assert_eq!(speaker.kind, "person");
                *speakers.entry(speaker.label.clone()).or_insert(0) += 1;
            }
        }
    }
    eprintln!("speakers shown: {speakers:?}");
    assert!(speakers.len() >= 2, "a multi-speaker recording should show at least two speakers: {speakers:?}");
    assert!(speakers.keys().all(|l| l.starts_with("Speaker ")));
    assert_eq!(engine.separate_past_recordings().unwrap(), 0, "nothing left to separate");

    // Renaming applies to every segment of that speaker.
    let speaker = events
        .iter()
        .find_map(|id| store.get_event(*id).unwrap().unwrap().segments.iter().find_map(|s| s.speaker.clone()))
        .unwrap();
    engine.rename_speaker(speaker.id, "Dana").unwrap();
    let renamed: Vec<_> = events
        .iter()
        .flat_map(|id| store.get_event(*id).unwrap().unwrap().segments)
        .filter(|s| s.speaker.as_ref().is_some_and(|p| p.id == speaker.id))
        .collect();
    assert!(!renamed.is_empty() && renamed.iter().all(|s| s.speaker.as_ref().unwrap().label == "Dana"));
    assert!(engine.rename_speaker(speaker.id, "  ").is_err());
    engine.shutdown();
    drop(engine);

    // No voice data was written anywhere in the database.
    let db = rusqlite::Connection::open(data.join("tuldmi.db")).unwrap();
    let stored: i64 = db.query_row("SELECT COUNT(*) FROM speakers WHERE embedding IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(stored, 0, "speakers.embedding must stay NULL");
    let unseparated: i64 = db
        .query_row("SELECT COUNT(*) FROM audio_events WHERE speakers_state IS NOT 'done'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(unseparated, 0);
}
