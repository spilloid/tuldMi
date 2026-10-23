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
