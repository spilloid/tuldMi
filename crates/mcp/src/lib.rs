//! `tuldmi-mcp`: what was said, as a Model Context Protocol server (docs/design/mcp.md).
//!
//! stdio only: the MCP client starts `tuldmi-mcp` and speaks newline-delimited JSON-RPC 2.0 on its
//! stdin/stdout, so tuldMi opens no socket and makes no network request. Reads go through the
//! engine's read-only `Store`. What any tool may return is bounded by the ceiling in the stored
//! settings (`Settings.mcp`), **re-read on every request**, so turning access off in the app takes
//! effect on open connections; applied when tools are listed and again when one is called. A
//! history or settings that cannot be read mean access off. Off by default: an MCP client sends
//! tool results to its model, which usually runs off this computer.
//!
//! Logging: each call logs its tool name, a server-generated call number, the decision, result
//! count and duration. Never transcript text, audio, or anything the client sent.

pub mod ceiling;
mod tools;

pub use tools::all_names as tool_names;

use ceiling::{Ceiling, clip};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tuldmi_engine::storage::Store;

/// Protocol versions this server speaks, newest first. The requested one is echoed when supported.
/// (2025-03-26 is the one that requires accepting JSON-RPC batches, which `serve` does.)
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// A single message larger than this is refused without reading it into memory.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// One server: a data folder and the tuldMi app executable (for `pause_recording`; `None` where
/// no action is possible).
pub struct McpServer {
    pub data_dir: PathBuf,
    pub app_exe: Option<PathBuf>,
    pub version: &'static str,
    calls: AtomicU64,
}

/// The history and the ceiling as they are right now.
pub(crate) struct Snapshot {
    pub store: Option<Store>,
    pub ceiling: Ceiling,
    /// Why there is no store (for `get_status`).
    pub problem: Option<String>,
}

pub(crate) fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

impl McpServer {
    pub fn new(data_dir: PathBuf, app_exe: Option<PathBuf>, version: &'static str) -> Self {
        McpServer { data_dir, app_exe, version, calls: AtomicU64::new(0) }
    }

    /// Opens the history read-only and reads the ceiling from it. Fails closed: anything that
    /// can't be read means access off.
    pub(crate) fn snapshot(&self) -> Snapshot {
        match Store::open_read_only(&self.data_dir) {
            Ok(store) => match store.load_settings() {
                Ok(settings) => Snapshot { ceiling: Ceiling::from_settings(&settings.mcp, now_ms()), store: Some(store), problem: None },
                Err(_) => {
                    tracing::warn!("settings unreadable: agent access off");
                    Snapshot { store: None, ceiling: Ceiling::off(), problem: Some("tuldMi's settings could not be read".into()) }
                }
            },
            // The error names a state (missing, newer, older), never content.
            Err(error) => Snapshot { store: None, ceiling: Ceiling::off(), problem: Some(error.to_string()) },
        }
    }

