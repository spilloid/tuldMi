//! The tools: definitions (name, schema, annotations), strict argument checking, and handlers.
//! Every handler re-checks the ceiling; `list` filtering is only discovery.

use crate::ceiling::{Ceiling, clip, parse_source, parse_time, source_name, time_text};
use crate::{McpServer, Snapshot};
use base64::Engine as _;
use serde_json::{Map, Value, json};
use tuldmi_engine::storage::{EventFilter, Store};
use tuldmi_engine::types::{HIGHLIGHT_END, HIGHLIGHT_START, SearchQuery, Source, SpeechEvent};
use uuid::Uuid;

/// One event's transcript is cut beyond this, in characters.
const MAX_EVENT_CHARS: usize = 20_000;
/// In listings (search, recent, conversation_at), each event's text is cut beyond this.
const MAX_LISTED_CHARS: usize = 2_000;
/// Audio larger than this is refused rather than sent.
const MAX_AUDIO_BYTES: u64 = 4 * 1024 * 1024;
/// Any string argument longer than this is refused.
const MAX_ARG_CHARS: usize = 1_000;
const MAX_RESULT_BYTES: usize = 512 * 1024;
/// The timeline's conversation grouping (src/lib/time.ts `CONVERSATION_GAP_MS`).
const CONVERSATION_GAP_MS: i64 = 3 * 60_000;
/// How far around a time `conversation_at` looks, and how many events a conversation returns.
const CONVERSATION_REACH_MS: i64 = 6 * 3_600_000;
const MAX_CONVERSATION_EVENTS: usize = 200;
/// How many events `conversation_at` reads on each side of the requested time.
const CONVERSATION_SIDE_LIMIT: u32 = 500;
/// The protocol version without audio content.
const NO_AUDIO_VERSION: &str = "2024-11-05";
/// Exit code of `tuldmi --pause` when no tuldMi was running (src-tauri `PAUSE_NOT_RUNNING`).
const PAUSE_NOT_RUNNING: i32 = 3;

const HOW_TO_ENABLE: &str = "The person can turn it on in tuldMi: Settings > Privacy & data > AI assistants (which explains what is shared).";

