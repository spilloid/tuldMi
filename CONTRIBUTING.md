# Contributing to tuldMi

Thanks for helping. tuldMi handles people's conversations, so two rules outrank everything else:

1. **Nothing fake.** A control works, or it's visibly disabled and disclosed. Meters show real
   signal. Demo data is always labelled as demo data.
2. **Nothing leaves the device** except a model download the user explicitly asked for. No
   telemetry, ever. Never log transcript text, search queries, or audio.

## Setup (Windows)

- Rust stable, **MSVC toolchain** (`rustup default stable-x86_64-pc-windows-msvc`; the repo pins it
  in `rust-toolchain.toml`)
- Visual Studio Build Tools with the "Desktop development with C++" workload (MSVC + CMake)
- LLVM (for `libclang`; bindgen needs it to build whisper.cpp bindings). Default install path is
  set in `.cargo/config.toml`; change `LIBCLANG_PATH` if yours differs.
- Node.js 22+ and npm

```
npm install
npm run tauri dev        # app with hot-reloading frontend
npm run dev              # frontend only, in a browser, on labelled demo data (?demo)
```

## Checks before a PR

```
cargo test -p tuldmi-engine
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck && npx vitest run && npm run build
```

Engine tests that need real Whisper weights are opt-in:
`TULDMI_TEST_MODEL=path\to\ggml-tiny.en.bin cargo test -p tuldmi-engine --test engine -- --ignored`.

UI changes: include before/after screenshots (both themes). Audio changes: say how you verified
on real devices — unit tests can't catch device-lifetime bugs.

## Layout

- `crates/engine` — capture, VAD, segmentation, Opus, SQLite, transcription. No UI dependencies.
- `src-tauri` — the desktop shell (tray, window, commands).
- `src` — the React frontend.
- `docs/` — architecture, IPC contract, privacy, models, release process.

## Schema changes

Add a new `crates/engine/src/storage/migrations/NNNN_*.sql`; never edit a shipped migration.

## License

By contributing you agree your contribution is licensed under the MIT License (see `LICENSE`).
