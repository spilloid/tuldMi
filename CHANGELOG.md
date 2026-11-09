# Changelog

All notable changes to tuldMi are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.3.0] — 2026-11-08

### Added

- **AI assistants (MCP), off by default.** `tuldmi-mcp`, installed with tuldMi, lets Claude Code,
  Claude Desktop, Cursor, Codex or any MCP client search and read your transcripts over stdio
  (`search`, `recent`, `conversation_at`, `get_event`, `get_audio`, `get_status`,
  `pause_recording`). Turn it on in Settings › Privacy & data › AI assistants, which says what
  leaves the computer: assistants send what they read to their AI provider. Then it reads your
  own side (Me) from the last 30 days; the other side of calls (Remote) and audio are separate
  opt-ins. It can pause recording, never resume it, delete or change anything. Read-only, opens
  no network port, and logs no transcript text. Setup: docs/mcp.md.
- `tuldmi --pause` pauses a running tuldMi (and starts nothing if none is running).

## [0.2.0] — 2026-11-07

### Added

- **macOS preview (Apple Silicon, macOS 15.4+).** Microphone through Core Audio; system audio
  through a Core Audio process tap that captures every app's sound, whichever output you use, and
  leaves playback untouched. Distributed as a disk image built in CI, ad-hoc signed and **not
  notarized** (Gatekeeper asks once; see docs/INSTALL.md).
- macOS app behaviour: menu bar icon (click for the menu), app menu with ⌘Q, ⌘, and working
  copy/paste, native traffic lights, Dock click reopens the window, ⌘-based shortcut labels.
  The default pause shortcut on macOS is ⌥⌘` (macOS uses ⌘⇧` to cycle windows).
  Copy says "menu bar" instead of "tray" on a Mac.
- Settings show where to grant Microphone and System Audio Recording permission on macOS, since a
  denied permission arrives as silence, not an error.

### Known limitations

- macOS: not notarized; each ad-hoc signed update asks for both permissions again; no Intel
  build. macOS 15.4 or later, because playback of the stored Ogg Opus clips needs it. Switching
  outputs and a coreaudiod restart are handled in code but untested on hardware.

## [0.1.1] — 2026-10-30

### Fixed

- The tray and window could show **Paused** while recording: two status updates racing could
  deliver the older one last, and the newer one was then never resent.
- Audio Windows refused to delete (another program holding the file) was forgotten until the
  next start. It is now retried every 30 seconds.
- Under "delete after transcription", audio of clips too quiet to transcribe was kept forever.
  It is now removed 24 hours after recording (time to play it; retention checks every 10
  minutes). Clips kept by v0.1.0 are removed at the first check. The timeline now marks them
  "Too quiet to transcribe". The 7- and 30-day settings already treated it like other audio.
  Audio still waiting to be transcribed is never removed.

## [0.1.0] — 2026-10-24

First release. Windows 10/11 x64.

### Added
- Ambient capture of microphone (**Me**) and system audio via WASAPI loopback (**Remote**) as
  separate tracks, with device selection, default-device following, and automatic recovery when a
  device disappears.
- Local voice-activity detection (Silero VAD v6.2, bundled) with 300 ms pre-roll, 700 ms
  post-roll, hysteresis, and overlap-splitting of long speech so no words fall between segments.
  Energy-based fallback if the model can't load (the UI says which is active).
- Speech stored as Ogg Opus (16 kHz mono, 24 kbps), written atomically.
- Local transcription with whisper.cpp; five downloadable Whisper models (74–547 MB), explicit
  consent, progress, cancel, SHA-256 verification. Capture continues while transcription catches
  up; failed jobs can be retried.
- SQLite store with migrations, FTS5 full-text search (highlights, Me/Remote and date filters),
  crash recovery for interrupted jobs and sessions, secure deletion.
- Timeline grouped by day and conversation, quiet-gap rows, live arrivals that never steal your
  scroll position, day index, keyboard navigation.
- Docked player with scrubbable waveform, speed control, segment-relative progress.
- Command-palette search (Ctrl+K or `/`) and a full search view.
- Seven-step onboarding: what's captured, privacy and storage, recording-consent
  acknowledgement, live device test (nothing stored), model setup.
- Settings: audio sources, transcription and models, speech detection, retention (delete after
  transcription / 7 days / 30 days / forever), privacy and data (export Markdown/JSON, open data
  folder, delete all), appearance (system/light/dark), shortcuts, launch at login, storage usage.
- System tray with runtime-drawn state icons, pause/resume, per-source status and backlog;
  global pause shortcut (Ctrl+Shift+`); close-to-tray with a first-time explanation.
- Per-user NSIS installer; installer, app and uninstaller Authenticode-signed (Azure Artifact
  Signing, timestamped).

### Known limitations
- Windows only. No speaker diarization (Remote is all system audio). No encryption at rest. CPU
  transcription only. Real-hardware unplug/default-device switching not yet
  exercised (covered by simulated-device tests). See README.
- Deferred to 0.1.1 (found in review, low severity): audio of clips too quiet to transcribe is kept
  indefinitely under "delete after transcription"; the tray uses the recording glyph while testing
  devices; a settings save can wait up to 5 s during a large retention sweep; rare out-of-order
  status publish between engine threads.

[0.3.0]: https://github.com/spilloid/tuldMi/releases/tag/v0.3.0
[0.2.0]: https://github.com/spilloid/tuldMi/releases/tag/v0.2.0
[0.1.1]: https://github.com/spilloid/tuldMi/releases/tag/v0.1.1
[0.1.0]: https://github.com/spilloid/tuldMi/releases/tag/v0.1.0
