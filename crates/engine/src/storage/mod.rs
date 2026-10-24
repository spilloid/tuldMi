//! SQLite persistence: sessions, speech events, transcripts (FTS5), jobs, settings, models.
//!
//! One connection behind a mutex, with WAL and short transactions. Audio paths are relative
//! on disk and absolute in the API. Deletion commits before the caller unlinks returned paths;
//! a crash in between leaves an orphan that `orphan_audio_files` can recover.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

use crate::types::*;
use crate::{Error, Result};

mod paths;
mod read;
mod search;

#[cfg(test)]
mod tests;

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

#[derive(Debug, Clone, PartialEq)]
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
    data_dir: PathBuf,
    connection: Mutex<Connection>,
}

fn now_ms() -> i64 {
    crate::clock::WallAnchor::now().unix_ms
}

impl Store {
    /// Open or create `data_dir/tuldmi.db`, applying pending migrations and seeding the two
    /// default speakers ("Me"/self, "Remote"/remote) if absent.
    pub fn open(data_dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(data_dir)?;
        let data_dir = std::path::absolute(data_dir)?;
        let mut connection = Connection::open(data_dir.join("tuldmi.db"))?;
        connection.busy_timeout(std::time::Duration::from_millis(5_000))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;

        const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_initial.sql")];
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let version: u32 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version > LATEST_SCHEMA {
                return Err(Error::Other(
                    "database schema is newer than this engine".into(),
                ));
            }
            let next = index as u32 + 1;
            if version < next {
                tx.execute_batch(migration)?;
                tx.pragma_update(None, "user_version", next)?;
            }
            tx.commit()?;
        }
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (label, kind) in [("Me", "self"), ("Remote", "remote")] {
            tx.execute(
                "INSERT INTO speakers (id, label, kind, created_at)
                 SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS (SELECT 1 FROM speakers WHERE kind = ?3)",
                params![Uuid::now_v7().to_string(), label, kind, now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(Store {
            data_dir,
            connection: Mutex::new(connection),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .connection
            .lock()
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    // --- sessions ---------------------------------------------------------------------------
    pub fn start_session(&self, started_at: i64) -> Result<Session> {
        let session = Session {
            id: Uuid::now_v7(),
            started_at,
            ended_at: None,
        };
        self.connection.lock().execute(
            "INSERT INTO sessions (id, started_at) VALUES (?1, ?2)",
            params![session.id.to_string(), started_at],
        )?;
        Ok(session)
    }

    pub fn end_session(&self, id: Uuid, ended_at: i64) -> Result<()> {
        self.connection.lock().execute(
            "UPDATE sessions SET ended_at = ?2 WHERE id = ?1",
            params![id.to_string(), ended_at],
        )?;
        Ok(())
    }

    /// Crash recovery: close sessions left open, using their last event's `ended_at` (or their
    /// own `started_at` if they have none). Returns how many were closed.
    pub fn close_dangling_sessions(&self) -> Result<u32> {
        Ok(self.connection.lock().execute(
            "UPDATE sessions SET ended_at = COALESCE(
                (SELECT MAX(ended_at) FROM audio_events WHERE session_id = sessions.id), started_at)
             WHERE ended_at IS NULL",
            [],
        )? as u32)
    }

    // --- events -----------------------------------------------------------------------------
    pub fn default_speaker(&self, source: Source) -> Result<Speaker> {
        read::default_speaker(&self.connection.lock(), source)
    }

    /// Insert an event (status `queued`, default speaker for its source), its audio asset if
    /// any, and a queued transcription job - atomically.
    pub fn insert_event(&self, new: &NewEvent) -> Result<SpeechEvent> {
        let audio = new
            .audio
            .as_ref()
            .map(|a| paths::relative(&self.data_dir, &a.path))
            .transpose()?;
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = Uuid::now_v7();
        let speaker = read::default_speaker(&tx, new.source)?;
        let created_at = now_ms();
        let asset_id = if let (Some(asset), Some(path)) = (&new.audio, audio) {
            let asset_id = Uuid::now_v7();
            tx.execute(
                "INSERT INTO audio_assets
                 (id, rel_path, sample_rate, duration_ms, bytes, peaks, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    asset_id.to_string(),
                    path,
                    asset.sample_rate,
                    asset.duration_ms,
                    asset.bytes,
                    asset.peaks,
                    created_at
                ],
            )?;
            Some(asset_id.to_string())
        } else {
            None
        };
        tx.execute(
            "INSERT INTO audio_events
             (id, session_id, source, speaker_id, started_at, ended_at, status, asset_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, ?8)",
            params![id.to_string(), new.session_id.to_string(), new.source.as_str(),
                speaker.id.to_string(), new.started_at, new.ended_at, asset_id, created_at],
        )?;
        tx.execute(
            "INSERT INTO transcription_jobs (event_id, state, enqueued_at, updated_at)
             VALUES (?1, 'queued', ?2, ?2)",
            params![id.to_string(), created_at],
        )?;
        let event = read::required_event(&tx, &self.data_dir, id)?;
        tx.commit()?;
        Ok(event)
    }

    pub fn get_event(&self, id: Uuid) -> Result<Option<SpeechEvent>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction()?;
        let event = read::events(&tx, &self.data_dir, "WHERE e.id = ?1", [id.to_string()])?.pop();
        tx.commit()?;
        Ok(event)
    }

    /// Newest first. `before`: only events with `started_at < before`.
    pub fn list_events(&self, before: Option<i64>, limit: u32) -> Result<Vec<SpeechEvent>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction()?;
        let events = read::events(
            &tx,
            &self.data_dir,
            "WHERE (?1 IS NULL OR e.started_at < ?1) ORDER BY e.started_at DESC, e.id DESC LIMIT ?2",
            params![before, limit],
        )?;
        tx.commit()?;
        Ok(events)
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
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = event_id.to_string();
        tx.execute("DELETE FROM transcript_segments WHERE event_id = ?1", [&id])?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO transcript_segments (id, event_id, start_ms, end_ms, text, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for segment in segments {
                insert.execute(params![
                    Uuid::now_v7().to_string(),
                    id,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.confidence
                ])?;
            }
        }
        let status = if segments.iter().any(|s| !s.text.trim().is_empty()) {
            "done"
        } else {
            "empty"
        };
        tx.execute(
            "UPDATE audio_events SET status = ?2, language = ?3, model_id = ?4, error = NULL
             WHERE id = ?1",
            params![id, status, language, model_id],
        )?;
        tx.execute(
            "UPDATE transcription_jobs SET state = 'done', last_error = NULL, updated_at = ?2
             WHERE event_id = ?1",
            params![id, now_ms()],
        )?;
        let event = read::required_event(&tx, &self.data_dir, event_id)?;
        tx.commit()?;
        Ok(event)
    }

    /// Deletes the event, its segments, job, and asset row. Returns the audio path to unlink.
    pub fn delete_event(&self, id: Uuid) -> Result<Option<PathBuf>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let path = read::live_path(&tx, &self.data_dir, id)?;
        let asset_id: Option<String> = tx
            .query_row(
                "SELECT asset_id FROM audio_events WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        tx.execute("DELETE FROM audio_events WHERE id = ?1", [id.to_string()])?;
        if let Some(asset_id) = asset_id {
            tx.execute("DELETE FROM audio_assets WHERE id = ?1", [asset_id])?;
        }
        tx.commit()?;
        Ok(path)
    }

    // --- jobs -------------------------------------------------------------------------------
    /// Claim the oldest queued job: job -> running (attempts += 1), event -> transcribing.
    pub fn next_job(&self) -> Result<Option<Job>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next = tx
            .query_row(
                "SELECT event_id, attempts FROM transcription_jobs WHERE state = 'queued'
             ORDER BY enqueued_at, event_id LIMIT 1",
                [],
                |row| Ok((read::uuid(row, 0)?, row.get::<_, u32>(1)?)),
            )
            .optional()?;
        let job = if let Some((event_id, attempts)) = next {
            tx.execute(
                "UPDATE transcription_jobs SET state = 'running', attempts = attempts + 1,
                 updated_at = ?2 WHERE event_id = ?1",
                params![event_id.to_string(), now_ms()],
            )?;
            tx.execute(
                "UPDATE audio_events SET status = 'transcribing', error = NULL WHERE id = ?1",
                [event_id.to_string()],
            )?;
            Some(Job {
                event_id,
                attempts: attempts + 1,
                audio_path: read::live_path(&tx, &self.data_dir, event_id)?,
            })
        } else {
            None
        };
        tx.commit()?;
        Ok(job)
    }

    /// Record a failed attempt. If `retryable` and attempts < MAX_ATTEMPTS the job is requeued
    /// (event back to `queued`); otherwise job and event become `failed` with `error`.
    pub fn fail_job(&self, event_id: Uuid, error: &str, retryable: bool) -> Result<()> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = event_id.to_string();
        let attempts: u32 = tx.query_row(
            "SELECT attempts FROM transcription_jobs WHERE event_id = ?1",
            [&id],
            |row| row.get(0),
        )?;
        let state = if retryable && attempts < MAX_ATTEMPTS {
            "queued"
        } else {
            "failed"
        };
        tx.execute(
            "UPDATE transcription_jobs SET state = ?2, last_error = ?3, updated_at = ?4
             WHERE event_id = ?1",
            params![id, state, error, now_ms()],
        )?;
        tx.execute(
            "UPDATE audio_events SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, state, error],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// User-initiated retry of a failed (or empty) event: requeue with attempts reset.
    /// Returns false if the event doesn't exist or its audio is gone.
    pub fn retry_event(&self, event_id: Uuid) -> Result<bool> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = event_id.to_string();
        let eligible: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM audio_events e JOIN audio_assets a ON a.id = e.asset_id
             WHERE e.id = ?1 AND e.status IN ('failed', 'empty')
             AND a.expired_at IS NULL AND a.rel_path IS NOT NULL)",
            [&id],
            |row| row.get(0),
        )?;
        if eligible {
            tx.execute(
                "INSERT INTO transcription_jobs (event_id, state, attempts, enqueued_at, updated_at)
                 VALUES (?1, 'queued', 0, ?2, ?2) ON CONFLICT(event_id) DO UPDATE SET
                 state = 'queued', attempts = 0, last_error = NULL,
                 enqueued_at = excluded.enqueued_at, updated_at = excluded.updated_at",
                params![id, now_ms()],
            )?;
            tx.execute(
                "UPDATE audio_events SET status = 'queued', error = NULL WHERE id = ?1",
                [&id],
            )?;
        }
        tx.commit()?;
        Ok(eligible)
    }

    /// Startup recovery: running jobs -> queued, transcribing events -> queued.
    pub fn recover_interrupted_jobs(&self) -> Result<u32> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = tx.execute(
            "UPDATE transcription_jobs SET state = 'queued', updated_at = ?1 WHERE state = 'running'",
            [now_ms()],
        )?;
        tx.execute(
            "UPDATE audio_events SET status = 'queued' WHERE status = 'transcribing'",
            [],
        )?;
        tx.commit()?;
        Ok(count as u32)
    }

    /// Count of queued + running jobs.
    pub fn backlog(&self) -> Result<u32> {
        Ok(self.connection.lock().query_row(
            "SELECT COUNT(*) FROM transcription_jobs WHERE state IN ('queued', 'running')",
            [],
            |row| row.get(0),
        )?)
    }

    // --- search -----------------------------------------------------------------------------
    /// Full-text search. User text is treated as plain words (never raw FTS syntax): each
    /// token is quoted, the last token gets prefix matching, all tokens must match. Any input,
    /// including quotes, operators and punctuation, must not error. Empty/whitespace text with
    /// no other filter returns no hits. Ordered by relevance, ties by newest.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        search::search(&self.connection.lock(), query)
    }

    // --- retention / deletion ---------------------------------------------------------------
    /// Expire audio of events that ended before `cutoff` and are finished (`done`, `empty`, or
    /// `failed`). Queued/transcribing audio is never expired. Transcripts are kept. Returns
    /// absolute paths to unlink.
    pub fn expire_audio_before(&self, cutoff: i64) -> Result<Vec<PathBuf>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let paths = {
            let mut statement = tx.prepare(
                "UPDATE audio_assets SET rel_path = NULL, expired_at = ?2 WHERE id IN (
                 SELECT a.id FROM audio_assets a JOIN audio_events e ON e.asset_id = a.id
                 WHERE a.rel_path IS NOT NULL AND a.expired_at IS NULL AND e.ended_at < ?1
                 AND e.status IN ('done', 'empty', 'failed'))",
            )?;
            // Capture paths before clearing the references in the same transaction.
            let paths = read::paths(
                &tx,
                &self.data_dir,
                "SELECT a.rel_path FROM audio_assets a JOIN audio_events e ON e.asset_id = a.id
                 WHERE a.rel_path IS NOT NULL AND a.expired_at IS NULL AND e.ended_at < ?1
                 AND e.status IN ('done', 'empty', 'failed')",
                [cutoff],
            )?;
            statement.execute(params![cutoff, now_ms()])?;
            paths
        };
        tx.commit()?;
        Ok(paths)
    }

    /// Expire one event's audio (DeleteAfterTranscription, or the user's "delete audio").
    pub fn expire_event_audio(&self, event_id: Uuid) -> Result<Option<PathBuf>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let path = read::live_path(&tx, &self.data_dir, event_id)?;
        tx.execute(
            "UPDATE audio_assets SET rel_path = NULL, expired_at = ?2
             WHERE id = (SELECT asset_id FROM audio_events WHERE id = ?1)
             AND expired_at IS NULL AND rel_path IS NOT NULL",
            params![event_id.to_string(), now_ms()],
        )?;
        tx.commit()?;
        Ok(path)
    }

    /// Delete every session/event/segment/job/asset (settings and models kept).
    /// Returns audio paths to unlink.
    pub fn delete_all_history(&self) -> Result<Vec<PathBuf>> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let paths = read::paths(
            &tx,
            &self.data_dir,
            "SELECT rel_path FROM audio_assets WHERE expired_at IS NULL AND rel_path IS NOT NULL",
            [],
        )?;
        tx.execute("DELETE FROM audio_events", [])?;
        tx.execute("DELETE FROM audio_assets", [])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.commit()?;
        Ok(paths)
    }

    /// Audio files under `data_dir/audio` that no live asset references.
    pub fn orphan_audio_files(&self) -> Result<Vec<PathBuf>> {
        let live = read::paths(
            &self.connection.lock(),
            &self.data_dir,
            "SELECT rel_path FROM audio_assets WHERE expired_at IS NULL AND rel_path IS NOT NULL",
            [],
        )?;
        paths::orphans(&self.data_dir, live)
    }

    /// (total events, events with status done)
    pub fn counts(&self) -> Result<(u64, u64)> {
        Ok(self.connection.lock().query_row(
            "SELECT COUNT(*), COUNT(CASE WHEN status = 'done' THEN 1 END) FROM audio_events",
            [],
            |row| Ok((read::unsigned(row, 0)?, read::unsigned(row, 1)?)),
        )?)
    }

    /// Sum of `bytes` over live (unexpired) audio assets, and their count.
    pub fn audio_usage(&self) -> Result<(u64, u64)> {
        Ok(self.connection.lock().query_row(
            "SELECT COALESCE(SUM(bytes), 0), COUNT(*) FROM audio_assets
             WHERE expired_at IS NULL AND rel_path IS NOT NULL",
            [],
            |row| Ok((read::unsigned(row, 0)?, read::unsigned(row, 1)?)),
        )?)
    }

    // --- settings ---------------------------------------------------------------------------
    /// Stored settings merged over defaults (unknown/missing keys -> default).
    pub fn load_settings(&self) -> Result<Settings> {
        let value: Option<String> = self
            .connection
            .lock()
            .query_row(
                "SELECT value FROM settings WHERE key = 'settings'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match value {
            None => Ok(Settings::default()),
            Some(value) => match serde_json::from_str(&value) {
                Ok(settings) => Ok(settings),
                Err(_) => {
                    tracing::warn!("invalid stored settings; using defaults");
                    Ok(Settings::default())
                }
            },
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let value = serde_json::to_string(settings)
            .map_err(|_| Error::Other("could not serialize settings".into()))?;
        self.connection.lock().execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [value],
        )?;
        Ok(())
    }

    // --- models -----------------------------------------------------------------------------
    pub fn record_model(&self, model: &InstalledModel) -> Result<()> {
        let relative = paths::relative(&self.data_dir, &model.path)?;
        let bytes = i64::try_from(model.bytes)
            .map_err(|_| Error::Other("model size exceeds SQLite integer range".into()))?;
        self.connection.lock().execute(
            "INSERT INTO model_installations (model_id, rel_path, bytes, sha256, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(model_id) DO UPDATE SET
             rel_path = excluded.rel_path, bytes = excluded.bytes, sha256 = excluded.sha256,
             installed_at = excluded.installed_at",
            params![model.model_id, relative, bytes, model.sha256, now_ms()],
        )?;
        Ok(())
    }

    pub fn remove_model(&self, model_id: &str) -> Result<()> {
        self.connection.lock().execute(
            "DELETE FROM model_installations WHERE model_id = ?1",
            [model_id],
        )?;
        Ok(())
    }

    pub fn installed_models(&self) -> Result<Vec<InstalledModel>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT model_id, rel_path, bytes, sha256 FROM model_installations ORDER BY model_id",
        )?;
        let models = statement
            .query_map([], |row| {
                Ok(InstalledModel {
                    model_id: row.get(0)?,
                    path: self.data_dir.join(row.get::<_, String>(1)?),
                    bytes: read::unsigned(row, 2)?,
                    sha256: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(models)
    }
}
