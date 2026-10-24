//! Live end-to-end check on real devices: records system audio + mic, waits for speech events to
//! be transcribed, then searches for a word. Uses a throwaway data dir.
//!
//!   cargo run -p tuldmi-engine --example e2e_live -- <ggml model path> <silero path> <seconds> [search word]
//!
//! Prints only metadata and the test phrase's transcript (meant for synthetic test speech).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tuldmi_engine::engine::{Engine, EngineConfig, EventSink};
use tuldmi_engine::storage::InstalledModel;
use tuldmi_engine::types::*;

struct Sink(Mutex<Vec<EngineEvent>>);
impl EventSink for Sink {
    fn emit(&self, e: EngineEvent) {
        if !matches!(e, EngineEvent::Levels(_)) {
            self.0.lock().push(e);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let model = PathBuf::from(args.get(1).ok_or("model path")?);
    let silero = PathBuf::from(args.get(2).ok_or("silero path")?);
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    let word = args.get(4).cloned().unwrap_or_else(|| "budget".into());

    let dir = std::env::temp_dir().join(format!("tuldmi-e2e-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("models"))?;
    let installed = dir.join("models").join(model.file_name().ok_or("model file")?);
    std::fs::copy(&model, &installed)?;

    let sink = Arc::new(Sink(Mutex::new(vec![])));
    let engine = Engine::start(EngineConfig { data_dir: dir.clone(), vad_model_path: Some(silero), suppress_autostart: true }, sink.clone())?;
    let entry = tuldmi_engine::models::entry("whisper-tiny.en").ok_or("catalog")?;
    engine.store().record_model(&InstalledModel { model_id: entry.id.into(), path: installed, bytes: entry.size_bytes, sha256: entry.sha256.into() })?;
    let mut s = engine.settings();
    s.onboarding_complete = true;
    s.model_id = Some(entry.id.into());
    s.language = "en".into();
    engine.update_settings(s)?;
    let st = engine.set_paused(false)?;
    println!("state={:?} sources={:?}", st.state, st.sources.iter().map(|x| (x.source, x.state, x.device_name.clone())).collect::<Vec<_>>());

    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(250));
    }
    engine.set_paused(true)?;
    // Let the transcription queue drain.
    let drain = Instant::now() + Duration::from_secs(60);
    while engine.status().backlog > 0 && Instant::now() < drain {
        std::thread::sleep(Duration::from_millis(250));
    }
    for e in engine.list_events(None, 50)?.iter().rev() {
        let text: String = e.segments.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ");
        println!(
            "{:?} {:>6} ms status={:?} audio={} text={:?}",
            e.source,
            e.ended_at - e.started_at,
            e.status,
            e.audio.as_ref().map(|a| a.bytes).unwrap_or(0),
            text
        );
    }
    let hits = engine.search(&SearchQuery { text: word.clone(), ..Default::default() })?;
    println!("search {word:?}: {} hit(s)", hits.len());
    for e in sink.0.lock().iter() {
        if let EngineEvent::Notice { level, message } = e {
            println!("notice {level:?}: {message}");
        }
    }
    engine.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
