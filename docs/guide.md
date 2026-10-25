---
title: tuldMi documentation
---

# tuldMi documentation

tuldMi is a local-first ambient audio memory for Windows. It listens to the microphone and system
audio you allow, keeps only the speech, transcribes it on your computer, and gives you a
searchable, playable timeline of what was said.

## Using tuldMi

- [Install and first run](INSTALL.md): download, verify the signature, and the seven-step setup.
- [Privacy](PRIVACY.md): what is captured, where it is stored, what never leaves your computer,
  and what v0.1 does not protect yet.
- [Models and licenses](MODELS.md): the bundled voice-activity model and the transcription models
  you can download, with sizes, languages and SHA-256 hashes.

## Under the hood

- [Architecture](ARCHITECTURE.md): the capture → speech detection → Opus → transcription →
  search pipeline, threads, time handling and tests.
- [IPC contract](IPC.md): the commands and events between the interface and the engine.
- [Frontend](FRONTEND.md): the React interface, design tokens and motion.

## Elsewhere

- [Releases and downloads](https://github.com/spilloid/tuldMi/releases)
- [Changelog](https://github.com/spilloid/tuldMi/blob/main/CHANGELOG.md)
- [Contributing](https://github.com/spilloid/tuldMi/blob/main/CONTRIBUTING.md)
- [Third-party notices](https://github.com/spilloid/tuldMi/blob/main/THIRD_PARTY_NOTICES.md)
