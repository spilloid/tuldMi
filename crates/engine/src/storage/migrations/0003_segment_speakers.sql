-- Remote speaker separation (docs/design/speakers.md). A segment's speaker_id NULL means the
-- event's speaker (Me or Remote). speakers_state: NULL = not separated (feature off, out of
-- order, or never attempted), 'done', or 'failed' (the worker could not process it).
-- speakers.embedding is never written: voice fingerprints stay in memory.
ALTER TABLE transcript_segments ADD COLUMN speaker_id TEXT REFERENCES speakers(id) ON DELETE SET NULL;
ALTER TABLE audio_events ADD COLUMN speakers_state TEXT CHECK (speakers_state IN ('done', 'failed'));
CREATE INDEX transcript_segments_speaker ON transcript_segments(speaker_id);
