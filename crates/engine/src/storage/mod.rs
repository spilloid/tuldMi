//! SQLite persistence: sessions, speech events, transcripts (FTS5), jobs, settings, models.
//!
//! One `Store` per process, shared behind `Arc`. Internally a single connection behind a mutex
//! (WAL mode, `foreign_keys=ON`, `busy_timeout`); every public method is one short transaction.
//! Audio files are referenced by path relative to `data_dir`; `Store` never deletes files
//! itself — methods that drop audio return the absolute paths for the caller to remove, so
//! file I/O stays out of the lock and a crash between DB commit and unlink leaves only an
//! orphan file (swept by `orphan_audio_files`), never a dangling reference.
//!
//! Migrations live in `migrations/NNNN_*.sql`, applied in order inside one transaction each,
//! tracked with `PRAGMA user_version`. Never edit a shipped migration; add a new one.

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::types::*;
use crate::Result;

pub const LATEST_SCHEMA: u32 = 1;
/// Automatic retries for a transcription job before it is marked failed.
pub const MAX_ATTEMPTS: u32 = 3;
pub const DEFAULT_SEARCH_LIMIT: u32 = 50;

#[derive(Debug, Clone)]
pub struct NewAudio {
    /// Absolute path; must be inside `data_dir`.
    pub path: PathBuf,
    pub sample_rate: u32,
    pub duration_ms: i64,
    pub bytes: i64,
    pub peaks: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub session_id: Uuid,
    pub source: Source,
    pub started_at: i64,
    pub ended_at: i64,
    pub audio: Option<NewAudio>,
}