pub enum ToolError {
    Unknown,
    /// A tool-level failure the agent should read (`isError: true`), with the tool's own name for
    /// the log.
    Refused(&'static str, String),
}

pub struct Done {
    pub tool: &'static str,
    pub result: Value,
    pub count: usize,
}

type ToolResult = Result<Done, String>;

pub fn error_result(message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

/// A readable summary, the same data as JSON text (for clients that only read `content`), and
/// `structuredContent`.
fn ok(text: String, structured: Value, count: usize) -> ToolResult {
    let json_text = serde_json::to_string(&structured).unwrap_or_default();
    if text.len().saturating_add(json_text.len()) > MAX_RESULT_BYTES {
        return Err("Result too large; narrow the time window or reduce the limit.".into());
    }
    Ok(Done {
        tool: "",
        result: json!({
            "content": [{ "type": "text", "text": text }, { "type": "text", "text": json_text }],
            "structuredContent": structured,
            "isError": false,
        }),
        count,
    })
}

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Int { min: i64, max: i64 },
}

struct Param {
    name: &'static str,
    kind: Kind,
    required: bool,
    description: &'static str,
}

const fn p(name: &'static str, kind: Kind, required: bool, description: &'static str) -> Param {
    Param { name, kind, required, description }
}

struct Def {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    params: &'static [Param],
    read_only: bool,
}

const TIME: &str = "RFC 3339, 'YYYY-MM-DD HH:MM' or 'YYYY-MM-DD' (local time), or an age like 30m, 2h, 7d";
const SOURCE: &str = "'me' (the person's microphone) or 'remote' (what their computer played: the other side of calls); omit for both";

const DEFS: &[Def] = &[
    Def {
        name: "search",
        title: "Search transcripts",
        description: "Find what was said: full-text search of transcripts, newest and best matches first. Matches are marked **like this**.",
        params: &[
            p("query", Kind::Str, true, "Words to find"),
            p("source", Kind::Str, false, SOURCE),
            p("from", Kind::Str, false, TIME),
            p("to", Kind::Str, false, TIME),
            p("limit", Kind::Int { min: 1, max: 50 }, false, "At most this many matches (default 20)"),
        ],
        read_only: true,
    },
    Def {
        name: "recent",
        title: "Recent speech",
        description: "The latest speech events, newest first, or those that started before a time.",
        params: &[
            p("before", Kind::Str, false, TIME),
            p("source", Kind::Str, false, SOURCE),
            p("limit", Kind::Int { min: 1, max: 100 }, false, "At most this many events (default 20)"),
        ],
        read_only: true,
    },
    Def {
        name: "conversation_at",
        title: "Conversation at a time",
        description: "The conversation around a time: consecutive speech events less than 3 minutes apart, oldest first, as the tuldMi timeline groups them.",
        params: &[p("time", Kind::Str, true, TIME), p("source", Kind::Str, false, SOURCE)],
        read_only: true,
    },
    Def {
        name: "get_event",
        title: "Get a speech event",
        description: "One speech event's full transcript, with the time of each segment.",
        params: &[p("id", Kind::Str, true, "An event id from search, recent or conversation_at")],
        read_only: true,
    },
    Def {
        name: "get_audio",
        title: "Get an event's audio",
        description: "One speech event's recorded audio (Ogg Opus), when the person kept it and allows audio for assistants.",
        params: &[p("id", Kind::Str, true, "An event id from search, recent or conversation_at")],
        read_only: true,
    },
    Def {
        name: "get_status",
        title: "tuldMi status",
        description: "Whether assistant access is on, what it allows, and how much history is in reach.",
        params: &[],
        read_only: true,
    },
    Def {
        name: "pause_recording",
        title: "Pause recording",
        description: "Pause tuldMi's recording now (visible in its tray or menu bar icon). It cannot resume recording; only the person can.",
        params: &[],
        read_only: false,
    },
];

fn schema(def: &Def) -> Value {
    let mut properties = Map::new();
    for param in def.params {
        let mut s = match param.kind {
            Kind::Str => json!({ "type": "string" }),
            Kind::Int { min, max } => json!({ "type": "integer", "minimum": min, "maximum": max }),
        };
        s["description"] = json!(param.description);
        properties.insert(param.name.into(), s);
    }
    let required: Vec<&str> = def.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    let mut schema = json!({ "type": "object", "properties": properties, "additionalProperties": false });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

/// The top-level typed result contract.
fn output_schema(name: &str) -> Option<Value> {
    let fields: &[(&str, Value)] = match name {
        "search" => &[("matches", json!({"type":"array", "maxItems":50, "items":{"type":"object"}}))],
        "recent" => &[("events", json!({"type":"array", "maxItems":100, "items":{"type":"object"}}))],
        "conversation_at" => &[
            ("conversation", json!({"type":["object","null"]})),
            ("truncated", json!({"type":"boolean"})),
        ],
        "get_event" => &[("event", json!({"type":"object"}))],
        "get_status" => &[
            ("access", json!({"type":"object"})),
            ("history", json!({"type":["object","null"]})),
        ],
        "pause_recording" => &[
            ("paused", json!({"type":"boolean"})),
            ("running", json!({"type":"boolean"})),
        ],
        _ => return None,
    };
    let properties: Map<String, Value> = fields.iter().map(|(key, value)| (key.to_string(), value.clone())).collect();
    let required: Vec<&str> = fields.iter().map(|(key, _)| *key).collect();
    Some(json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false}))
}

