//! The contract over a real transport: the built `tuldmi-mcp` binary on a synthetic history,
//! spoken to over its stdin/stdout like an MCP client would.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tuldmi_engine::storage::{NewAudio, NewEvent, NewSegment, Store};
use tuldmi_engine::types::{McpSettings, Source};
use uuid::Uuid;

const DAY: i64 = 86_400_000;

struct History {
    dir: tempfile::TempDir,
    me: Uuid,
    remote: Uuid,
    old: Uuid,
    audio: Vec<u8>,
}

fn now() -> i64 {
    chrono::Local::now().timestamp_millis()
}

fn history() -> History {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let t = now() - 3_600_000;
    let session = store.start_session(t - 60_000).unwrap();
    let audio = b"OggS synthetic audio".to_vec();
    let clip = dir.path().join("audio").join("remote.ogg");
    std::fs::create_dir_all(clip.parent().unwrap()).unwrap();
    std::fs::write(&clip, &audio).unwrap();
    let add = |source, at: i64, text: &str, audio: Option<NewAudio>| {
        let event = store.insert_event(&NewEvent { session_id: session.id, source, started_at: at, ended_at: at + 5_000, audio }).unwrap();
        store.save_transcript(event.id, &[NewSegment { start_ms: 0, end_ms: 5_000, text: text.into(), confidence: None }], Some("en"), "tiny").unwrap();
        event.id
    };
    let me = add(Source::Mic, t, "I will send the deck tonight", None);
    let remote = add(
        Source::System,
        t + 10_000,
        "great the deck is what the board wants",
        Some(NewAudio { path: clip, sample_rate: 16_000, duration_ms: 5_000, bytes: audio.len() as i64, peaks: vec![1] }),
    );
    let old = add(Source::Mic, now() - 40 * DAY, "an old deck from last month", None);
    History { dir, me, remote, old, audio }
}

fn set(dir: &Path, mcp: McpSettings) {
    let store = Store::open(dir).unwrap();
    let mut settings = store.load_settings().unwrap();
    settings.mcp = mcp;
    store.save_settings(&settings).unwrap();
}

