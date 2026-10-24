//! Row decoding and batched transcript hydration shared by the public operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Params, Row, params_from_iter};
use uuid::Uuid;

use super::paths;
use crate::types::*;
use crate::{Error, Result};

pub(super) fn uuid(row: &Row<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let value: String = row.get(index)?;
    Uuid::parse_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

pub(super) fn unsigned(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
pub(super) fn source(row: &Row<'_>, index: usize) -> rusqlite::Result<Source> {
    let value: String = row.get(index)?;
    Source::parse(&value).ok_or_else(|| invalid_value(index, "invalid stored audio source"))
}

fn invalid_value(index: usize, message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )),
    )
}

pub(super) fn default_speaker(connection: &Connection, source: Source) -> Result<Speaker> {
    let kind = match source {
        Source::Mic => "self",
        Source::System => "remote",
    };
    Ok(connection.query_row(
        "SELECT id, label, kind FROM speakers WHERE kind = ?1 ORDER BY created_at, id LIMIT 1",
        [kind],
        |row| {
            Ok(Speaker {
                id: uuid(row, 0)?,
                label: row.get(1)?,
                kind: row.get(2)?,
            })
        },
    )?)
}

pub(super) fn required_event(
    connection: &Connection,
    root: &Path,
    id: Uuid,
) -> Result<SpeechEvent> {
    events(connection, root, "WHERE e.id = ?1", [id.to_string()])?
        .pop()
        .ok_or_else(|| Error::Other("event does not exist".into()))
}

pub(super) fn events(
    connection: &Connection,
    root: &Path,
    clause: &str,
    parameters: impl Params,
) -> Result<Vec<SpeechEvent>> {
    // `clause` is internal SQL, never user input. Every value is bound separately.
    let sql = format!(
        "SELECT e.id, e.session_id, e.source, e.started_at, e.ended_at, e.status,
         e.language, e.model_id, e.error, s.id, s.label, s.kind,
         a.id, a.rel_path, a.duration_ms, a.bytes, a.peaks, a.expired_at
         FROM audio_events e LEFT JOIN speakers s ON s.id = e.speaker_id
         LEFT JOIN audio_assets a ON a.id = e.asset_id {clause}"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut events = statement
        .query_map(parameters, |row| {
            let speaker = if row.get::<_, Option<String>>(9)?.is_some() {
                Some(Speaker {
                    id: uuid(row, 9)?,
                    label: row.get(10)?,
                    kind: row.get(11)?,
                })
            } else {
                None
            };
            let expired = row.get::<_, Option<i64>>(17)?.is_some();
            let path: Option<String> = row.get(13)?;
            let audio = if let Some(path) = path
                .filter(|_| !expired)
                .and_then(|path| paths::stored(root, &path))
            {
                Some(AudioRef {
                    asset_id: uuid(row, 12)?,
                    path: path.to_string_lossy().into_owned(),
                    duration_ms: row.get(14)?,
                    bytes: row.get(15)?,
                    peaks: row.get(16)?,
                })
            } else {
                None
            };
            let status: String = row.get(5)?;
            Ok(SpeechEvent {
                id: uuid(row, 0)?,
                session_id: uuid(row, 1)?,
                source: source(row, 2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                status: EventStatus::parse(&status)
                    .ok_or_else(|| invalid_value(5, "invalid stored event status"))?,
                speaker,
                audio,
                audio_expired: expired,
                segments: Vec::new(),
                language: row.get(6)?,
                model_id: row.get(7)?,
                error: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Stay below even SQLite's older variable limit. A 200-event page uses one segment query.
    for batch in events.chunks_mut(400) {
        let indices: HashMap<Uuid, usize> =
            batch.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
        let placeholders = vec!["?"; batch.len()].join(",");
        let ids: Vec<String> = batch.iter().map(|e| e.id.to_string()).collect();
        let mut statement = connection.prepare(&format!(
            "SELECT id, event_id, start_ms, end_ms, text, confidence FROM transcript_segments
             WHERE event_id IN ({placeholders}) ORDER BY event_id, start_ms, seq"
        ))?;
        let segments = statement.query_map(params_from_iter(ids.iter()), |row| {
            Ok(TranscriptSegment {
                id: uuid(row, 0)?,
                event_id: uuid(row, 1)?,
                start_ms: row.get(2)?,
                end_ms: row.get(3)?,
                text: row.get(4)?,
                confidence: row.get(5)?,
            })
        })?;
        for segment in segments {
            let segment = segment?;
            if let Some(&index) = indices.get(&segment.event_id) {
                batch[index].segments.push(segment);
            }
        }
    }
    Ok(events)
}

pub(super) fn live_path(connection: &Connection, root: &Path, id: Uuid) -> Result<Option<PathBuf>> {
    let path: Option<String> = connection
        .query_row(
            "SELECT a.rel_path FROM audio_events e JOIN audio_assets a ON a.id = e.asset_id
         WHERE e.id = ?1 AND a.expired_at IS NULL AND a.rel_path IS NOT NULL",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(path.and_then(|path| paths::stored(root, &path)))
}

pub(super) fn paths(
    connection: &Connection,
    root: &Path,
    sql: &str,
    parameters: impl Params,
) -> Result<Vec<PathBuf>> {
    let mut statement = connection.prepare(sql)?;
    let paths = statement
        .query_map(parameters, |row| {
            Ok(paths::stored(root, &row.get::<_, String>(0)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths.into_iter().flatten().collect())
}