/// Rejects unknown, missing, mistyped and out-of-range arguments: an argument the tool cannot use
/// exactly as given is an error, never a silent default.
fn validate(def: &Def, args: &Value) -> Result<(), String> {
    let empty = Map::new();
    let given = args.as_object().unwrap_or(&empty);
    for (key, value) in given {
        let Some(param) = def.params.iter().find(|p| p.name == key) else {
            return Err(format!("{} has no argument '{}'", def.name, clip(key, 40)));
        };
        let fine = match param.kind {
            Kind::Str => value.as_str().is_some_and(|s| s.chars().count() <= MAX_ARG_CHARS),
            Kind::Int { min, max } => value.as_i64().is_some_and(|n| (min..=max).contains(&n)),
        };
        if !fine {
            let expected = match param.kind {
                Kind::Str => format!("a string of at most {MAX_ARG_CHARS} characters"),
                Kind::Int { min, max } => format!("an integer from {min} to {max}"),
            };
            return Err(format!("{}: '{}' must be {expected}", def.name, param.name));
        }
    }
    for param in def.params.iter().filter(|p| p.required) {
        if !given.contains_key(param.name) {
            return Err(format!("{} needs '{}'", def.name, param.name));
        }
    }
    Ok(())
}

/// Every tool name, whatever the ceiling.
pub fn all_names() -> Vec<&'static str> {
    DEFS.iter().map(|d| d.name).collect()
}

/// Whether the ceiling (and the negotiated protocol) let this tool be listed and called; `Err`
/// says why not.
fn allowed(ceiling: &Ceiling, version: Option<&str>, name: &str) -> Result<(), String> {
    if name == "get_status" {
        return Ok(());
    }
    if !ceiling.enabled {
        return Err(format!("tuldMi's assistant access is turned off. {HOW_TO_ENABLE}"));
    }
    if name == "get_audio" && !ceiling.allow_audio {
        return Err("Audio is not shared with assistants. The person can allow it in tuldMi: Settings > Privacy & data > AI assistants.".into());
    }
    if name == "get_audio" && version == Some(NO_AUDIO_VERSION) {
        return Err(format!("Audio content needs MCP protocol 2025-03-26 or later; this connection uses {NO_AUDIO_VERSION}."));
    }
    Ok(())
}

/// The advertised tools for this ceiling.
pub fn list(ceiling: &Ceiling, version: Option<&str>) -> Vec<Value> {
    DEFS.iter()
        .filter(|d| allowed(ceiling, version, d.name).is_ok())
        .map(|d| {
            let mut tool = json!({
                "name": d.name,
                "title": d.title,
                "description": d.description,
                "inputSchema": schema(d),
                "annotations": {
                    "title": d.title,
                    "readOnlyHint": d.read_only,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false,
                },
            });
            if let Some(schema) = output_schema(d.name) {
                tool["outputSchema"] = schema;
            }
            tool
        })
        .collect()
}

