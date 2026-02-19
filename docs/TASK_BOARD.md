# Task Board

Current execution board for maintenance work.

## Current focus

- `Maintenance / Hardening`

## Active tasks

- [ ] No active implementation task selected.

## Backlog candidates

- [ ] Extract shared test setup utilities for temp paths and common env setup (review finding 7).
- [ ] Expand tool metadata contract assertions beyond `additionalProperties = false` (review finding 8).
- [ ] Add optional cost/usage counters in turn trace output.

## Recently completed

- [x] v1 phases 0-5 completed (see `docs/legacy/2026-02/ROADMAP.md` for full history).
- [x] Documentation consolidation and archival into `docs/legacy/2026-02/` (2026-02-19).
- [x] Added transport parity integration tests for CLI/HTTP guardrail + upstream error behavior (2026-02-19).
- [x] Implemented file-backed `search_notes` behavior with ranking and tests (2026-02-19).
- [x] Replaced HTTP substring-based error classification with typed turn error kinds to preserve status mapping robustness (2026-02-19).
- [x] Centralized tool metadata (signature/description/schema) in `tools` and removed duplicated agent definitions (2026-02-19).
- [x] Unified answer-format validation logic across runtime and eval via shared module (2026-02-19).
- [x] Deduplicated provider tool-call parsing/request conversion internals in `src/model/client.rs` and added focused adapter parsing tests (2026-02-19).
- [x] Consolidated repeated runtime settings logging blocks in `src/agent/mod.rs` via shared helper (2026-02-19).
- [x] Added and adopted `ToolDispatchError` constructor helpers in `src/tools/mod.rs` to reduce repetitive error construction (2026-02-19).
- [x] Added optional safe redirect-following for `fetch_url`, restricted to allowlisted hosts (`FETCH_URL_FOLLOW_REDIRECTS`) (2026-02-19).

## Board rules

- Keep only active/backlog items here.
- Move detailed historical logs to `docs/legacy/`.
