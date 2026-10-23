# tuldMi v0.1.0 — Build Mission (product spec)

Operator-authored brief, 2026-10-22. This is the source of truth for scope and the bar. Agents:
read in full before starting any unit.

**Mission:** Ship a genuinely functional, beautiful, installable v0.1.0. Write code, not just
plans. Do not stop at scaffolding. When a feature is hard, build the strongest functional
fallback and keep going. This must feel like a polished consumer product from its first release.

## 1. Product identity and philosophy

- **Product:** tuldMi · **Tagline:** Your conversations, remembered. · **Version:** 0.1.0
- **License:** MIT · **Platforms:** Windows first; macOS and Linux architectural support
- **Distribution:** GitHub releases, native installers where feasible
- **Privacy:** local-first, offline-capable, no telemetry or cloud dependency

tuldMi is an ambient audio memory application. It sits in the system tray, listens to permitted
microphone and system audio, detects speech, produces timestamped transcripts, and lets users
navigate their conversational history. Think: premium audio production software × beautifully
designed personal journal × searchable knowledge system × always-on personal memory timeline ×
the effortless feel of a well-engineered native macOS app.

Not a meeting assistant with a big Record button and an AI summary box. A **continuous,
intelligent, searchable personal audio timeline**. May later integrate with rsRewind; stay
independently installable with an architecture that allows that.

## 2. NON-NEGOTIABLE: UX is first-class engineering

Visual polish, responsiveness, interaction design, accessibility, and animation are core
requirements. No Electron-era enterprise dashboard. No default component styling as the finished
design. No gradients and blur substituting for real hierarchy. A coherent design language.

**Visual direction:** sophisticated, calm, deliberate, tactile. Premium audio software's precision,
Linear's responsiveness and density, Raycast's command-oriented elegance, Apple's restrained
typography and transitions, a journal's warmth — without copying any of them. Distinctive tuldMi
identity. Dark and light themes respecting system preference, carefully chosen accent palette.
Strong hierarchy, restrained surfaces, beautiful typography, excellent spacing, soft elevation,
intentional borders, consistent motion. The primary experience is an elegant chronological
conversation timeline, not a table.

**Canvas / advanced rendering** where it materially helps: live waveform, animated mic and
system-audio activity indicators, interactive waveform scrubbing, timeline zoom/pan, segment
preview, recording-state transitions, playback progress. Not for text, buttons, controls or
accessibility-critical UI. rAF used responsibly: frame-rate limiting, DPR handling, cleanup,
reduced motion. Target smooth 60 FPS without wasting CPU/GPU.

**Motion system:** microinteractions 100–180 ms; panels 180–280 ms; major views 250–400 ms;
ambient visualization continuous and performance-adaptive. Intentional easing; prefer
transform/opacity. Every animation communicates state, continuity, hierarchy, or feedback —
nothing purely decorative.

**Interaction requirements:** instant-feeling search, beautiful empty states, skeletons, helpful
inline errors, context menus, keyboard shortcuts, hover/focus states, fluid transcript expansion,
accessible playback controls, selection and copy, useful tooltips, pleasant settings controls,
thoughtful loading/processing states. Respect reduced motion, screen readers, keyboard navigation,
contrast. **The app should feel excellent even when no audio has been recorded yet.**

## 3. Recommended technical architecture

Use unless investigation establishes a substantially better approach.

**Desktop:** Tauri 2, Rust backend, React + TypeScript, Vite, Tailwind CSS 4, a small headless
accessible component toolkit where useful, Motion for React if justified, Lucide icons, Canvas for
waveform/timeline. Avoid unnecessary runtime weight.

**Persistence:** SQLite; FTS5 for transcript search; migrations; timestamped speech events; Opus
audio files; configurable retention; crash-safe processing state.

**Audio capture:** an `AudioCaptureProvider` abstraction. Windows first: WASAPI microphone +
render-endpoint loopback. Planned: macOS (ScreenCaptureKit / Core Audio), Linux (PipeWire).
Preserve separate mic and system tracks throughout. Handle device changes, sample-rate
differences, interruptions, disconnections. Common monotonic clock, converted to persisted
wall-clock time. No audio work on the UI thread.

