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
- "Remote" means "whatever came out of your speakers". With **speaker separation** switched on
  (Settings → Transcription → Speakers, off by default), tuldMi labels the voices in it Speaker
  1, Speaker 2… within each conversation. See [Speaker separation](#speaker-separation).

## Where it goes

Nowhere, unless you turn on AI assistant access (below). Everything stays on your computer:

Paths below are for Windows; on macOS the folder is
`~/Library/Application Support/com.spillerstech.tuldmi/`.

| What | Where (Windows) |
|---|---|
| Transcripts, settings, index | `%APPDATA%\com.spillerstech.tuldmi\tuldmi.db` |
| Retained speech audio | `%APPDATA%\com.spillerstech.tuldmi\audio\` (Ogg Opus files) |
| Transcription and speaker models | `%APPDATA%\com.spillerstech.tuldmi\models\` |
| Logs | `%APPDATA%\com.spillerstech.tuldmi\logs\` — technical events only, never transcript text or audio |

Settings → Privacy & data → *Open data folder* takes you there.

- **No telemetry, analytics, crash reporting, accounts, or cloud processing.** Transcription runs
  locally with whisper.cpp.
- **The only network requests tuldMi makes** are model downloads, and only when you click
  *Download*: a transcription model, and optionally the two speaker-separation models. They go
  to Hugging Face (huggingface.co), show the size first, and each file is verified with SHA-256.
- **AI assistants (MCP), off by default.** If you turn on Settings → Privacy & data → *AI
  assistants*, an assistant you connect (Claude, Cursor, Codex, …) can read your transcripts
  through `tuldmi-mcp`, and **it sends what it reads to its AI provider**. tuldMi itself still
  makes no network request; the assistant does. By default it reads only your side (Me) from the
  last 30 days; the other side of calls (Remote) and audio are separate opt-ins, and Remote is
  other people's words, so include it only with their agreement. Turning access off takes effect
  on the assistant's next request, and also turns Remote and audio off. `tuldmi-mcp` logs which
  tool ran and how many results, never what was asked or said. An assistant that can read files
  on your computer can open the data folder directly whatever this setting says, because the
  history isn't encrypted yet. Details: [mcp.md](mcp.md).

## How long it stays

- **Transcripts** are kept until you delete them.
- **Audio** follows your retention setting: delete right after transcription, keep 7 days, keep
  30 days (default), or keep forever. Audio still waiting to be transcribed is never deleted by
  retention.
- **Exception: clips too quiet to transcribe.** When a clip is too quiet for the transcriber to
  try, it has no transcript, so "delete right after transcription" doesn't remove it at once.
  Since v0.1.1 it is removed 24 hours after recording (retention is checked every 10 minutes, so
  allow a few minutes more), which leaves time to play it. **v0.1.0 kept that audio until you
  deleted it**, including any quiet speech from the other side of a call; installing v0.1.1
  removes those older clips at its first check. The 7- and 30-day settings treat these clips like
  any other audio. They show as "Too quiet to transcribe" in the timeline, where you can delete
  them sooner.
- Retention runs every 10 minutes, so audio can outlive its setting by up to that long.
- You can delete one conversation's audio, one conversation entirely, or all history
  (Settings → Privacy & data). Deletion removes the database rows and the audio files.

## Always visible, always stoppable

- The tray icon shows whether tuldMi is recording, paused, or having device trouble.
- Pause from the tray, the main window, or the global shortcut (default **Ctrl+Shift+`** on Windows, **⌥⌘`** on a Mac).
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

- **No encryption at rest.** The database and audio files are protected only by your OS
  account and disk encryption (e.g. BitLocker, FileVault). Anyone with access to your user account can read
  them. Database encryption (SQLCipher) and per-file audio encryption with OS-keychain keys are
  planned; tuldMi will not show a password or "encrypted" toggle until that actually protects
  the data.
- Temporary audio buffers live in process memory only; tuldMi does not write unencrypted temp
  files outside its data folder, but memory is not locked against paging to the system page
  file.

## Speaker separation

Off by default. When you switch it on (Settings → Transcription → Speakers):

- **It runs on your computer**, in a separate helper program (`tuldmi-speakers`) that tuldMi
  starts and stops. It's given audio only, never text, and has no network access of its own.
- **Voice fingerprints stay in memory.** To keep one label per person through a conversation,
  tuldMi computes a numeric "embedding" of each voice it hears. That is biometric information
  about other people, so it is never written to disk or logged: it lives only in the app's
  memory, for the current conversation, and is dropped when the conversation ends (3 minutes
  without speech, or midnight), as soon as you switch separation off, or when you quit. After a restart tuldMi
  can't tell that a voice is someone it heard before; it keeps numbering instead.
- **Names you type are stored** with your transcripts (renaming "Speaker 2" to "Dana" saves
  "Dana"). Once you delete all the speech a name labelled, the name is erased within a day
  (the speaker goes back to "Speaker 2").
- **AI assistants** see speaker names only if you let them read the other side of calls
  (*Include the other side (Remote)*); names are part of Remote.
- **Separate past recordings** works only on conversations whose audio you kept, and changes only
  the labels, never the words.

