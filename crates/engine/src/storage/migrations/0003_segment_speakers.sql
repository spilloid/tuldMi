-- Remote speaker separation (docs/design/speakers.md). A segment's speaker_id NULL means the
-- event's speaker (Me or Remote). speakers_state: NULL = not separated (feature off, out of
-- order, or never attempted), 'done', or 'failed' (the worker could not process it).
-- speakers.embedding is never written: voice fingerprints stay in memory. speakers.ordinal is
-- the N of "Speaker N" for separated speakers; it outlives renames. speakers.seen_at is the start
-- of the event where the speaker was first shown, which ties the number to its conversation
-- even after every segment showing it is deleted.
ALTER TABLE transcript_segments ADD COLUMN speaker_id TEXT REFERENCES speakers(id) ON DELETE SET NULL;
ALTER TABLE audio_events ADD COLUMN speakers_state TEXT CHECK (speakers_state IN ('done', 'failed'));
CREATE INDEX transcript_segments_speaker ON transcript_segments(speaker_id);
ALTER TABLE speakers ADD COLUMN ordinal INTEGER;
ALTER TABLE speakers ADD COLUMN seen_at INTEGER;
-- Music rejection: an event judged music is `empty` with MUSIC_NOTE as its note. The user can
-- override that ("Transcribe anyway"), which sets transcribe_anyway so the music check is skipped.
ALTER TABLE audio_events ADD COLUMN transcribe_anyway INTEGER NOT NULL DEFAULT 0;