**VAD:** investigate Silero VAD with a compatible MIT-licensed inference implementation. Bounded
rolling buffers with configurable activation threshold, min speech duration, pre-roll, post-roll,
min silence, max segment duration. Defaults: 300 ms pre-roll, 700 ms post-roll, ~2 s rolling
history. Don't store long silences; keep accurate timestamps for omitted periods. Deliberate
mechanism against losing speech at segment boundaries. Don't claim VAD retains non-speech.

**Transcription:** pluggable `TranscriptionProvider`. Investigate Parakeet TDT 0.6B v3,
whisper.cpp, faster-whisper (optional/dev). Choose the best realistically packageable local
backend, prioritizing reliability, CPU support, small footprint, acceleration where available,
word/segment timestamps, offline, language support, clear model licenses. Never download weights
on every launch. First-run model setup with download progress, size, disk-space info, actionable
errors. No silent large downloads without consent. Capture keeps working while the engine loads
or works through a backlog.

**Diarization:** v0.1.0 mandatory source separation only — Microphone → Me, System audio → Remote.
Multi-speaker diarization optional. Never imply separate remote speakers were identified unless a
real diarization model did it. Future-compatible speaker identity schema (embeddings, manual
naming, background diarization).

## 4. Core user experience

**A. First launch / onboarding:** explain what tuldMi does, which sources are captured, where data
is stored, what stays local, required permissions, how to pause instantly, how retention works.
Device selection for mic and system audio. Live audio test with responsive waveform. Guided local
model install. Meaningful ready state. No account, login, telemetry opt-in, or subscription.

**B. System tray (fundamental):** recording status, pause/resume, open timeline, device status,
processing backlog, settings, quit. Clear recording indicator. Opt-in launch at login. Closing the
main window must not silently end an active session — communicate it. Configurable global
pause/resume shortcut.

**C. Main timeline (centerpiece):** each event has absolute timestamp, audio source,
speaker/source label, text, processing status, audio reference when retained, session, confidence
when available. Group nearby events naturally; easy to scan without losing exact chronology.
Distinguish mic vs system audio with restrained accents and excellent typography. Show meaningful
gaps without wasting space. Thoughtful transitions for arriving segments. Never yank scroll to the
bottom while reviewing older material; provide a clear return-to-live.

**D. Audio visualization:** live mic and system activity, smooth amplitude, animated recording
indicator, playback waveform, seek/scrub, high-DPI, reduced-motion behavior, graceful fallback for
missing audio. Real levels only. Efficient, all animation resources cleaned up.

**E. Search:** SQLite FTS5 across all transcripts, match highlighting, timestamp navigation,
source and date filters, useful empty results, jump straight to matching segments. Debounced but
not sluggish. Offline.

**F. Playback:** play/pause, seek, waveform, speed controls, current timestamp, segment-relative
progress, clear unavailable-audio states. Clicking an event plays its retained audio. Expired audio
→ transcript still shown normally.

**G. Settings (designed, not a config dump):** audio sources, transcription engine, model
management, recording behavior, retention, privacy and data controls, appearance, keyboard
shortcuts, startup, storage usage (clear disk consumption, open storage folder), about / version /
license. Explicit delete controls with confirmation.

## 5. Data architecture

Long-lived memory database. Suggested tables: sessions, audio_events, transcript_segments,
speakers, audio_assets, transcription_jobs, settings, model_installations. Migrations from day
one. Normalized timestamps, stable UUIDs. Source types tracked independently of speaker identity.
rsRewind-compatible. Audio as external Opus files (voice-tuned), not SQLite blobs. Retention:
delete after transcription / 7 days / 30 days / indefinitely. Transcripts independent of audio
retention. Partial/failed jobs recover after restart. Atomic file operations.

## 6. Privacy and safety

