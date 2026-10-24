//! Plain-text query construction. SQL and FTS syntax are never accepted from the caller.

use rusqlite::{Connection, params};

use crate::Result;
use crate::types::*;

use super::{DEFAULT_SEARCH_LIMIT, read};

fn match_text(text: &str) -> String {
    let mut tokens: Vec<String> = text
        .split_whitespace()
        .filter_map(|token| {
            let cleaned: String = token
                .chars()
                .filter(|ch| ch.is_alphabetic() || ch.is_numeric() || *ch == '\'')
                .collect();
            if cleaned.is_empty() {
                None
            } else {
                Some(format!("\"{cleaned}\""))
            }
        })
        .collect();
    if let Some(last) = tokens.last_mut() {
        last.push('*');
    }
    tokens.join(" ")
}

pub(super) fn search(connection: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    let text = match_text(&query.text);
    let has_text = !text.is_empty();
    if !has_text && query.sources.is_empty() && query.from.is_none() && query.to.is_none() {
        return Ok(Vec::new());
    }
    let all_sources = query.sources.is_empty();
    let mic = all_sources || query.sources.contains(&Source::Mic);
    let system = all_sources || query.sources.contains(&Source::System);
    let limit = query.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(200);
    // Empty text with filters lists matching segments, with an unhighlighted text snippet.
    let sql = if has_text {
        "SELECT e.id, s.id, e.source, e.started_at, s.start_ms,
         snippet(transcript_fts, 0, ?1, ?2, '…', 16), bm25(transcript_fts)
         FROM transcript_fts JOIN transcript_segments s ON s.seq = transcript_fts.rowid
         JOIN audio_events e ON e.id = s.event_id
         WHERE transcript_fts MATCH ?3 AND (?4 OR e.source != 'mic')
         AND (?5 OR e.source != 'system') AND (?6 IS NULL OR e.started_at >= ?6)
         AND (?7 IS NULL OR e.started_at <= ?7)
         ORDER BY bm25(transcript_fts), e.started_at DESC, e.id DESC, s.start_ms LIMIT ?8"
    } else {
        "SELECT e.id, s.id, e.source, e.started_at, s.start_ms,
         snippet(transcript_fts, 0, ?1, ?2, '…', 16), 0.0
         FROM transcript_fts JOIN transcript_segments s ON s.seq = transcript_fts.rowid
         JOIN audio_events e ON e.id = s.event_id
         WHERE ?3 = ''
         AND (?4 OR e.source != 'mic') AND (?5 OR e.source != 'system')
         AND (?6 IS NULL OR e.started_at >= ?6) AND (?7 IS NULL OR e.started_at <= ?7)
         ORDER BY e.started_at DESC, e.id DESC, s.start_ms LIMIT ?8"
    };
    let mut statement = connection.prepare(sql)?;
    let hits = statement
        .query_map(
            params![
                HIGHLIGHT_START.to_string(),
                HIGHLIGHT_END.to_string(),
                text,
                mic,
                system,
                query.from,
                query.to,
                limit
            ],
            |row| {
                let started_at: i64 = row.get(3)?;
                let start_ms: i64 = row.get(4)?;
                Ok(SearchHit {
                    event_id: read::uuid(row, 0)?,
                    segment_id: read::uuid(row, 1)?,
                    source: read::source(row, 2)?,
                    at: started_at.saturating_add(start_ms),
                    snippet: row.get(5)?,
                    rank: row.get(6)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(hits)
}