pub fn call(server: &McpServer, snapshot: &Snapshot, version: Option<&str>, name: &str, args: &Value) -> Result<Done, ToolError> {
    let def = DEFS.iter().find(|d| d.name == name).ok_or(ToolError::Unknown)?;
    let tool = def.name;
    let refuse = |message: String| ToolError::Refused(tool, message);
    let ceiling = &snapshot.ceiling;
    allowed(ceiling, version, tool).map_err(refuse)?;
    validate(def, args).map_err(refuse)?;
    if tool == "get_status" {
        return get_status(snapshot).map(|done| Done { tool, ..done }).map_err(refuse);
    }
    if tool == "pause_recording" {
        return pause(server).map(|done| Done { tool, ..done }).map_err(refuse);
    }
    if snapshot.store.is_none() {
        return Err(refuse(snapshot.problem.clone().unwrap_or_else(|| "tuldMi's history could not be read".into())));
    }
    let store = snapshot.store.as_ref().ok_or_else(|| refuse(snapshot.problem.clone().unwrap_or_else(|| "tuldMi's history could not be read".into())))?;
    let now = chrono::Local::now();
    let result = match tool {
        "search" => search(store, ceiling, args, now),
        "recent" => recent(store, ceiling, args, now),
        "conversation_at" => conversation_at(store, ceiling, args, now),
        "get_event" => get_event(store, ceiling, args),
        "get_audio" => get_audio(store, ceiling, args),
        _ => return Err(ToolError::Unknown),
    };
    result.map(|done| Done { tool, ..done }).map_err(refuse)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn int_arg(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn time_arg(args: &Value, key: &str, now: chrono::DateTime<chrono::Local>) -> Result<Option<i64>, String> {
    str_arg(args, key).map(|t| parse_time(t, now)).transpose()
}

/// The sources to read: the one asked for (if the ceiling admits it), else all admitted ones.
fn sources(ceiling: &Ceiling, args: &Value) -> Result<Vec<Source>, String> {
    match str_arg(args, "source") {
        None => Ok(ceiling.sources()),
        Some(text) => match parse_source(text) {
            Some(Source::System) if !ceiling.include_remote => Err(
                "The other side of calls ('remote') is not shared with assistants. The person can include it in tuldMi: Settings > Privacy & data > AI assistants.".into(),
            ),
            Some(source) => Ok(vec![source]),
            None => Err("source must be 'me' or 'remote'".into()),
        },
    }
}

fn transcript(event: &SpeechEvent) -> String {
    let parts: Vec<&str> = event.segments.iter().map(|s| s.text.trim()).filter(|t| !t.is_empty()).collect();
    parts.join(" ")
}

/// Cuts `text` to `max` characters; says whether it did.
fn cut(text: String, max: usize) -> (String, bool) {
    if text.chars().count() > max { (text.chars().take(max).collect(), true) } else { (text, false) }
}

fn event_json(event: &SpeechEvent, max_chars: usize) -> Value {
    let (text, truncated) = cut(transcript(event), max_chars);
    json!({
        "id": event.id.to_string(),
        "source": source_name(event.source),
        "started": time_text(event.started_at),
        "ended": time_text(event.ended_at),
        "seconds": (event.ended_at - event.started_at).max(0) as f64 / 1000.0,
        "status": event.status.as_str(),
        "language": event.language,
        "text": text,
        "truncated": truncated,
        "has_audio": event.audio.is_some(),
    })
}

fn event_line(event: &SpeechEvent) -> String {
    let (text, truncated) = cut(transcript(event), 200);
    let text = if text.is_empty() { format!("({})", event.status.as_str()) } else { text };
    format!("- {} [{}] {}{}", time_text(event.started_at), source_name(event.source), text, if truncated { "…" } else { "" })
}

fn search(store: &Store, ceiling: &Ceiling, args: &Value, now: chrono::DateTime<chrono::Local>) -> ToolResult {
    let query = str_arg(args, "query").unwrap_or_default();
    if query.trim().is_empty() {
        return Err("query must contain words to find".into());
    }
    let sources = sources(ceiling, args)?;
    let from = ceiling.clamp_from(time_arg(args, "from", now)?);
    let to = time_arg(args, "to", now)?;
    // Newest first, not by relevance: bm25's statistics span hidden transcripts too.
    let hits = store
        .search_by_time(&SearchQuery { text: query.into(), sources, from, to, limit: Some(int_arg(args, "limit", 20) as u32) })
        .map_err(|_| "search failed".to_string())?;
    let matches: Vec<Value> = hits
        .iter()
        .filter(|hit| ceiling.admits(hit.source, hit.at))
        .map(|hit| {
            let snippet = hit.snippet.replace([HIGHLIGHT_START, HIGHLIGHT_END], "**");
            json!({ "event": hit.event_id.to_string(), "source": source_name(hit.source), "at": time_text(hit.at), "snippet": snippet })
        })
        .collect();
    let mut text = format!("{} match(es).", matches.len());
    for m in &matches {
        text.push_str(&format!("\n- {} [{}] {} (event {})", m["at"].as_str().unwrap_or(""), m["source"].as_str().unwrap_or(""), m["snippet"].as_str().unwrap_or(""), m["event"].as_str().unwrap_or("")));
    }
    let count = matches.len();
    ok(text, json!({ "matches": matches }), count)
}

fn recent(store: &Store, ceiling: &Ceiling, args: &Value, now: chrono::DateTime<chrono::Local>) -> ToolResult {
    let filter = EventFilter {
        from: ceiling.clamp_from(None),
        to: time_arg(args, "before", now)?,
        sources: sources(ceiling, args)?,
        newest_first: true,
        limit: int_arg(args, "limit", 20) as u32,
    };
    let events: Vec<SpeechEvent> = store
        .events_matching(&filter)
        .map_err(|_| "could not read the history".to_string())?
        .into_iter()
        .filter(|e| ceiling.admits(e.source, e.started_at))
        .collect();
    let mut text = format!("{} event(s), newest first.", events.len());
    for e in &events {
        text.push('\n');
        text.push_str(&event_line(e));
    }
    let json_events: Vec<Value> = events.iter().map(|e| event_json(e, MAX_LISTED_CHARS)).collect();
    ok(text, json!({ "events": json_events }), events.len())
}

/// Splits time-ordered events into the timeline's conversations.
fn conversations(events: Vec<SpeechEvent>) -> Vec<Vec<SpeechEvent>> {
    let mut groups: Vec<Vec<SpeechEvent>> = Vec::new();
    for event in events {
        match groups.last_mut() {
            Some(group) if event.started_at - group.iter().map(|e| e.ended_at).max().unwrap_or(i64::MIN) < CONVERSATION_GAP_MS => group.push(event),
            _ => groups.push(vec![event]),
        }
    }
    groups
}

fn conversation_at(store: &Store, ceiling: &Ceiling, args: &Value, now: chrono::DateTime<chrono::Local>) -> ToolResult {
    let at = time_arg(args, "time", now)?.unwrap_or_default();
    let sources = sources(ceiling, args)?;
    let read = |filter: EventFilter| store.events_matching(&filter).map_err(|_| "could not read the history".to_string());
    // Read outward from the requested time, so busy hours before it can't crowd it out.
    let mut before = read(EventFilter {
        from: ceiling.clamp_from(Some(at - CONVERSATION_REACH_MS)),
        to: Some(at),
        sources: sources.clone(),
        newest_first: true,
        limit: CONVERSATION_SIDE_LIMIT,
    })?;
    let after = read(EventFilter {
        from: ceiling.clamp_from(Some(at)),
        to: Some(at + CONVERSATION_REACH_MS),
        sources,
        newest_first: false,
        limit: CONVERSATION_SIDE_LIMIT,
    })?;
    // A full side may have more events beyond what was read.
    let earliest_read = (before.len() as u32 == CONVERSATION_SIDE_LIMIT).then(|| before.last().map(|e| e.id)).flatten();
    let latest_read = (after.len() as u32 == CONVERSATION_SIDE_LIMIT).then(|| after.last().map(|e| e.id)).flatten();
    before.reverse();
    let events: Vec<SpeechEvent> = before.into_iter().chain(after).filter(|e| ceiling.admits(e.source, e.started_at)).collect();
    let found = conversations(events).into_iter().find(|group| {
        let start = group.first().map_or(i64::MAX, |e| e.started_at);
        let end = group.iter().map(|e| e.ended_at).max().unwrap_or(i64::MIN);
        at >= start - CONVERSATION_GAP_MS && at <= end + CONVERSATION_GAP_MS
    });
    let Some(group) = found else {
        return ok(format!("No conversation within 3 minutes of {}.", time_text(at)), json!({ "conversation": null, "truncated": false }), 0);
    };
    let cut_short = group.iter().any(|e| Some(e.id) == earliest_read || Some(e.id) == latest_read);
    let truncated = cut_short || group.len() > MAX_CONVERSATION_EVENTS;
    let shown = &group[..group.len().min(MAX_CONVERSATION_EVENTS)];
    let started = shown.first().map(|e| e.started_at).unwrap_or(at);
    let ended = group.iter().map(|e| e.ended_at).max().unwrap_or(at);
    let mut text = format!("Conversation {} to {}, {} event(s), oldest first.", time_text(started), time_text(ended), group.len());
    for e in shown {
        text.push('\n');
        text.push_str(&event_line(e));
    }
    let json_events: Vec<Value> = shown.iter().map(|e| event_json(e, MAX_LISTED_CHARS)).collect();
    ok(
        text,
        json!({ "conversation": { "started": time_text(started), "ended": time_text(ended), "events": json_events }, "truncated": truncated }),
        shown.len(),
    )
}

/// The event if it exists and the ceiling admits it; the same answer otherwise, so a hidden
/// event's existence isn't revealed.
fn shared_event(store: &Store, ceiling: &Ceiling, args: &Value) -> Result<SpeechEvent, String> {
    let not_found = || "No shared event has that id.".to_string();
    let id = Uuid::parse_str(str_arg(args, "id").unwrap_or_default().trim()).map_err(|_| not_found())?;
    // The ceiling is part of the query, so a hidden event is never even loaded.
    store
        .get_event_matching(id, ceiling.oldest, &ceiling.sources())
        .map_err(|_| "could not read the history".to_string())?
        .filter(|e| ceiling.admits(e.source, e.started_at))
        .ok_or_else(not_found)
}

fn get_event(store: &Store, ceiling: &Ceiling, args: &Value) -> ToolResult {
    let event = shared_event(store, ceiling, args)?;
    let mut budget = MAX_EVENT_CHARS;
    let mut truncated = false;
    let mut segments = Vec::new();
    for s in &event.segments {
        if budget == 0 {
            truncated = true;
            break;
        }
        let (text, cut_here) = cut(s.text.trim().to_string(), budget);
        budget -= text.chars().count();
        truncated |= cut_here;
        segments.push(json!({ "at": time_text(event.started_at + s.start_ms), "seconds_into_event": s.start_ms as f64 / 1000.0, "text": text }));
    }
    let mut value = event_json(&event, MAX_EVENT_CHARS);
    value["segments"] = json!(segments);
    value["truncated"] = json!(truncated || value["truncated"].as_bool().unwrap_or(false));
    let mut text = format!("{} [{}], {:.0} s, {}.", time_text(event.started_at), source_name(event.source), value["seconds"].as_f64().unwrap_or(0.0), event.status.as_str());
    for s in &segments {
        text.push_str(&format!("\n{} {}", s["at"].as_str().unwrap_or(""), s["text"].as_str().unwrap_or("")));
    }
    ok(text, json!({ "event": value }), 1)
}

fn get_audio(store: &Store, ceiling: &Ceiling, args: &Value) -> ToolResult {
    let event = shared_event(store, ceiling, args)?;
    let audio = event.audio.as_ref().ok_or("This event's audio wasn't kept (or has expired under the person's retention setting).")?;
    let path = std::path::Path::new(&audio.path);
    let size = std::fs::metadata(path).map_err(|_| "The audio file is missing.".to_string())?.len();
    if size > MAX_AUDIO_BYTES {
        return Err(format!("The audio is larger than {} MiB, so it isn't sent.", MAX_AUDIO_BYTES / 1024 / 1024));
    }
    let bytes = std::fs::read(path).map_err(|_| "The audio file could not be read.".to_string())?;
    Ok(Done {
        tool: "",
        result: json!({
            "content": [
                { "type": "text", "text": format!("Audio of the {} event at {} ({:.0} s, Ogg Opus).", source_name(event.source), time_text(event.started_at), audio.duration_ms as f64 / 1000.0) },
                { "type": "audio", "data": base64::engine::general_purpose::STANDARD.encode(&bytes), "mimeType": "audio/ogg" },
            ],
            "isError": false,
        }),
        count: 1,
    })
}

fn get_status(snapshot: &Snapshot) -> ToolResult {
    let c = &snapshot.ceiling;
    let access = json!({
        "enabled": c.enabled,
        "sources": if c.include_remote { json!(["me", "remote"]) } else { json!(["me"]) },
        "audio": c.allow_audio,
        "oldest": c.oldest.map(time_text),
    });
    let history = match (&snapshot.store, c.enabled) {
        (Some(store), true) => store.event_summary(c.oldest, &c.sources()).ok().map(|(events, newest)| json!({ "events": events, "newest": newest.map(time_text) })),
        _ => None,
    };
    let text = if !c.enabled {
        match &snapshot.problem {
            Some(problem) => format!("Assistant access is off: {problem}. {HOW_TO_ENABLE}"),
            None => format!("Assistant access is off. {HOW_TO_ENABLE}"),
        }
    } else {
        let who = if c.include_remote { "both sides (me and remote)" } else { "the person's own microphone only (me); the other side of calls is not shared" };
        let age = c.oldest.map_or("all of it".to_string(), |o| format!("since {}", time_text(o)));
        let count = history.as_ref().and_then(|h| h["events"].as_u64()).unwrap_or(0);
        format!("Assistant access is on: {who}, {age}, audio {}. {count} event(s) in reach.", if c.allow_audio { "allowed" } else { "not shared" })
    };
    ok(text, json!({ "access": access, "history": history }), 1)
}

fn pause(server: &McpServer) -> ToolResult {
    let app = server.app_exe.as_ref().ok_or("tuldMi's app wasn't found next to tuldmi-mcp, so it can't be paused from here.")?;
    let token = Uuid::new_v4();
    let ack = server.data_dir.join("mcp").join(token.to_string());
    let mut child = std::process::Command::new(app)
        .arg("--pause")
        .arg("--pause-ack")
        .arg(token.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "tuldMi could not be asked to pause.".to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return Err("tuldMi didn't answer the pause request.".into());
            }
        }
    };
    match status.code() {
        Some(PAUSE_NOT_RUNNING) => return ok("tuldMi isn't running, so nothing is being recorded.".into(), json!({ "paused": false, "running": false }), 0),
        Some(0) => {}
        _ => return Err("tuldMi could not be asked to pause.".into()),
    }
    // Exit 0 only means the request reached the running app; its answer is the acknowledgement.
    let outcome = loop {
        match std::fs::read_to_string(&ack) {
            Ok(outcome) => {
                let _ = std::fs::remove_file(&ack);
                break outcome;
            }
            Err(_) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return Err("tuldMi didn't confirm the pause. Ask the person to check its tray or menu bar icon.".into()),
        }
    };
    match outcome.trim() {
        "paused" => ok("Paused. Recording stays paused until the person resumes it.".into(), json!({ "paused": true, "running": true }), 1),
        "not-recording" => ok("tuldMi wasn't recording (already paused or stopped), so nothing changed.".into(), json!({ "paused": false, "running": true }), 0),
        "not-set-up" => ok("tuldMi hasn't been set up yet, so nothing is being recorded.".into(), json!({ "paused": false, "running": true }), 0),
        "starting" => Err("tuldMi is still starting; try again in a moment.".into()),
        _ => Err("tuldMi could not pause. Ask the person to pause it from its tray or menu bar icon.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_annotations_and_object_schemas() {
        let all = Ceiling { enabled: true, include_remote: true, allow_audio: true, oldest: None };
        let tools = list(&all, None);
        assert!(list(&all, Some(NO_AUDIO_VERSION)).iter().all(|t| t["name"] != "get_audio"), "no audio content in 2024-11-05");
        assert_eq!(tools.len(), DEFS.len());
        for tool in &tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert_eq!(tool["annotations"]["destructiveHint"], false);
            assert_eq!(tool["annotations"]["openWorldHint"], false);
        }
        let pause = tools.iter().find(|t| t["name"] == "pause_recording").unwrap();
        assert_eq!(pause["annotations"]["readOnlyHint"], false);
    }

    #[test]
    fn the_ceiling_decides_what_is_offered() {
        let names = |c: &Ceiling| list(c, None).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(&Ceiling::off()), vec!["get_status"]);
        let text_only = Ceiling { enabled: true, include_remote: false, allow_audio: false, oldest: None };
        assert!(!names(&text_only).contains(&"get_audio".to_string()));
        assert!(names(&text_only).contains(&"search".to_string()));
    }

    #[test]
    fn arguments_are_checked_exactly() {
        let search = DEFS.iter().find(|d| d.name == "search").unwrap();
        assert!(validate(search, &json!({"query": "deck"})).is_ok());
        assert!(validate(search, &json!({})).is_err());
        assert!(validate(search, &json!({"query": "deck", "limit": 51})).is_err());
        assert!(validate(search, &json!({"query": "deck", "extra": 1})).is_err());
        assert!(validate(search, &json!({"query": "x".repeat(MAX_ARG_CHARS + 1)})).is_err());
        let remote_off = Ceiling { enabled: true, include_remote: false, allow_audio: false, oldest: None };
        assert!(sources(&remote_off, &json!({"source": "remote"})).is_err());
        assert_eq!(sources(&remote_off, &json!({})), Ok(vec![Source::Mic]));
    }
}
