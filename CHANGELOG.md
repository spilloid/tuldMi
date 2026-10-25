# Changelog

All notable changes to tuldMi are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

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

[0.1.0]: https://github.com/spilloid/tuldMi/releases/tag/v0.1.0
