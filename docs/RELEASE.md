# Releasing tuldMi

## Gate (all must hold before tagging)

1. `cargo test -p tuldmi-engine` green; the opt-in real-model test green with
   `TULDMI_TEST_MODEL` set to `ggml-tiny.en.bin`.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `npm run typecheck`, `npx vitest run`, `npm run build` clean.
4. Live pipeline check on real hardware (not just tests): install the built NSIS installer on
   Windows, complete onboarding, speak into the mic and play audio through speakers, confirm
   both appear as Me/Remote events, are transcribed, are searchable, and play back; pause from
   the tray and the shortcut; close and reopen the window; quit and relaunch — history intact.
5. Screenshots refreshed in `docs/screenshots/` (light + dark).
6. `CHANGELOG.md` has the version's section; README limitations match reality.
7. A release-gate adversarial review (Codex Astra, review-only) of the diff since the last tag,
   findings adjudicated in `docs/dev-process.md`.

## Build

```
npm ci
npm run tauri build
```

Output: `target/release/bundle/nsis/tuldMi_<version>_x64-setup.exe` (per-user install, no admin).

## Signing

v0.1.0 ships **unsigned**; Windows SmartScreen will warn. Spillers Technology's signing path is
Azure Artifact Signing (corporate-strategy D-0047) — wiring it into a release workflow is a
v0.1.x item. Do not describe a build as signed until `Get-AuthenticodeSignature` on the
published asset says `Valid`.

## Publish

Only with the operator's explicit go-ahead, to `github.com/spilloid/tuldMi`:

```
git tag -a v<version> -m "tuldMi v<version>"
git push origin v<version>
gh release create v<version> --prerelease --title "tuldMi v<version>" --notes-file <notes.md> <installer.exe> <installer.exe.sha256>
```

Publish as a prerelease, download the public asset, verify its SHA-256 and that it installs and
launches, then promote.