#[derive(Debug, Clone)]
pub struct NewSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub event_id: Uuid,
    pub attempts: u32,
    /// Absolute path of the event's audio. None if the audio is gone (job will fail).
    pub audio_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledModel {
    pub model_id: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

pub struct Store {
    // Implementation-defined.
}

impl Store {
    /// Open or create `data_dir/tuldmi.db`, applying pending migrations and seeding the two
    /// default speakers ("Me"/self, "Remote"/remote) if absent.
    pub fn open(data_dir: &Path) -> Result<Store> {
        let _ = data_dir;
        todo!()
    }

    pub fn data_dir(&self) -> &Path {
        todo!()
    }
    pub fn schema_version(&self) -> Result<u32> {
        todo!()
    }

    // --- sessions ---------------------------------------------------------------------------
    pub fn start_session(&self, started_at: i64) -> Result<Session> {
        let _ = started_at;
        todo!()
    }
    pub fn end_session(&self, id: Uuid, ended_at: i64) -> Result<()> {
        let _ = (id, ended_at);
        todo!()
    }
    /// Crash recovery: close sessions left open, using their last event's `ended_at` (or their
    /// own `started_at` if they have none). Returns how many were closed.
    pub fn close_dangling_sessions(&self) -> Result<u32> {
        todo!()
    }

    // --- events -----------------------------------------------------------------------------
    pub fn default_speaker(&self, source: Source) -> Result<Speaker> {
        let _ = source;
        todo!()
    }
    /// Insert an event (status `queued`, default speaker for its source), its audio asset if
    /// any, and a queued transcription job — atomically.
    pub fn insert_event(&self, new: &NewEvent) -> Result<SpeechEvent> {
        let _ = new;
        todo!()
    }
    pub fn get_event(&self, id: Uuid) -> Result<Option<SpeechEvent>> {
        let _ = id;
        todo!()
    }
    /// Newest first. `before`: only events with `started_at < before`.
    pub fn list_events(&self, before: Option<i64>, limit: u32) -> Result<Vec<SpeechEvent>> {
        let _ = (before, limit);
        todo!()
    }
    /// Replace the event's segments; status becomes `done`, or `empty` if every segment's text
    /// is blank (or there are none). Marks the job done. Returns the updated event.
    pub fn save_transcript(
        &self,
        event_id: Uuid,
        segments: &[NewSegment],
        language: Option<&str>,
        model_id: &str,
    ) -> Result<SpeechEvent> {
        let _ = (event_id, segments, language, model_id);
        todo!()
    }
    /// Deletes the event, its segments, job, and asset row. Returns the audio path to unlink.
    pub fn delete_event(&self, id: Uuid) -> Result<Option<PathBuf>> {
        let _ = id;
        todo!()
    }

    // --- jobs -------------------------------------------------------------------------------
    /// Claim the oldest queued job: job -> running (attempts += 1), event -> transcribing.
    pub fn next_job(&self) -> Result<Option<Job>> {
        todo!()
    }
    /// Record a failed attempt. If `retryable` and attempts < MAX_ATTEMPTS the job is requeued
    /// (event back to `queued`); otherwise job and event become `failed` with `error`.
    pub fn fail_job(&self, event_id: Uuid, error: &str, retryable: bool) -> Result<()> {
        let _ = (event_id, error, retryable);
        todo!()
    }
    /// User-initiated retry of a failed (or empty) event: requeue with attempts reset.
    /// Returns false if the event doesn't exist or its audio is gone.
    pub fn retry_event(&self, event_id: Uuid) -> Result<bool> {
        let _ = event_id;
        todo!()
    }
    /// Startup recovery: running jobs -> queued, transcribing events -> queued.
    pub fn recover_interrupted_jobs(&self) -> Result<u32> {
        todo!()
    }
    /// Count of queued + running jobs.
    pub fn backlog(&self) -> Result<u32> {
        todo!()
    }

    // --- search -----------------------------------------------------------------------------
    /// Full-text search. User text is treated as plain words (never raw FTS syntax): each
    /// token is quoted, the last token gets prefix matching, all tokens must match. Any input,
    /// including quotes, operators and punctuation, must not error. Empty/whitespace text with
    /// no other filter returns no hits. Ordered by relevance, ties by newest.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let _ = query;
        todo!()
    }

    // --- retention / deletion ---------------------------------------------------------------
    /// Expire audio of events that ended before `cutoff` and are finished (`done`, `empty`, or
    /// `failed`). Queued/transcribing audio is never expired. Transcripts are kept. Returns
    /// absolute paths to unlink.
    pub fn expire_audio_before(&self, cutoff: i64) -> Result<Vec<PathBuf>> {
        let _ = cutoff;
        todo!()
    }
    /// Expire one event's audio (DeleteAfterTranscription, or the user's "delete audio").
    pub fn expire_event_audio(&self, event_id: Uuid) -> Result<Option<PathBuf>> {
        let _ = event_id;
        todo!()
    }
    /// Delete every session/event/segment/job/asset (settings and models kept).
    /// Returns audio paths to unlink.
    pub fn delete_all_history(&self) -> Result<Vec<PathBuf>> {
        todo!()
    }
    /// Audio files under `data_dir/audio` that no live asset references.
    pub fn orphan_audio_files(&self) -> Result<Vec<PathBuf>> {
        todo!()
    }
    /// (total events, events with status done)
    pub fn counts(&self) -> Result<(u64, u64)> {
        todo!()
    }
    /// Sum of `bytes` over live (unexpired) audio assets, and their count.
    pub fn audio_usage(&self) -> Result<(u64, u64)> {
        todo!()
    }

    // --- settings ---------------------------------------------------------------------------
    /// Stored settings merged over defaults (unknown/missing keys -> default).
    pub fn load_settings(&self) -> Result<Settings> {
        todo!()
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let _ = settings;
        todo!()
    }

    // --- models -----------------------------------------------------------------------------
    pub fn record_model(&self, model: &InstalledModel) -> Result<()> {
        let _ = model;
        todo!()
    }
    pub fn remove_model(&self, model_id: &str) -> Result<()> {
        let _ = model_id;
        todo!()
    }
    pub fn installed_models(&self) -> Result<Vec<InstalledModel>> {
        todo!()
    }
}
