# tuldMi — guidance for AI agents

tuldMi is a local-first ambient audio memory app (Tauri 2 + Rust + React/TS). The product spec is
[`docs/MISSION.md`](docs/MISSION.md) — read it before any unit of work. It is a Spillers Technology
product line; company memory lives in `../corporate-strategy` (see `state/products/tuldMi.md`).
Sibling product: `../rsRewind` (screen memory, Rust) — keep schemas integration-friendly, but no
code dependency on it.

## Product rules that override convenience

- **No fake functionality.** A control either works or is visibly disabled and disclosed in the
  release notes. Demo data is always labelled as demo data in the UI.
- **Real signals only.** Meters and waveforms are driven by actual audio levels, never random.
- **Privacy is a requirement.** No telemetry, no network calls except user-consented model
  downloads. Never log transcript text. Obvious recording state at all times.
- **Visual polish is part of the MVP.** Compiling is not proof of good UX — screenshot and look.
- Don't claim speaker diarization beyond Me (mic) / Remote (system audio) unless a model did it.

## Roles

Claude (the orchestrator session) owns architecture, contracts/tests, the final diff, and all
validation. Implementers and reviewers are consultants. Never accept a change because a bigger
model wrote it — only because it was run and held up. Method: corporate-strategy
`standards/STD-001-adversarial-review.md`.

## Routing — two model families, matched to the work

| Work | Implementer | Reviewer |
|---|---|---|
| Mechanical: scaffolding, config, docs, boilerplate mirroring an existing pattern | `codex-luna` or Claude **Haiku** | Terra / Sonnet |
| Default feature work: React components, settings screens, Tauri commands, search UI | Claude **Sonnet** (frontend) / `codex-terra` (Rust) | the *other* family |
| Design system, motion language, timeline/waveform canvas rendering | Claude **Opus** | Terra/high or Sonnet + screenshot inspection |
| Audio capture (WASAPI, `unsafe`), threading/backpressure, VAD buffers, SQLite migrations, inference integration, privacy-relevant code | `codex-sol` (**gpt-6.1-sol**, high) or Claude **Opus** | the other family at the top tier |
| Release-gate / whole-pipeline adversarial review | — | `codex-astra` (**gpt-6-astra**, ultra) — scarce, review only |

- **Cross-family review by default:** Claude-written code goes to Codex for review and vice versa.
- Claude tiers are dispatched with the Agent tool's `model` override (`haiku`, `sonnet`, `opus`).
- Codex tiers go through `.claude/agents/codex-*.md` dispatchers.
- Sol/ultra **as implementer** needs the operator's explicit in-session approval.
- Parallel implementers work in separate git worktrees; stage explicit paths, never `git add -A`.

## Invoking Codex directly

```
cat brief.md | codex exec -m <model> -c model_reasoning_effort=<low|medium|high|xhigh|max|ultra> \
  -s read-only --skip-git-repo-check --color never -C "<dir>" -o out.md "<short instructions>"
```

Models: `gpt-5.6-luna`, `gpt-5.6-terra`, `gpt-6.1-sol`, `gpt-6-astra`. Reviewers `-s read-only`;
implementers `--approve-for-me` (implies workspace-write — don't also pass `-s`). Pipe bulky
content on stdin; inlining past ~100KB fails on Windows.

## Standing review rules (STD-001)

- A finding can be right about the problem and wrong about the fix — adjudicate, choose the fix.
- Re-read diffs after a fix round.
- When a reviewer indicts the spec, believe it.
- Review reads diffs; lifetime, device-change, startup/shutdown and cross-run bugs need live runs.
- Verify before trusting, both directions. A green suite is evidence about the tests — have
  reviewers grade the tests.
- A dispatcher's report is a claim, not proof. Check `git status`/exit codes yourself.
- A sandboxed run can fake a "pre-existing failure" — confirm unsandboxed.
- Log every round in [`docs/dev-process.md`](docs/dev-process.md): tiers, defects found vs real,
  tokens/time.

## Conventions

- Commits: no AI attribution lines. Conventional-ish prefixes (`feat:`, `fix:`, `docs:`).
- Never push or publish a release without the operator's explicit go-ahead.
