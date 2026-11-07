# MCP server: what was said, for AI assistants

Status: **design, approved direction 2026-11-07** (a separate `tuldmi-mcp` binary, target 0.3.0).
Modelled on rsRewind's MCP server (`../rsRewind/docs/design/mcp.md`, STD-005 agent surface).
tuldMi takes the patterns, not the code: the products share no code dependency (CLAUDE.md).

## What it is for

"What did Dana say about the deadline on Tuesday's call?", "find where I promised to send the
deck", "summarize this morning's standup". An assistant that can search and read the transcript
history answers those directly. `tuldmi-mcp` gives Claude Code, Claude Desktop, Cursor, Codex or
any MCP client that ability, read-only, over stdio.

## Shape

- **A separate binary, `tuldmi-mcp`**, shipped inside the installer and the app bundle (Tauri
  `externalBin`), signed like the app. It is not the Tauri app in a special mode: an assistant
  launching it must not start a WebView, take the single-instance lock or open audio devices.
- **Crate `crates/mcp`** (`tuldmi-mcp`): a library (protocol, tools, ceiling) plus a thin `main`.
  It depends on `tuldmi-engine` with `default-features = false` (no whisper) and reads only
  through a new **`Store::open_read_only`**: `SQLITE_OPEN_READ_ONLY`, no migrations, refuses a
  schema newer than it knows, and fails with "no history yet" when there is no database. The
  app's writes (WAL) are visible to it at once.
- **stdio only.** Newline-delimited JSON-RPC 2.0 on stdin/stdout, so tuldMi still opens no
  network port and makes no network request. Protocol versions `2025-11-25`, `2025-06-18`,
  `2025-03-26` (batches), `2024-11-05`; initialize, then `notifications/initialized`, then tools.
  The server exits when stdin closes. Messages over 1 MiB are refused without being buffered.
- **Data folder**: the app's (`%APPDATA%\com.spillerstech.tuldmi`,
  `~/Library/Application Support/com.spillerstech.tuldmi`); `--data-dir` overrides it for tests.

## Privacy: what is different about audio memory

An MCP client sends tool results to its model, which usually runs on the provider's servers. So
**using these tools sends parts of your transcripts off this computer**, the one thing tuldMi
otherwise never does. And unlike a screen history, half of what tuldMi holds is **other people's
words** (Remote). Therefore:

1. **Off by default.** Until enabled, `tools/list` offers only `get_status`, which says how to
   turn access on. Enabling is a deliberate act in the app (Settings › Privacy & data › AI
   assistants), behind a dialog that says what will leave the computer and where.
2. **Remote is a separate opt-in** (`includeRemote`, default **off**). With it off, the other
   side of every call does not exist as far as MCP is concerned: search, listings and counts
   leave it out. Turning it on asks the user to confirm they have the consent of the people
   recorded, the same responsibility as recording itself.
3. **Audio is a separate opt-in** (`allowAudio`, default off): text first, sound only if asked.
4. **A time window** (`maxAgeDays`, default 30; 0 = no limit) caps every result.
5. The **ceiling** is the stored settings, **re-read on every request**: turning access off in
   the app takes effect on an open connection's next call. Unreadable settings mean access off.
   It filters `tools/list` and is re-checked on every call (discovery filtering alone is not a
   boundary).
6. Results carry time and source (Me/Remote) so an agent can cite them. Transcript text is never
   logged.

What it does not protect: an assistant that can read files or run commands (Claude Code and
other coding agents usually can) can open the data folder directly, whatever these settings say;
the database is not encrypted (PRIVACY.md). The docs say so plainly.

## Settings (`Settings.mcp`, stored with the other settings)

```json
"mcp": { "enabled": false, "includeRemote": false, "allowAudio": false, "maxAgeDays": 30 }
```

Old settings without the key load as these defaults (`#[serde(default)]`).

## Tools

| Tool | What it does | Class | Needs |
|---|---|---|---|
| `get_status` | Whether access is on, what it allows, event counts in scope, newest event time, whether tuldMi is running | read | always offered |
| `search` | Full-text search of transcripts (`query`, optional `source` me/remote, `from`, `to`, `limit` ≤ 50) | read | enabled |
| `recent` | Latest events, or those before a time (`before`, `source`, `limit` ≤ 100) | read | enabled |
| `get_event` | One event: time, source, duration, status, language, every segment with its time | read | enabled |
| `conversation_at` | The conversation around a time, grouped like the timeline (events less than 3 minutes apart) | read | enabled |
| `get_audio` | One event's audio (Ogg Opus, ≤ 4 MiB) as MCP audio content | read | enabled + `allowAudio` |
| `pause_recording` | Pause capture now. Cannot resume, delete or change settings | write (reversible) | enabled |

- Times: RFC 3339, `YYYY-MM-DD HH:MM`, `YYYY-MM-DD`, or an age like `30m`, `2h`, `7d`; local
  times use this computer's time zone.
- Every tool has annotations (`readOnlyHint`, `destructiveHint: false`, `openWorldHint: false`),
  an `inputSchema`, and an `outputSchema` its `structuredContent` matches; text content mirrors it.
- Out of scope: resuming, deleting, exporting, settings changes, resources, prompts,
  subscriptions, HTTP transport.

### Pausing from another process

`pause_recording` runs the app executable with `--pause`. The single-instance plugin hands the
arguments to the running app, which pauses (the same path as the tray, so the tray icon changes
and the pause is visible). If tuldMi isn't running, the new process sees `--pause`, does nothing
and exits at once **without starting the app**, and the tool answers "tuldMi isn't running, so
nothing is being recorded". It never resumes: a running app that is already paused stays paused.

## Logging

Each call logs, to stderr: static tool name, `channel=mcp`, process id, a server-generated call
number, `executed`/`refused`, result count and duration. Never arguments, client request ids,
transcript text or audio.

## Packaging

- `scripts/build-mcp.mjs` (run by `beforeBuildCommand` after the frontend build) builds
  `tuldmi-mcp` for the bundle's target and copies it to `src-tauri/binaries/tuldmi-mcp-<triple>`.
- Installed at `%LOCALAPPDATA%\tuldMi\tuldmi-mcp.exe` and
  `/Applications/tuldMi.app/Contents/MacOS/tuldmi-mcp`. Signed: Authenticode via the existing
  `signCommand` on Windows (verified after install), ad-hoc + hardened runtime on macOS
  (verified in CI with `codesign --verify`).
- The Settings page shows the exact command for each client with the installed path, and a copy
  button.

## Tests

- Protocol contract over a real pipe: spawn the built `tuldmi-mcp` on a synthetic history,
  `initialize` → `tools/list` (exact advertised set per ceiling) → `tools/call` for each tool.
- Ceiling: off → only `get_status`; Remote off → no Remote anywhere (search, recent, counts,
  `get_event` on a Remote id is "not found"); age limit; audio gate; a hidden tool called directly
  is refused; settings changed mid-connection apply on the next call.
- `Store::open_read_only`: no writes possible, newer schema refused, missing database.
- Logging: a call's log line contains none of the query text or results.
- `--pause` with no running app exits without starting it (shell test).
