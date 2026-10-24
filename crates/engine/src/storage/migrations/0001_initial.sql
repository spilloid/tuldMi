-- tuldMi schema v1. UUIDs stored as TEXT (hyphenated, lowercase). Times are INTEGER
-- milliseconds since the Unix epoch (UTC). Large binaries (audio) live on disk, never here.

CREATE TABLE sessions (
    id          TEXT PRIMARY KEY,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER
);

-- Who spoke. Independent of source: today mic -> 'self', system -> 'remote'; future diarized or
-- user-named people get kind 'person' and optionally an embedding.
CREATE TABLE speakers (
    id          TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('self', 'remote', 'person')),
    embedding   BLOB,
    created_at  INTEGER NOT NULL
);

CREATE TABLE audio_assets (
    id           TEXT PRIMARY KEY,
    -- Path relative to the data directory; NULL once expired/deleted.
    rel_path     TEXT,
    codec        TEXT NOT NULL DEFAULT 'opus',
    sample_rate  INTEGER NOT NULL,
    duration_ms  INTEGER NOT NULL,
    bytes        INTEGER NOT NULL,
    peaks        BLOB NOT NULL,
    created_at   INTEGER NOT NULL,
    expired_at   INTEGER
);

-- One VAD-delimited span of speech from one source.
CREATE TABLE audio_events (
    id            TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source        TEXT NOT NULL CHECK (source IN ('mic', 'system')),
    speaker_id    TEXT REFERENCES speakers(id) ON DELETE SET NULL,
    started_at    INTEGER NOT NULL,
    ended_at      INTEGER NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('queued', 'transcribing', 'done', 'empty', 'failed')),
    asset_id      TEXT REFERENCES audio_assets(id) ON DELETE SET NULL,
    language      TEXT,
    model_id      TEXT,
    error         TEXT,
    created_at    INTEGER NOT NULL
);
CREATE INDEX audio_events_started ON audio_events(started_at DESC);
CREATE INDEX audio_events_status ON audio_events(status);

-- `seq` is the stable integer key the FTS index points at (implicit rowids can be renumbered
-- by VACUUM on tables without an INTEGER PRIMARY KEY, which would corrupt external content).
CREATE TABLE transcript_segments (
    seq         INTEGER PRIMARY KEY,
    id          TEXT NOT NULL UNIQUE,
    event_id    TEXT NOT NULL REFERENCES audio_events(id) ON DELETE CASCADE,
    start_ms    INTEGER NOT NULL,
    end_ms      INTEGER NOT NULL,
    text        TEXT NOT NULL,
    confidence  REAL
);
CREATE INDEX transcript_segments_event ON transcript_segments(event_id, start_ms);

CREATE VIRTUAL TABLE transcript_fts USING fts5(
    text,
    content = 'transcript_segments',
    content_rowid = 'seq',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER transcript_segments_ai AFTER INSERT ON transcript_segments BEGIN
    INSERT INTO transcript_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TRIGGER transcript_segments_ad AFTER DELETE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES ('delete', old.seq, old.text);
END;
CREATE TRIGGER transcript_segments_au AFTER UPDATE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES ('delete', old.seq, old.text);
    INSERT INTO transcript_fts(rowid, text) VALUES (new.seq, new.text);
END;

CREATE TABLE transcription_jobs (
    event_id     TEXT PRIMARY KEY REFERENCES audio_events(id) ON DELETE CASCADE,
    state        TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed')),
    attempts     INTEGER NOT NULL DEFAULT 0,
    last_error   TEXT,
    enqueued_at  INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX transcription_jobs_state ON transcription_jobs(state, enqueued_at);

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

CREATE TABLE model_installations (
    model_id      TEXT PRIMARY KEY,
    rel_path      TEXT NOT NULL,
    bytes         INTEGER NOT NULL,
    sha256        TEXT NOT NULL,
    installed_at  INTEGER NOT NULL
);
