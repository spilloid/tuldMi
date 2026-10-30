# Privacy

tuldMi records conversations. That makes privacy the product, not a feature. This document says
exactly what happens to your audio, in plain terms, and what tuldMi does **not** do yet.

## What tuldMi captures

- **Microphone** (labelled **Me**) and **system audio** — what your computer plays, e.g. the other
  side of a call (labelled **Remote**). Each can be turned off independently, and both stop
  completely when you pause.
- Only **speech** is kept. A voice-activity detector runs on your device and discards silence and
  non-speech before anything is written to disk. (It isn't perfect: loud non-speech sound can
  occasionally be kept, and very quiet speech can be missed.)
- "Remote" means "whatever came out of your speakers". tuldMi does **not** identify individual
  remote speakers in v0.1.0.

## Where it goes

Nowhere. Everything stays on your computer:

| What | Where (Windows) |
|---|---|
| Transcripts, settings, index | `%APPDATA%\com.spillerstech.tuldmi\tuldmi.db` |
| Retained speech audio | `%APPDATA%\com.spillerstech.tuldmi\audio\` (Ogg Opus files) |
| Transcription models | `%APPDATA%\com.spillerstech.tuldmi\models\` |
| Logs | `%APPDATA%\com.spillerstech.tuldmi\logs\` — technical events only, never transcript text or audio |

Settings → Privacy & data → *Open data folder* takes you there.

- **No telemetry, analytics, crash reporting, accounts, or cloud processing.** Transcription runs
  locally with whisper.cpp.
- **The only network request tuldMi makes** is downloading a transcription model, and only when
  you click *Download*. It goes to Hugging Face (huggingface.co), shows the size first, and the
  file is verified with SHA-256.

## How long it stays

- **Transcripts** are kept until you delete them.
- **Audio** follows your retention setting: delete right after transcription, keep 7 days, keep
  30 days (default), or keep forever. Audio still waiting to be transcribed is never deleted by
  retention.
- **Exception: clips too quiet to transcribe.** When a clip is too quiet for the transcriber to
  try, it has no transcript, so "delete right after transcription" doesn't apply to it. **In
  v0.1.0 that audio is kept until you delete it**, including any quiet speech from the other side
  of a call. The next release removes it 24 hours after recording. The 7- and 30-day settings
  already remove it on schedule. Such clips show "Too quiet to transcribe; audio kept" in the
  timeline, where you can delete them.
- You can delete one conversation's audio, one conversation entirely, or all history
  (Settings → Privacy & data). Deletion removes the database rows and the audio files.

## Always visible, always stoppable

- The tray icon shows whether tuldMi is recording, paused, or having device trouble.
- Pause from the tray, the main window, or the global shortcut (default **Ctrl+Shift+`**).
  Paused means the devices are closed — nothing is captured at all.
- Closing the window does not stop recording; the first time, tuldMi tells you so and offers to
  pause.
- tuldMi has no hidden or covert mode and never will.

## Your responsibility

Recording conversations is regulated. Many places require the consent of everyone being
recorded, and many workplaces have policies about recording calls and meetings. **You are
responsible for complying with the laws and policies that apply to you.** When in doubt, tell
people you're using tuldMi, or pause it.

## Not yet (honest limits of v0.1.0)

- **No encryption at rest.** The database and audio files are protected only by your Windows
  account and disk encryption (e.g. BitLocker). Anyone with access to your user account can read
  them. Database encryption (SQLCipher) and per-file audio encryption with OS-keychain keys are
  planned; tuldMi will not show a password or "encrypted" toggle until that actually protects
  the data.
- Temporary audio buffers live in process memory only; tuldMi does not write unencrypted temp
  files outside its data folder, but memory is not locked against paging to the system page
  file.