fn on(include_remote: bool, allow_audio: bool) -> McpSettings {
    McpSettings { enabled: true, include_remote, allow_audio, max_age_days: 30 }
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start(dir: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tuldmi-mcp"))
            .arg("--data-dir")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client { child, stdin, stdout, next: 1 }
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], id);
        reply
    }

    fn ready(dir: &Path) -> Client {
        Client::ready_with(dir, "2025-06-18")
    }

    fn ready_with(dir: &Path, version: &str) -> Client {
        let mut client = Client::start(dir);
        let init = client.request("initialize", json!({ "protocolVersion": version, "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }));
        assert_eq!(init["result"]["protocolVersion"], version);
        assert_eq!(init["result"]["serverInfo"]["name"], "tuldmi");
        client.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        client
    }

    fn tools(&mut self) -> Vec<String> {
        let reply = self.request("tools/list", json!({}));
        reply["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({ "name": name, "arguments": arguments }))["result"].clone()
    }

    /// Closes stdin and returns what the server logged.
    fn finish(self) -> String {
        let Client { child, stdin, .. } = self;
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stderr).into_owned()
    }
}

fn ids(result: &Value, list: &str, key: &str) -> Vec<String> {
    result["structuredContent"][list].as_array().unwrap().iter().map(|e| e[key].as_str().unwrap().to_string()).collect()
}

#[test]
fn off_by_default_offers_only_status() {
    let h = history();
    let mut c = Client::ready(h.dir.path());
    assert_eq!(c.tools(), vec!["get_status"]);
    let refused = c.call("search", json!({ "query": "deck" }));
    assert_eq!(refused["isError"], true, "a hidden tool called directly is refused");
    assert!(refused["content"][0]["text"].as_str().unwrap().contains("turned off"));
    let status = c.call("get_status", json!({}));
    assert_eq!(status["structuredContent"]["access"]["enabled"], false);
    assert!(status["structuredContent"]["history"].is_null(), "no counts while off");
    c.finish();
}

#[test]
fn tools_wait_for_initialization() {
    let h = history();
    let mut c = Client::start(h.dir.path());
    let early = c.request("tools/list", json!({}));
    assert_eq!(early["error"]["code"], -32600);
    c.finish();
}

#[test]
fn remote_stays_hidden_until_included() {
    let h = history();
    set(h.dir.path(), on(false, false));
    let mut c = Client::ready(h.dir.path());
    let tools = c.tools();
    assert!(tools.contains(&"search".to_string()) && !tools.contains(&"get_audio".to_string()));

    let found = c.call("search", json!({ "query": "deck" }));
    assert_eq!(ids(&found, "matches", "event"), vec![h.me.to_string()], "no Remote, nothing older than 30 days");
    let recent = c.call("recent", json!({}));
    assert_eq!(ids(&recent, "events", "id"), vec![h.me.to_string()]);
    assert_eq!(c.call("get_event", json!({ "id": h.remote.to_string() }))["isError"], true);
    assert_eq!(c.call("get_event", json!({ "id": h.old.to_string() }))["isError"], true);
    assert_eq!(c.call("search", json!({ "query": "deck", "source": "remote" }))["isError"], true);
    assert_eq!(c.call("get_audio", json!({ "id": h.remote.to_string() }))["isError"], true);
    let status = c.call("get_status", json!({}));
    assert_eq!(status["structuredContent"]["history"]["events"], 1);

    let event = c.call("get_event", json!({ "id": h.me.to_string() }));
    assert_eq!(event["structuredContent"]["event"]["source"], "me");
    assert_eq!(event["structuredContent"]["event"]["text"], "I will send the deck tonight");
    c.finish();
}

#[test]
fn both_sides_and_audio_when_allowed() {
    let h = history();
    set(h.dir.path(), on(true, true));
    let mut c = Client::ready(h.dir.path());
    let mut found = ids(&c.call("search", json!({ "query": "deck" })), "matches", "event");
    found.sort();
    let mut both = vec![h.me.to_string(), h.remote.to_string()];
    both.sort();
    assert_eq!(found, both);
    let conversation = c.call("conversation_at", json!({ "time": "50m" }));
    assert!(conversation["structuredContent"]["conversation"].is_null(), "nothing within 3 minutes of 50 minutes ago");
    let at = c.call("get_event", json!({ "id": h.me.to_string() }))["structuredContent"]["event"]["started"].as_str().unwrap().to_string();
    let conversation = c.call("conversation_at", json!({ "time": at }));
    let events: Vec<String> = conversation["structuredContent"]["conversation"]["events"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(events, vec![h.me.to_string(), h.remote.to_string()], "one conversation, oldest first");

    let audio = c.call("get_audio", json!({ "id": h.remote.to_string() }));
    assert_eq!(audio["content"][1]["type"], "audio");
    assert_eq!(audio["content"][1]["mimeType"], "audio/ogg");
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(audio["content"][1]["data"].as_str().unwrap()).unwrap();
    assert_eq!(bytes, h.audio);
    c.finish();
}

#[test]
fn turning_access_off_applies_to_the_open_connection() {
    let h = history();
    set(h.dir.path(), on(true, false));
    let mut c = Client::ready(h.dir.path());
    assert_eq!(c.call("recent", json!({}))["isError"], false);
    set(h.dir.path(), McpSettings::default());
    assert_eq!(c.call("recent", json!({}))["isError"], true);
    assert_eq!(c.tools(), vec!["get_status"]);
    c.finish();
}

#[test]
fn logs_never_carry_what_was_asked_or_said() {
    let h = history();
    set(h.dir.path(), on(true, false));
    let mut c = Client::ready(h.dir.path());
    c.call("search", json!({ "query": "deck tonight" }));
    c.call("get_event", json!({ "id": h.me.to_string() }));
    let log = c.finish();
    assert!(log.contains("mcp call"), "calls are logged: {log}");
    for secret in ["deck", "tonight", "board", &h.me.to_string()] {
        assert!(!log.contains(secret), "log leaked '{secret}': {log}");
    }
}

#[test]
fn a_missing_history_means_access_off() {
    let empty = tempfile::tempdir().unwrap();
    let mut c = Client::ready(empty.path());
    assert_eq!(c.tools(), vec!["get_status"]);
    let status = c.call("get_status", json!({}));
    assert!(status["content"][0]["text"].as_str().unwrap().contains("no tuldMi history"));
    c.finish();
    assert!(!empty.path().join("tuldmi.db").exists());
}

#[test]
fn remote_audio_stays_hidden_while_remote_is_off() {
    let h = history();
    set(h.dir.path(), on(false, true));
    let mut c = Client::ready(h.dir.path());
    assert!(c.tools().contains(&"get_audio".to_string()));
    let refused = c.call("get_audio", json!({ "id": h.remote.to_string() }));
    assert_eq!(refused["isError"], true);
    assert!(refused["content"][0]["text"].as_str().unwrap().contains("No shared event"), "same answer as a missing id");
    let missing = c.call("get_audio", json!({ "id": Uuid::new_v4().to_string() }));
    assert_eq!(missing["content"][0]["text"], refused["content"][0]["text"]);
    c.finish();
}

#[test]
fn audio_is_not_offered_on_the_oldest_protocol() {
    let h = history();
    set(h.dir.path(), on(true, true));
    let mut c = Client::ready_with(h.dir.path(), "2024-11-05");
    assert!(!c.tools().contains(&"get_audio".to_string()));
    assert_eq!(c.call("get_audio", json!({ "id": h.remote.to_string() }))["isError"], true);
    c.finish();
}

#[test]
fn a_busy_history_cannot_crowd_out_the_conversation_asked_for() {
    let h = history();
    set(h.dir.path(), on(true, false));
    // 600 events a minute apart, ending 10 minutes before the one asked about: one long
    // conversation that is not the one at the requested time.
    let store = Store::open(h.dir.path()).unwrap();
    let session = store.start_session(0).unwrap();
    let target = now() - 2 * 3_600_000;
    for i in 0..600 {
        let at = target - 10 * 60_000 - (600 - i) * 60_000;
        store.insert_event(&NewEvent { session_id: session.id, source: Source::Mic, started_at: at, ended_at: at + 1_000, audio: None }).unwrap();
    }
    let asked = store.insert_event(&NewEvent { session_id: session.id, source: Source::Mic, started_at: target, ended_at: target + 1_000, audio: None }).unwrap();
    drop(store);
    let mut c = Client::ready(h.dir.path());
    let at = chrono::DateTime::from_timestamp_millis(target).unwrap().to_rfc3339();
    let conversation = c.call("conversation_at", json!({ "time": at }));
    let events = conversation["structuredContent"]["conversation"]["events"].as_array().expect("found");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["id"], asked.id.to_string());
    // Asked inside the long one: it is longer than what is read, and says so.
    let inside = chrono::DateTime::from_timestamp_millis(target - 10 * 60_000 - 60_000).unwrap().to_rfc3339();
    let long = c.call("conversation_at", json!({ "time": inside }));
    assert_eq!(long["structuredContent"]["truncated"], true);
    c.finish();
}
