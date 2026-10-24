//! Plain-text query construction. SQL and FTS syntax are never accepted from the caller.

use rusqlite::{Connection, params};

use crate::Result;
use crate::types::*;

use super::{DEFAULT_SEARCH_LIMIT, read};

pub(super) fn init_tokenizer(connection: &Connection) -> Result<()> {
    // TEMP storage is configured as MEMORY at open. Use the index's actual tokenizer on
    // the store's single connection, without persisting query text or duplicating Unicode
    // classification rules. The contentless table only needs token positions.
    connection.execute_batch(
        "CREATE VIRTUAL TABLE temp.search_input USING fts5(
             text, content = '', tokenize = 'unicode61 remove_diacritics 2');
         CREATE VIRTUAL TABLE temp.search_tokens USING fts5vocab('temp', 'search_input', 'instance');",
    )?;
    Ok(())
}

fn match_text(connection: &Connection, text: &str) -> Result<String> {
    // Clear a previous query even if its token read failed.
    connection.execute(
        "INSERT INTO temp.search_input(search_input) VALUES ('delete-all')",
        [],
    )?;
    connection.execute("INSERT INTO temp.search_input(text) VALUES (?1)", [text])?;
    let mut tokens = {
        let mut statement =
            connection.prepare("SELECT term FROM temp.search_tokens ORDER BY offset")?;
        statement
            .query_map([], |row| {
                let token: String = row.get(0)?;
                Ok(format!("\"{}\"", token.replace('"', "\"\"")))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    connection.execute(
        "INSERT INTO temp.search_input(search_input) VALUES ('delete-all')",
        [],
    )?;
    if let Some(last) = tokens.last_mut() {
        last.push('*');
    }
    Ok(tokens.join(" "))
}

pub(super) fn search(connection: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    let text = match_text(connection, &query.text)?;
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
         snippet(transcript_fts, 0, ?1, ?2, '...', 16), bm25(transcript_fts)
         FROM transcript_fts JOIN transcript_segments s ON s.seq = transcript_fts.rowid
         JOIN audio_events e ON e.id = s.event_id
         WHERE transcript_fts MATCH ?3 AND (?4 OR e.source != 'mic')
         AND (?5 OR e.source != 'system') AND (?6 IS NULL OR e.started_at >= ?6)
         AND (?7 IS NULL OR e.started_at <= ?7)
         ORDER BY bm25(transcript_fts), e.started_at DESC, e.id DESC, s.start_ms LIMIT ?8"
    } else {
        "SELECT e.id, s.id, e.source, e.started_at, s.start_ms,
         snippet(transcript_fts, 0, ?1, ?2, '...', 16), 0.0
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