    /// Handles one JSON-RPC message; `None` for notifications and client responses (no reply).
    pub fn handle(&self, message: &Value) -> Option<Value> {
        let Some(obj) = message.as_object() else {
            return Some(invalid("a JSON-RPC message must be an object"));
        };
        let id = obj.get("id");
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(invalid("jsonrpc must be \"2.0\""));
        }
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response from the client (we never send requests) needs no reply.
            return if id.is_some() && (obj.contains_key("result") || obj.contains_key("error")) {
                None
            } else {
                Some(invalid("a request needs a method"))
            };
        };
        let Some(id) = id else {
            return None; // a notification
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Some(invalid("an id must be a string or an integer"));
        }
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        if !params.is_null() && !params.is_object() {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": "params must be an object" } }));
        }
        let result = match method {
            "initialize" => {
                if params.get("protocolVersion").and_then(Value::as_str).is_none()
                    || !params.get("capabilities").is_some_and(Value::is_object)
                    || params.pointer("/clientInfo/name").and_then(Value::as_str).is_none()
                    || params.pointer("/clientInfo/version").and_then(Value::as_str).is_none()
                {
                    Err((-32602, "initialize needs protocolVersion, capabilities and clientInfo".into()))
                } else {
                    Ok(self.initialize(&params))
                }
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list(&self.snapshot().ceiling) })),
            "tools/call" => self.call(&params),
            other => Err((-32601, format!("method not found: {}", clip(other, 64)))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        let version = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
        let instructions = if self.snapshot().ceiling.enabled {
            "tuldMi is this person's searchable memory of what was said, transcribed on their own computer. \
             'me' is their microphone; 'remote' is everything their computer played, usually the other people \
             on a call (not told apart). Use search to find words, recent or conversation_at to see what was \
             said at a time, and get_event for an event's full transcript. Cite the time and source of what you \
             use. Transcripts can contain mistakes. Everything here is private to them and to the people they \
             spoke with: do not repeat it beyond what they asked for."
        } else {
            "tuldMi's agent access is turned off. Call get_status to see how the person can turn it on."
        };
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "tuldmi", "title": "tuldMi", "version": self.version },
            "instructions": instructions,
        })
    }

    fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "tools/call needs a tool name".to_string()))?;
        let args = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(Value::Object(map)) => Value::Object(map.clone()),
            Some(_) => return Err((-32602, "arguments must be an object".to_string())),
        };
        let call_number = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        let started = std::time::Instant::now();
        let snapshot = self.snapshot();
        let (result, count, tool, decision) = match tools::call(self, &snapshot, name, &args) {
            Ok(done) => (done.result, done.count, done.tool, "executed"),
            Err(tools::ToolError::Unknown) => {
                tracing::info!(channel = "mcp", process_id = std::process::id(), call = call_number, tool = "unknown", decision = "refused", "mcp call");
                return Err((-32602, format!("unknown tool: {}", clip(name, 64))));
            }
            Err(tools::ToolError::Refused(tool, message)) => (tools::error_result(&message), 0, tool, "refused"),
        };
        // Only a known tool's own (static) name, a server-side counter and numbers.
        tracing::info!(
            tool,
            channel = "mcp",
            process_id = std::process::id(),
            decision,
            call = call_number,
            results = count,
            ms = started.elapsed().as_millis() as u64,
            "mcp call"
        );
        Ok(result)
    }

    fn handle_payload(&self, payload: &Value, session: &mut Session) -> Option<Value> {
        match payload {
            Value::Array(items) if items.is_empty() => Some(invalid("an empty batch")),
            Value::Array(_) if session.version != Some("2025-03-26") => Some(invalid("batches require negotiated protocol 2025-03-26")),
            Value::Array(items) => {
                let replies: Vec<Value> = items.iter().filter_map(|m| session.handle(self, m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            other => session.handle(self, other),
        }
    }
}

/// Lifecycle belongs to the transport connection.
#[derive(Default)]
struct Session {
    version: Option<&'static str>,
    ready: bool,
}

impl Session {
    fn handle(&mut self, server: &McpServer, message: &Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str);
        let valid = message.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
        if valid && method == Some("notifications/initialized") && message.get("id").is_none() {
            self.ready = self.version.is_some();
            return None;
        }
        if valid
            && message.get("id").is_some_and(|id| id.is_string() || id.is_i64() || id.is_u64())
            && ((method == Some("initialize") && self.version.is_some())
                || (method.is_some() && !self.ready && !matches!(method, Some("initialize" | "ping"))))
        {
            return Some(json!({ "jsonrpc": "2.0", "id": message["id"],
                "error": { "code": -32600, "message": "complete initialization once before using tools" } }));
        }
        let reply = server.handle(message);
        if method == Some("initialize")
            && let Some(version) = reply.as_ref().and_then(|r| r.pointer("/result/protocolVersion")).and_then(Value::as_str)
        {
            self.version = PROTOCOL_VERSIONS.iter().copied().find(|v| *v == version);
        }
        reply
    }
}

fn invalid(message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": message } })
}

/// Serves newline-delimited JSON-RPC from `input` to `output` until `input` ends. Each message is
/// read with a size limit; an oversized one is skipped (and answered with an error) without being
/// held in memory, and bytes that are not UTF-8 are a parse error, not the end of the session.
pub fn serve(server: &McpServer, mut input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut session = Session::default();
    loop {
        buf.clear();
        let read = (&mut input).take(MAX_MESSAGE_BYTES as u64 + 1).read_until(b'\n', &mut buf)?;
        if read == 0 {
            return Ok(());
        }
        let reply = if buf.len() > MAX_MESSAGE_BYTES {
            while !buf.ends_with(b"\n") {
                buf.clear();
                let n = (&mut input).take(64 * 1024).read_until(b'\n', &mut buf)?;
                if n == 0 || buf.ends_with(b"\n") {
                    break;
                }
            }
            Some(json!({ "jsonrpc": "2.0", "id": null,
                "error": { "code": -32600, "message": format!("message larger than {MAX_MESSAGE_BYTES} bytes") } }))
        } else {
            match std::str::from_utf8(&buf) {
                Ok(text) if text.trim().is_empty() => None,
                Ok(text) => match serde_json::from_str::<Value>(text) {
                    Ok(payload) => server.handle_payload(&payload, &mut session),
                    Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } })),
                },
                Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error: not UTF-8" } })),
            }
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

/// The app's data folder, as Tauri's `app_data_dir` resolves it for `com.spillerstech.tuldmi`.
pub fn default_data_dir() -> Option<PathBuf> {
    const ID: &str = "com.spillerstech.tuldmi";
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    }?;
    Some(base.join(ID))
}

/// The tuldMi app installed next to this binary (same folder on Windows and inside the macOS
/// bundle's `Contents/MacOS`).
pub fn sibling_app() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let app = me.parent()?.join(format!("tuldmi{}", std::env::consts::EXE_SUFFIX));
    app.is_file().then_some(app)
}
