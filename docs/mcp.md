---
title: Using tuldMi from an AI assistant (MCP)
---

# Using tuldMi from an AI assistant (MCP)

`tuldmi-mcp` lets an AI assistant search and read what was said: "what did Dana say about the
deadline on Tuesday's call?", "find where I promised to send the deck", "summarize this morning's
standup". It speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio: your
assistant starts it as a program. tuldMi still opens no network port and makes no network request.

## Before you turn it on

**Assistants send what they read to their AI provider**, which usually runs on the provider's
servers. Turning this on is a way part of your history can leave your computer through that
assistant. It is **off** until you turn it on in tuldMi: **Settings › Privacy & data › AI
assistants**. A dialog says what will be shared first.

Once on, an assistant can read transcripts from the **last 30 days** of **your own side (Me)**.
It **cannot**:

- read the other side of your calls (Remote) unless you include it. Remote is other people's
  words, so only include it if they have agreed;
- fetch audio unless you share it;
- resume recording, delete anything, export, or change settings. It can pause recording.

Turning access off, or narrowing it, takes effect on the assistant's next request.

These limits apply to what `tuldmi-mcp` answers, and nothing else. An assistant that can also read
files or run commands on your computer (Claude Code and other coding assistants usually can) can
open the data folder directly, audio included, whether or not this is on; the history is not
encrypted ([PRIVACY.md](PRIVACY.md)). Treat such an assistant as having access to everything
tuldMi recorded.

## Connect your assistant

The program is installed with tuldMi:

- **Windows:** `%LOCALAPPDATA%\tuldMi\tuldmi-mcp.exe`
- **macOS:** `/Applications/tuldMi.app/Contents/MacOS/tuldmi-mcp`

Settings › Privacy & data › AI assistants shows the exact command for this computer, with a copy
button.

| Client | Setup |
|---|---|
| Claude Code | `claude mcp add tuldmi -- <path to tuldmi-mcp>` |
| Claude Desktop | `claude_desktop_config.json`: `{ "mcpServers": { "tuldmi": { "command": "<path to tuldmi-mcp>" } } }` |
| Cursor | `.cursor/mcp.json` (or the global one): the same `mcpServers` entry |
| Codex CLI | `~/.codex/config.toml`: `[mcp_servers.tuldmi]` with `command = "<path to tuldmi-mcp>"` |

Restart the assistant's connection after changing what is shared, so its list of tools refreshes.

## Tools

| Tool | What it does |
|---|---|
| `search` | Find words in transcripts, with Me/Remote and time filters (at most 50 matches) |
| `recent` | The latest speech, or what came before a time (at most 100 events) |
| `conversation_at` | The conversation around a time, grouped like the timeline (events less than 3 minutes apart) |
| `get_event` | One event's full transcript with the time of each segment |
| `get_audio` | One event's recording, Ogg Opus, at most 4 MiB (only if you share audio) |
| `get_status` | Whether access is on, what it allows, and how much history is in reach (always available) |
| `pause_recording` | Pause tuldMi now. It can't resume; only you can |

Times can be RFC 3339, `YYYY-MM-DD HH:MM`, `YYYY-MM-DD`, or an age such as `30m`, `2h`, `7d`; local
times use this computer's time zone. `me` is your microphone; `remote` is everything your computer
played (the other side of calls), not told apart by speaker. Transcripts are machine-made and can
contain mistakes.

If tuldMi isn't running, `pause_recording` says so; the launched copy exits at once without recording
or showing anything.

## Separated speakers

When tuldMi has separated the other side of a call into speakers (Settings → Transcription →
Speakers), results name them: event text marks each change of speaker ("Speaker 1: … Dana: …"),
`get_event` segments carry a `speaker`, events list their `speakers`, and search matches say who
said them. Speech nobody could be sure about stays "Remote". Speaker names are part of Remote, so
an assistant sees them only with *Include the other side (Remote)* on.

## What is logged

`tuldmi-mcp` writes one line per call to standard error, which assistants show in their MCP logs:
the tool's name, a call number, whether it ran or was refused, the number of results and how long
it took. Never what was asked, transcript text, audio, or the assistant's request ids.

Supported protocol versions: `2025-11-25`, `2025-06-18`, `2025-03-26` (with JSON-RPC batches),
`2024-11-05`. Clients send `initialize`, then `notifications/initialized`, before using tools. The
server exits when standard input closes. No HTTP endpoint, resources, prompts or subscriptions.