Local-only by default; no telemetry; no cloud dependency; obvious recording state; immediate
pause/resume; mic and system toggles; clear permissions onboarding; configurable retention; manual
deletion; no covert recording features; secure handling of files and temp buffers. Sensible
architecture for encryption at rest (full key management may be post-0.1.0) — never present a
password toggle as encryption unless it actually protects data. Clear disclosure that users are
responsible for recording-consent laws and workplace policies. Never make hidden recording a
selling point.

## 7. Quality and reliability

Capture runs independently of the window. Closing/reopening the window doesn't interrupt capture.
Inference never freezes the UI. Recording continues while transcription catches up. Capture errors
surface. Failed jobs retryable. Devices reselectable. Data survives restart. Paused is visibly
paused. Search works on real stored transcripts. Bounded queues, no unbounded growth, no data drops
under normal load. Meaningful Rust error handling and structured logging without transcript
contents. Unit and integration tests for the critical pipeline.

## 8. Licensing and dependency compliance

App source MIT. Audit major dependencies and model licenses; distinguish app license, third-party
library licenses, downloadable model licenses, redistribution restrictions on weights. Don't
assume open = MIT. Avoid copyleft/source-available deps that conflict with distribution. Document
attribution obligations for other permissive licenses. If an MIT-compatible model bundle isn't
practical, offer separate optional downloads with disclosure. LICENSE + third-party notices.

## 9. Execution phases

1. **Establish the product** — inspect environment, research only decisions that matter for
   0.1.0, init Tauri/Rust/React, build the design system, typography, themes, navigation, motion
   primitives, polished shell, and a fully interactive timeline on **explicitly labelled demo
   data**.
2. **Real capture and persistence** — Windows mic + loopback, silence detection, Opus segments,
   live meters on real signal, SQLite + migrations + session state. Verify record and replay.
3. **Actual transcription** — working local backend fed by VAD events through a job queue;
   persist and display results; loading, missing-model, failed-job, backlog states.
4. **Timeline experience** — live SQLite data, search, playback, filters, navigation, export,
   microinteractions, finished settings.
5. **Hardening and release** — type checks, Rust checks, tests, production builds, full
   capture→transcript→search→playback exercise, regressions fixed, native installer, release docs,
   changelog. Screenshot and visually inspect rendered screens.

## 10. Definition of done for v0.1.0

A user can: (1) install and launch; (2) complete onboarding; (3) select an input; (4) capture real
mic audio; (5) capture system audio on Windows; (6) pause/resume from the tray; (7) see real live
activity; (8) have speech detected and segmented; (9) have it transcribed locally; (10) see
transcripts appear in the timeline; (11) search past text; (12) replay retained audio; (13) close
and reopen the window without losing history; (14) adjust recording and retention settings;
(15) quit cleanly; (16) experience a cohesive, highly polished interface.

Anything unimplemented is disclosed in the release notes — no nonfunctional controls or fakes.
Prefer a smaller fully working product over a sprawling broken one — but that is never an excuse
to abandon polish. **Visual polish is part of the MVP.**

## 11. Repository and delivery

Create README.md, LICENSE (MIT), CHANGELOG.md, CONTRIBUTING.md, docs/ARCHITECTURE.md,
docs/PRIVACY.md, docs/MODELS.md, docs/RELEASE.md. README: description, screenshots, features,
platform matrix, install, development, privacy, limitations, licensing. Logical modules for Rust,
frontend, capture, inference, persistence — no elaborate monorepo or premature abstraction.
Meaningful commits at milestones. Never publish or push to an unconfirmed destination.

Final report covers: what works, what's incomplete, what was tested, what's only structural, build
outputs and installer locations, performance observations, known bugs, v0.1.1 priorities. Never
report success based solely on generated source.

## 12. Final product bar

Within thirty seconds of first launch it should be obvious the product was designed with care:
distinctive personality, intentional motion, polished controls, clear privacy boundaries,
trustworthy. A beautiful interface with fake functionality is unacceptable. A functional recorder
with a mediocre interface is also unacceptable. **tuldMi is not an engineering demo. It's the
beginning of a genuinely excellent personal memory application.**
