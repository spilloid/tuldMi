//! The running engine: owns capture streams, per-source processing threads, the transcription
//! worker, housekeeping, and the store. All public methods are cheap and non-blocking from the
//! caller's point of view (they may take a short lock); nothing here runs on the UI thread's
//! behalf for longer than a database call.
//!
//! Threads:
//! - one capture stream per enabled source (provider-owned; callback -> bounded channel)
//! - `tuldmi-proc`: drains the capture channel; per source: `ToMono16k` -> `LevelMeter` ->
//!   `SpeechDetector` -> `Segmenter`; finished segments go to a bounded writer queue.
//!   Levels are batched to ~30 Hz. Writer backpressure stops draining capture audio;
//!   capture callbacks then drop and count buffers while controls remain responsive.
//! - `tuldmi-write`: encodes (`encode::write_ogg_opus` to
//!   `audio/YYYY/MM/DD/<event-uuid>.ogg`), inserts (`Store::insert_event`), announces
//!   (`EventUpserted`), and wakes the transcription worker.
//! - `tuldmi-asr`: loops `Store::next_job`; lazily loads the configured model; decodes the
//!   event's audio; transcribes; `save_transcript`; applies `DeleteAfterTranscription`; emits
//!   upserts and status (backlog/transcriber state). Parks on a condvar/channel when idle or
//!   when no model is installed; failures go through `fail_job` (model-missing = not retryable
//!   until the user installs a model, then queued jobs proceed).
//! - `tuldmi-house`: at start, every 10 minutes, and on request after a retention settings
//!   change: retention (`expire_audio_events_before`, one bounded SQL update; upserts only for
//!   events whose audio expired), unlink returned files, orphan sweep at start; every 3 s:
//!   reopen `Unavailable` sources and follow default-device changes for unpinned sources.
//! - `tuldmi-events` (`dispatch`): the only thread that calls the `EventSink`. Every other
//!   thread (and model downloads) enqueue into a bounded queue and never block on the sink.
//!
//! Partial model downloads (`models/*.part`) are swept at start, when no download can be active.
//!
//! State machine: Stopped -> (start/resume) Starting -> Recording; Recording -> (pause) Paused.
//! While a session runs, the state is Recording only if some enabled source has a live stream;
//! otherwise it is Error: every enabled source failed (each shows `Unavailable` with its error;
//! housekeeping keeps retrying, and a device notice is emitted only when a source's state or
//! error changes), or no source is enabled at all (each shows `enabled: false`). A session row
//! spans one run (resume starts a new session). Pause and shutdown flush segmenters first, so
//! in-progress speech is kept. Monitoring opens devices and emits levels with *no* segmenting or
//! storage, and is only allowed while not recording.
//!
//! Shutdown: cancel downloads, stop streams, flush and persist in-progress segments, end
//! session, signal threads, join with a timeout (an in-flight transcription is abandoned and
//! recovered as `queued` on next start), join cancelled downloads briefly, then give the event
//! dispatcher a short grace and abandon it if the sink is stuck. Persisting never depends on
//! the sink.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::Result;
use crate::capture::AudioCaptureProvider;
use crate::storage::Store;
use crate::types::*;

pub trait EventSink: Send + Sync + 'static {
    /// Return promptly; queue UI work rather than calling engine commands synchronously.
    fn emit(&self, event: EngineEvent);
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Root for tuldmi.db, audio/, models/.
    pub data_dir: PathBuf,
    /// Bundled Silero ggml model. None or missing file = energy VAD fallback (with a notice).
    pub vad_model_path: Option<PathBuf>,
    /// Override for tests: never start capture automatically even if settings say so.
    pub suppress_autostart: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Markdown,
    Json,
}

#[path = "runtime.rs"]
mod runtime;
use crate::dispatch::Dispatcher;
use crate::{Error, models::ModelManager};
use crossbeam_channel::{Sender, bounded};
use parking_lot::Mutex;
use runtime::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct Engine {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    /// Requests for an immediate retention sweep on `tuldmi-house` (reply when done).
    retention: Sender<Sender<()>>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    events: Dispatcher,
    events_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    models: ModelManager,
    transitions: Mutex<()>,
    stopped: AtomicBool,
}
/// How long `shutdown` waits for a stuck event sink before abandoning undelivered events.
const DISPATCH_GRACE: Duration = Duration::from_millis(250);

