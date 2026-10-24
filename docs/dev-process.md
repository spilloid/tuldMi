# tuldMi dev process log

Every unit of work routed to a model other than the orchestrator gets one row here, so routing
stays derived from measurements on this repo. Method: corporate-strategy
`standards/STD-001-adversarial-review.md`; routing table in [`CLAUDE.md`](../CLAUDE.md).

Tiers available (all live-pinged 2026-10-22): Codex `gpt-5.6-luna`, `gpt-5.6-terra`,
`gpt-6.1-sol`, `gpt-6-astra` (review only); Claude Haiku 5.5, Sonnet 5.5, Opus 5.5
(orchestrator: Opus 5.5).

| # | Date | Unit | Implementer | Reviewer | Findings | Real | Rejected (why) | Tokens / time | Verdict / notes |
|---|---|---|---|---|---|---|---|---|---|
| 0 | 2026-10-22 | Orchestration setup: CLAUDE.md, codex dispatcher agents, this log | Opus (orchestrator) | — | — | — | — | ~2k tok per tier ping | Pings only; no code yet |
| 1a | 2026-10-23 | Segmenter, 16 kHz resampler, Ogg Opus encode/decode — 3 files, tests-first contract | Codex `gpt-6.1-sol` high | Claude Opus (cross-family) | — | — | — | 127,621 tok / 945 s | 15/15 contract tests green, re-run by orchestrator. Deviated from contract: `ToMono16k::process/flush` return `Result` — accepted (rubato is fallible). |
| 1b | 2026-10-23 | SQLite store, migrations, FTS5, jobs, retention — 5 files | Codex `gpt-6.1-sol` high | Claude Opus (cross-family) | — | — | — | 89,947 tok / 940 s | 8/8 contract tests green, re-run by orchestrator. Added 9 own tests. Removed the module's contract doc comment (restored in 1c). |
| R1 | 2026-10-24 | Adversarial review of 1a+1b | — | Claude Opus | 13 | 11 (8 confirmed by repro, 3 by inspection) | 2 deferred (Low: `.part` reuse, filter-only search scan) | 141,320 tok / 1,724 s | Most valuable: **retention deleted never-transcribed audio** — a defect in the orchestrator's *spec*, not the code (`failed` counted as finished). Also: crash-loop on recovery (no attempts cap), punctuation searches matched nothing, silence-only tail segments after forced split (299/400 randomized trials), deleted text recoverable from db/WAL. Test grade B−: hostile-search test asserted "no error" but never correctness, which is exactly where #3 hid. |
