# tuldMi architecture

tuldMi is three layers with one-way dependencies:

```
src/ (React + TypeScript, Vite)          the product surface; never touches devices or disk
  └── IPC: Tauri commands + events (docs/IPC.md)
src-tauri/ (Tauri 2 shell)               window, tray, global shortcut, autostart, command glue
  └── tuldmi-engine (crates/engine)      everything that captures, decides, stores, transcribes
```

The engine has no Tauri dependency and is tested on its own (`cargo test -p tuldmi-engine`).

## The pipeline

```
 device callback (cpal / WASAPI)          [capture thread per source, provider-owned]
   │  copy buffer, try_send ──► bounded channel (512 buffers ≈ 5 s)   full ⇒ drop + count + notice
   ▼
 tuldmi-proc                              [one thread, all sources]
   ToMono16k (downmix + rubato sinc resample to 16 kHz)
   ├─► LevelMeter ──► Levels @ ~30 Hz ──► UI meters (real signal, never simulated)
   └─► SpeechDetector (Silero v6 via whisper.cpp ggml; energy fallback)
         └─► Segmenter (pre-roll 300 ms, post-roll 700 ms, hysteresis, 30 s max with
               overlap-split, 2 s idle history; memory bounded)
               └─► SpeechSegment ──► write_ogg_opus (atomic .part → rename)
                                     └─► Store::insert_event (event + asset + queued job, one txn)
                                           └─► EventUpserted ──► UI; wake ASR
 tuldmi-asr                               [one thread]
   Store::next_job ─► read_ogg_opus_16k ─► WhisperProvider::transcribe ─► save_transcript
   ─► (retention = delete-after-transcription ⇒ expire audio) ─► EventUpserted
 tuldmi-house                             [one thread]
   retention sweep (start + every 10 min), orphan sweep (start),
   device recovery + default-device following (every 3 s)
```

Capture never waits on encoding, the database, or transcription. If transcription falls behind,
the queue grows on disk (`transcription_jobs`), not in memory.

### Time

Each stream gets a `StreamClock` anchored at its first buffer; sample positions map to one
monotonic `Instant` timeline and then to wall-clock milliseconds through a single
`(Instant, SystemTime)` anchor taken at engine start. Mic and system timestamps therefore share
one clock even if the wall clock moves while recording. Large device-vs-monotonic drift
(> 250 ms) re-bases the stream anchor.

### Segment boundaries

Speech is never cut silently. Pre-roll comes from the idle history ring, post-roll is kept after
the last speech frame, short pauses (< `min_silence_ms`) stay inside a segment, dips are bridged
by hysteresis, and when a segment hits the maximum length it is split at the lowest-probability
frame in its last 3 s with a 250 ms overlap, so a word straddling the cut appears in both halves.
Pause, device loss and shutdown flush the in-progress segment instead of dropping it.

## Persistence

`data_dir` = `%APPDATA%\com.spillerstech.tuldmi` on Windows.

```
tuldmi.db            SQLite (WAL); schema in crates/engine/src/storage/migrations/
audio/YYYY/MM/DD/    <event-uuid>.ogg — Ogg Opus, 16 kHz mono, 24 kbps (~3 KB/s of speech)
models/              downloaded ggml Whisper models
logs/                daily-rotated tuldmi.log (no transcript text, no audio content)
```

Tables: `sessions`, `speakers`, `audio_assets`, `audio_events`, `transcript_segments` (+ FTS5
external-content index `transcript_fts` kept in sync by triggers), `transcription_jobs`,
`settings`, `model_installations`. IDs are UUIDv7 text; times are Unix-epoch milliseconds.

Design choices that matter for a long-lived memory database:

- **Source ≠ speaker.** `audio_events.source` (mic/system) is a capture fact; `speaker_id` is an
  identity claim. Today mic ⇒ "Me" (`self`) and system ⇒ "Remote" (`remote`); a future
  diarizer or manual naming adds `person` speakers with optional embeddings, without migration.
- **Transcripts outlive audio.** Retention expires `audio_assets` (path nulled, `expired_at` set)
  and never touches transcripts. Audio still waiting for transcription is never expired.
- **Crash safety.** Every mutation is a single transaction; audio files are written atomically
  before the row that references them; the store never unlinks files itself (it returns paths),
  so a crash leaves at worst an orphan file, swept on next start. Interrupted jobs return to
  `queued` and open sessions are closed on startup.
- **rsRewind compatibility.** Same conventions as rsRewind's store (UUIDs, epoch-ms, external
  media files, append-friendly tables), so a future integration can join on time ranges without
  either app depending on the other.

Encryption at rest is not implemented in v0.1.0 (see PRIVACY.md). The seams for it are the
store's single connection (SQLCipher-compatible) and the single audio writer/reader pair.

## Shell behaviour

- The engine starts with the app, not the window. Closing the window hides it; capture
  continues and the tray shows it. The first such close while recording explains this in a
  native dialog with a "Pause recording" option.
- Tray: status, pause/resume, open, per-source status, transcription backlog, settings, quit.
  Icons are drawn at runtime per state (recording / paused / idle / error).
- Global shortcut (default `Ctrl+Shift+\``) toggles pause from anywhere.
- Launch at login is opt-in (`--minimized` starts in the tray once onboarding is done).
- All commands are `async` (off the UI thread). The asset protocol is scoped to `audio/`.

## Platform providers

| Platform | Mic | System audio | Status |
|---|---|---|---|
| Windows 10/11 | WASAPI (cpal) | WASAPI loopback on the render endpoint (cpal) | implemented |
| macOS 13+ | Core Audio (cpal) | ScreenCaptureKit audio / Core Audio process taps | planned — new `AudioCaptureProvider` |
| Linux | PipeWire/ALSA (cpal) | PipeWire monitor source | planned — new `AudioCaptureProvider` |

## Testing

- `crates/engine/tests/segmenter.rs` — boundary, split, hysteresis, memory-bound behaviour with
  sample-exact position checks.
- `crates/engine/tests/storage.rs` — migrations, lifecycle, job retry/recovery, FTS5 with hostile
  input, retention semantics.
- `crates/engine/tests/encode_dsp.rs` — Opus round-trip, resampler duration/frequency fidelity.
- `crates/engine/tests/engine.rs` — end-to-end on a fake provider using a real speech recording:
  capture → VAD → segment → Opus + SQLite, monitoring stores nothing, shutdown flushes, device
  loss recovers, export; Silero on real speech; Whisper on real speech (opt-in, needs weights).
- Frontend: Vitest unit tests + headless-Edge screenshots of every surface (`docs/screenshots/`).
