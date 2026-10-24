//! The running engine: owns capture streams, per-source processing threads, the transcription
//! worker, housekeeping, and the store. All public methods are cheap and non-blocking from the
//! caller's point of view (they may take a short lock); nothing here runs on the UI thread's
//! behalf for longer than a database call.
//!
//! Threads:
//! - one capture stream per enabled source (provider-owned; callback -> bounded channel)
//! - `tuldmi-proc`: drains the capture channel; per source: `ToMono16k` -> `LevelMeter` ->
//!   `SpeechDetector` -> `Segmenter`; finished segments are encoded (`encode::write_ogg_opus`
//!   to `audio/YYYY/MM/DD/<event-uuid>.ogg`), inserted (`Store::insert_event`), announced
//!   (`EventUpserted`), and the transcription worker is woken. Levels are batched to ~30 Hz.
//! - `tuldmi-asr`: loops `Store::next_job`; lazily loads the configured model; decodes the
//!   event's audio; transcribes; `save_transcript`; applies `DeleteAfterTranscription`; emits
//!   upserts and status (backlog/transcriber state). Parks on a condvar/channel when idle or
//!   when no model is installed; failures go through `fail_job` (model-missing = not retryable
//!   until the user installs a model, then queued jobs proceed).
//! - `tuldmi-house`: at start and every 10 minutes: retention (`expire_audio_before`), unlink
//!   returned files, orphan sweep at start; every 3 s: reopen `Unavailable` sources and follow
//!   default-device changes for unpinned sources.
//!
//! State machine: Stopped -> (start/resume) Starting -> Recording; Recording -> (pause) Paused;
//! any -> Error when every enabled source fails (still retrying). A session row spans one
//! Recording run (resume starts a new session). Pause and shutdown flush segmenters first, so
//! in-progress speech is kept. Monitoring opens devices and emits levels with *no* segmenting or
//! storage, and is only allowed while not recording.
//!
//! Shutdown: stop streams, flush and persist in-progress segments, end session, signal threads,
//! join with a timeout (an in-flight transcription is abandoned and recovered as `queued` on
//! next start).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use crate::capture::AudioCaptureProvider;
use crate::storage::Store;
use crate::types::*;
use crate::Result;

pub trait EventSink: Send + Sync + 'static {
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

pub struct Engine {
    // Implementation-defined.
}

impl Engine {
    /// Real devices (cpal).
    pub fn start(config: EngineConfig, sink: Arc<dyn EventSink>) -> Result<Engine> {
        let _ = (config, sink);
        todo!()
    }

    /// Injected provider (tests, future platforms). Startup: open store, recover interrupted
    /// jobs, close dangling sessions, sweep orphans, load settings, spawn threads, then start
    /// capture if `onboarding_complete && capture_on_launch && !suppress_autostart`.
    pub fn start_with_provider(config: EngineConfig, sink: Arc<dyn EventSink>, provider: Arc<dyn AudioCaptureProvider>) -> Result<Engine> {
        let _ = (config, sink, provider);
        todo!()
    }

    pub fn status(&self) -> CaptureStatus {
        todo!()
    }
    /// `false` from Stopped/Paused starts recording.
    pub fn set_paused(&self, paused: bool) -> Result<CaptureStatus> {
        let _ = paused;
        todo!()
    }
    pub fn set_monitoring(&self, enabled: bool) -> Result<CaptureStatus> {
        let _ = enabled;
        todo!()
    }
    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        todo!()
    }
    pub fn settings(&self) -> Settings {
        todo!()
    }
    /// Persists, then applies: reopens sources whose enabled/device changed, rebuilds
    /// segmenters if VAD settings changed (flushing first), swaps the model if `model_id`
    /// changed, runs retention immediately if retention got stricter.
    pub fn update_settings(&self, settings: Settings) -> Result<Settings> {
        let _ = settings;
        todo!()
    }
    pub fn store(&self) -> Arc<Store> {
        todo!()
    }
    pub fn list_events(&self, before: Option<i64>, limit: u32) -> Result<Vec<SpeechEvent>> {
        let _ = (before, limit);
        todo!()
    }
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let _ = query;
        todo!()
    }
    pub fn retry_event(&self, id: Uuid) -> Result<bool> {
        let _ = id;
        todo!()
    }
    /// Deletes rows and the audio file; emits `EventDeleted`.
    pub fn delete_event(&self, id: Uuid) -> Result<()> {
        let _ = id;
        todo!()
    }
    /// Removes the audio file, keeps the transcript; emits `EventUpserted`.
    pub fn delete_event_audio(&self, id: Uuid) -> Result<()> {
        let _ = id;
        todo!()
    }
    pub fn delete_all_history(&self) -> Result<()> {
        todo!()
    }
    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        todo!()
    }
    /// Starts a background download; progress via `ModelProgress` events.
    pub fn download_model(&self, model_id: &str) -> Result<()> {
        let _ = model_id;
        todo!()
    }
    pub fn cancel_model_download(&self, model_id: &str) {
        let _ = model_id;
        todo!()
    }
    pub fn delete_model(&self, model_id: &str) -> Result<()> {
        let _ = model_id;
        todo!()
    }
    pub fn storage_stats(&self) -> Result<StorageStats> {
        todo!()
    }
    /// Writes `tuldMi-<from>-<to>.{md,json}` into `dest_dir` (created if missing).
    pub fn export(&self, format: ExportFormat, from: Option<i64>, to: Option<i64>, dest_dir: &Path) -> Result<PathBuf> {
        let _ = (format, from, to, dest_dir);
        todo!()
    }
    pub fn data_dir(&self) -> &Path {
        todo!()
    }
    /// Idempotent. Also run by `Drop`.
    pub fn shutdown(&self) {
        todo!()
    }
}
