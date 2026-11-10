//! The live pipeline with speaker separation, end to end, on a recording instead of a device:
//! capture (fake system-audio source) -> VAD -> events -> Whisper -> tuldmi-speakers -> store.
//! Uses a throwaway data dir. Meant for public test recordings (e.g. the AMI Meeting Corpus):
//! it prints a few labelled lines of transcript.
//!
//!   cargo run --release -p tuldmi-engine --example speakers_live -- <ggml-base.en.bin> \
//!       <silero.bin> <tuldmi-speakers> <models dir> <recording.wav> [seconds]
//!
//! `models dir` holds pyannote-segmentation-3.0.onnx and nemo-titanet-small.onnx.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tuldmi_engine::capture::fake::{FakeProvider, FakeSource};
use tuldmi_engine::engine::{Engine, EngineConfig, EventSink};
use tuldmi_engine::models::{SPEAKER_CATALOG, entry};
use tuldmi_engine::storage::InstalledModel;
use tuldmi_engine::types::*;

#[derive(Default)]
struct Sink(Mutex<Vec<String>>);
impl EventSink for Sink {
    fn emit(&self, e: EngineEvent) {
        if let EngineEvent::Notice { message, .. } = e {
            self.0.lock().push(message);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (whisper, silero, worker, models, wav) = (&a[0], &a[1], &a[2], PathBuf::from(&a[3]), &a[4]);
    let seconds: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(240);
    let mut reader = hound::WavReader::open(wav)?;
    assert_eq!(reader.spec().sample_rate, 16_000);
    let audio: Vec<f32> = reader.samples::<i16>().take(seconds * 16_000).map(|s| s.unwrap() as f32 / 32768.0).collect();

    let dir = std::env::temp_dir().join(format!("tuldmi-speakers-live-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("models"))?;
    let record = |id: &str, from: PathBuf| -> Result<InstalledModel, Box<dyn std::error::Error>> {
        let e = entry(id).ok_or("catalog")?;
        let path = dir.join("models").join(e.file);
        std::fs::copy(from, &path)?;
        Ok(InstalledModel { model_id: e.id.into(), path, bytes: e.size_bytes, sha256: e.sha256.into() })
    };
    let mut installed = vec![record("whisper-base.en", whisper.into())?];
    for e in SPEAKER_CATALOG {
        installed.push(record(e.id, models.join(e.file))?);
    }

    let provider = FakeProvider::new().with_source(Source::System, FakeSource { silence_when_exhausted: false, ..FakeSource::mono_16k(audio.clone()) });
    let sink = Arc::new(Sink::default());
    let config = EngineConfig { data_dir: dir.clone(), vad_model_path: Some(silero.into()), suppress_autostart: true, speaker_worker: Some(worker.into()) };
    let engine = Engine::start_with_provider(config, sink.clone(), Arc::new(provider))?;
    for m in &installed {
        engine.store().record_model(m)?;
    }
    let mut s = engine.settings();
    s.onboarding_complete = true;
    s.mic_enabled = false;
    s.system_enabled = true;
    s.model_id = Some("whisper-base.en".into());
    s.language = "en".into();
    s.speakers_enabled = true;
    engine.update_settings(s)?;

    let started = Instant::now();
    engine.set_paused(false)?;
    // The fake source runs faster than real time; give it a moment past the end, then flush.
    std::thread::sleep(Duration::from_secs(5).max(Duration::from_millis(audio.len() as u64 / 160)));
    engine.set_paused(true)?;
    loop {
        let st = engine.status();
        if st.backlog == 0 && st.separation.state != SeparationState::Working {
            break;
        }
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("transcription did not finish".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let status = engine.status();
    let events: Vec<SpeechEvent> = engine.list_events(None, 2_000)?.into_iter().rev().filter(|e| e.source == Source::System).collect();
    let segments: Vec<&TranscriptSegment> = events.iter().flat_map(|e| &e.segments).collect();
    let labelled = segments.iter().filter(|s| s.speaker.is_some()).count();
    let mut speakers: Vec<String> = segments.iter().filter_map(|s| s.speaker.as_ref().map(|p| p.label.clone())).collect();
    speakers.sort();
    speakers.dedup();
    println!(
        "{:.0} s of audio -> {} events, {} segments, {} labelled ({:.0}%), speakers {:?}, separation {:?}, took {:.0} s",
        audio.len() as f32 / 16_000.0,
        events.len(),
        segments.len(),
        labelled,
        100.0 * labelled as f32 / segments.len().max(1) as f32,
        speakers,
        status.separation.state,
        started.elapsed().as_secs_f32()
    );
    let mut last = String::new();
    for s in segments.iter().take(40) {
        let who = s.speaker.as_ref().map_or("Remote".to_string(), |p| p.label.clone());
        if who != last {
            println!("{who}:");
            last = who;
        }
        println!("    {}", s.text);
    }
    for notice in sink.0.lock().iter() {
        println!("notice: {notice}");
    }
    engine.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