/// No download can be active before the engine starts: any `.part` is a stranded partial.
fn sweep_partial_downloads(models_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(models_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "part")
            && path.is_file()
            && let Err(error) = unlink(&path)
        {
            tracing::warn!(%error, "could not remove a stale partial model download");
        }
    }
}
impl Engine {
    pub fn start(config: EngineConfig, sink: Arc<dyn EventSink>) -> Result<Engine> {
        Self::start_with_provider(
            config,
            sink,
            Arc::new(crate::capture::cpal_provider::CpalProvider::new()),
        )
    }
    pub fn start_with_provider(
        config: EngineConfig,
        sink: Arc<dyn EventSink>,
        provider: Arc<dyn AudioCaptureProvider>,
    ) -> Result<Engine> {
        let store = Arc::new(Store::open(&config.data_dir)?);
        sweep_partial_downloads(&store.data_dir().join("models"));
        store.recover_interrupted_jobs()?;
        store.close_dangling_sessions()?;
        recover_audio_names(&store)?;
        for path in store.orphan_audio_files()? {
            unlink(&path)?;
        }
        let settings = store.load_settings()?;
        let autostart = settings.onboarding_complete
            && settings.capture_on_launch
            && !config.suppress_autostart;
        let (commands, receive) = bounded(32);
        let (wake, waking) = bounded(1);
        let (retention, retaining) = bounded(4);
        // Engine threads emit only into the dispatcher; it alone calls the shell's sink.
        let (events, events_thread) = Dispatcher::start(sink)?;
        let sink: Arc<dyn EventSink> = Arc::new(events.clone());
        let shared = match Shared::new(config, store.clone(), sink.clone(), provider, settings, wake)
        {
            Ok(shared) => Arc::new(shared),
            Err(error) => {
                events.finish(events_thread, DISPATCH_GRACE);
                return Err(error);
            }
        };
        let models = ModelManager::new(store, sink);
        let mut engine = Self {
            shared: shared.clone(),
            commands: commands.clone(),
            retention,
            threads: Mutex::new(Vec::new()),
            events,
            events_thread: Mutex::new(Some(events_thread)),
            models,
            transitions: Mutex::new(()),
            stopped: AtomicBool::new(false),
        };
        let (persist, persisting) = bounded(16);
        let starts = [
            spawn("tuldmi-write", shared.clone(), move |s| {
                writer(s, persisting)
            }),
            spawn("tuldmi-proc", shared.clone(), move |s| {
                processing(s, receive, persist)
            }),
            spawn("tuldmi-asr", shared.clone(), move |s| {
                transcription(s, waking)
            }),
            spawn("tuldmi-house", shared.clone(), move |s| {
                housekeeping(s, commands, retaining)
            }),
        ];
        for result in starts {
            match result {
                Ok(thread) => engine.threads.get_mut().push(thread),
                Err(error) => {
                    engine.shutdown();
                    return Err(error);
                }
            }
        }
        shared.publish();
        if autostart {
            engine.set_paused(false)?;
        }
        Ok(engine)
    }
    fn request(&self, make: impl FnOnce(Sender<Result<()>>) -> Command) -> Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(Error::Other("engine is shut down".into()));
        }
        let (reply, result) = bounded(1);
        self.commands
            .send_timeout(make(reply), Duration::from_millis(100))
            .map_err(|_| Error::Other("engine is busy or unavailable".into()))?;
        result
            .recv_timeout(Duration::from_secs(8))
            .map_err(|_| Error::Other("engine operation timed out".into()))?
    }
    pub fn status(&self) -> CaptureStatus {
        self.shared.status.lock().clone()
    }
    pub fn set_paused(&self, paused: bool) -> Result<CaptureStatus> {
        let _guard = self.transitions.lock();
        self.request(|reply| Command::Pause(paused, reply))?;
        Ok(self.status())
    }
    pub fn set_monitoring(&self, enabled: bool) -> Result<CaptureStatus> {
        let _guard = self.transitions.lock();
        self.request(|reply| Command::Monitor(enabled, reply))?;
        Ok(self.status())
    }
    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        self.shared.provider.list_devices()
    }
    pub fn settings(&self) -> Settings {
        self.shared.settings.lock().clone()
    }
    pub fn update_settings(&self, settings: Settings) -> Result<Settings> {
        let _guard = self.transitions.lock();
        if !settings.vad.threshold.is_finite()
            || !(0.0..=1.0).contains(&settings.vad.threshold)
            || settings.vad.max_segment_ms == 0
            || settings.vad.max_segment_ms > 300_000
            || settings.vad.pre_roll_ms > 2_000
            || settings.vad.post_roll_ms > 300_000
            || settings.vad.min_silence_ms > 300_000
            || settings.vad.min_speech_ms > 300_000
        {
            return Err(Error::Other("invalid VAD settings".into()));
        }
        let previous_retention = self.shared.settings.lock().retention;
        self.shared.store.save_settings(&settings)?;
        self.request(|reply| Command::Settings(settings.clone(), reply))?;
        let _ = self.shared.wake.try_send(());
        if previous_retention != settings.retention {
            // The sweep runs on the housekeeping thread (bounded SQL, never the capture path).
            // Wait briefly so the caller sees the new policy applied; on timeout it finishes
            // in the background.
            let (reply, done) = bounded(1);
            if self
                .retention
                .send_timeout(reply, Duration::from_millis(100))
                .is_ok()
            {
                let _ = done.recv_timeout(Duration::from_secs(5));
            }
        }
        Ok(settings)
    }
    pub fn store(&self) -> Arc<Store> {
        self.shared.store.clone()
    }
    pub fn list_events(&self, before: Option<i64>, limit: u32) -> Result<Vec<SpeechEvent>> {
        self.shared.store.list_events(before, limit)
    }
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        self.shared.store.search(query)
    }
    pub fn retry_event(&self, id: Uuid) -> Result<bool> {
        let _gate = self.shared.persist_gate.lock();
        let changed = self.shared.store.retry_event(id)?;
        if changed {
            self.shared.upsert(id)?;
            let _ = self.shared.wake.try_send(());
            self.shared.refresh_backlog();
        }
        Ok(changed)
    }
    pub fn delete_event(&self, id: Uuid) -> Result<()> {
        let _gate = self.shared.persist_gate.lock();
        let removed = self
            .shared
            .store
            .delete_event(id)?
            .map(|path| unlink(&path))
            .transpose();
        self.shared.emit(EngineEvent::EventDeleted(id));
        self.shared.refresh_backlog();
        removed?;
        Ok(())
    }
    pub fn delete_event_audio(&self, id: Uuid) -> Result<()> {
        let _gate = self.shared.persist_gate.lock();
        let removed = self
            .shared
            .store
            .expire_event_audio(id)?
            .map(|path| unlink(&path))
            .transpose();
        self.shared.upsert(id)?;
        removed?;
        Ok(())
    }
    pub fn delete_all_history(&self) -> Result<()> {
        let _guard = self.transitions.lock();
        let recording = matches!(
            self.status().state,
            CaptureState::Recording | CaptureState::Starting | CaptureState::Error
        );
        self.request(|reply| Command::Pause(true, reply))?;
        let gate = self.shared.persist_gate.lock();
        let events = self.shared.store.list_events(None, u32::MAX)?;
        let mut removal_error = None;
        for path in self.shared.store.delete_all_history()? {
            if let Err(error) = unlink(&path) {
                removal_error = Some(error);
            }
        }
        for event in events {
            self.shared.emit(EngineEvent::EventDeleted(event.id));
        }
        self.shared.refresh_backlog();
        drop(gate);
        if recording {
            self.request(|reply| Command::Pause(false, reply))?;
        }
        if let Some(error) = removal_error {
            return Err(error);
        }
        Ok(())
    }
    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        self.models.list()
    }
    pub fn download_model(&self, model_id: &str) -> Result<()> {
        self.models.download(model_id)
    }
    pub fn cancel_model_download(&self, model_id: &str) {
        self.models.cancel(model_id);
    }
    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        self.models.delete(model_id)?;
        let _ = self.shared.wake.try_send(());
        Ok(())
    }
    pub fn storage_stats(&self) -> Result<StorageStats> {
        let store = &self.shared.store;
        let (events, transcribed_events) = store.counts()?;
        let (audio_bytes, audio_files) = store.audio_usage()?;
        let database_bytes = ["tuldmi.db", "tuldmi.db-wal", "tuldmi.db-shm"]
            .iter()
            .map(|name| std::fs::metadata(self.data_dir().join(name)).map_or(0, |m| m.len()))
            .sum();
        let models_bytes = store
            .installed_models()?
            .iter()
            .map(|m| std::fs::metadata(&m.path).map_or(0, |m| m.len()))
            .sum();
        Ok(StorageStats {
            data_dir: self.data_dir().to_string_lossy().into_owned(),
            database_bytes,
            audio_bytes,
            audio_files,
            models_bytes,
            events,
            transcribed_events,
            free_disk_bytes: fs4::available_space(self.data_dir()).ok(),
        })
    }
    pub fn export(
        &self,
        format: ExportFormat,
        from: Option<i64>,
        to: Option<i64>,
        dest_dir: &Path,
    ) -> Result<PathBuf> {
        let mut events = self.shared.store.list_events(None, u32::MAX)?;
        events.retain(|e| {
            from.is_none_or(|t| e.started_at >= t) && to.is_none_or(|t| e.started_at <= t)
        });
        events.reverse();
        std::fs::create_dir_all(dest_dir)?;
        let ext = if format == ExportFormat::Json {
            "json"
        } else {
            "md"
        };
        let path = dest_dir.join(format!(
            "tuldMi-{}-{}.{ext}",
            from.map_or_else(|| "all".into(), |n| n.to_string()),
            to.map_or_else(|| "all".into(), |n| n.to_string())
        ));
        let text = if format == ExportFormat::Json {
            serde_json::to_string_pretty(&events).map_err(|e| Error::Other(e.to_string()))?
        } else {
            let mut text = String::from("# tuldMi transcripts\n");
            let mut day = String::new();
            for event in events {
                let date = chrono::DateTime::from_timestamp_millis(event.started_at)
                    .ok_or_else(|| Error::Other("event timestamp out of range".into()))?
                    .with_timezone(&chrono::Local);
                let next = date.format("%Y-%m-%d").to_string();
                if next != day {
                    text.push_str(&format!("\n## {next}\n\n"));
                    day = next;
                }
                let label = if event.source == Source::Mic {
                    "Me"
                } else {
                    "Remote"
                };
                for segment in event.segments {
                    let offset = chrono::Duration::try_milliseconds(segment.start_ms)
                        .ok_or_else(|| Error::Other("transcript timestamp out of range".into()))?;
                    let at = date
                        .checked_add_signed(offset)
                        .ok_or_else(|| Error::Other("transcript timestamp out of range".into()))?;
                    text.push_str(&format!(
                        "**{} {label}**: {}\n\n",
                        at.format("%H:%M"),
                        segment.text
                    ));
                }
            }
            text
        };
        std::fs::write(&path, text)?;
        Ok(path)
    }
    pub fn data_dir(&self) -> &Path {
        self.shared.store.data_dir()
    }
    pub fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        // Worker budget; downloads and the event dispatcher get short grace periods after it.
        let deadline = Instant::now() + Duration::from_millis(2600);
        self.models.cancel_all();
        self.shared.cancel_asr.store(true, Ordering::Release);
        let (reply, done) = bounded(1);
        let _ = self
            .commands
            .send_timeout(Command::Shutdown(reply), Duration::from_millis(50));
        let _ = done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        self.shared.quit.store(true, Ordering::Release);
        let _ = self.shared.wake.try_send(());
        let mut threads = self.threads.lock();
        while threads.iter().any(|t| !t.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        for thread in threads.drain(..) {
            if thread.is_finished() {
                if thread.join().is_err() {
                    self.shared.notice("engine worker failed");
                }
            } else {
                self.shared.notice(
                    "worker is finishing in the background; unfinished jobs recover on restart",
                );
            }
        }
        drop(threads);
        // Cancelled downloads notice within ~250 ms and remove their `.part`.
        self.models.shutdown(
            deadline
                .saturating_duration_since(Instant::now())
                .clamp(Duration::from_millis(100), Duration::from_secs(1)),
        );
        if let Some(thread) = self.events_thread.lock().take() {
            self.events.finish(thread, DISPATCH_GRACE);
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
