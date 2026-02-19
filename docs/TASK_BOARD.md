# Task Board

Current execution board for maintenance work.

## Current focus

- `Maintenance / Hardening`
- `Native Studio Canvas Prototype (v0)`

## Active tasks

- (none)

## Backlog candidates

- [ ] Add integration tests for studio event flow and non-blocking refresh behavior.
- [ ] Add optional cost/usage counters in turn trace output.

## Recently completed

- [x] Polished native `studio` UI with themed card layout, improved chat/canvas readability, and cleaner graph legend/hover presentation (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-19).
- [x] Added read-only canvas graph rendering (node/edge visuals) with changed/impact styling in `src/studio/canvas.rs` and wired it into the studio canvas pane (2026-02-19).
- [x] Added changed-node highlight with optional 1-hop impact overlay toggle and annotations in `studio` graph refresh flow (`src/studio/mod.rs`) with diff/overlay tests (2026-02-19).
- [x] Implemented canvas operation reducer (`SetGraph`, `HighlightNodes`, `FocusNode`, `AddAnnotation`, `ClearAnnotations`) in `src/studio/canvas.rs`, wired into studio state updates with reducer tests (2026-02-19).
- [x] Added debounced filesystem watcher and turn-completion-triggered graph refresh (`src/graph/watch.rs`), wired into `studio` canvas updates (2026-02-19).
- [x] Implemented deterministic Rust file/module graph builder (`src/graph/mod.rs`) with stable node/edge contracts and deterministic ordering tests (2026-02-19).
- [x] Implemented native `studio` command shell with chat pane + canvas pane (`egui`) and wired `cargo run -- studio` CLI command (2026-02-19).
- [x] Added typed `studio` event channel contract for chat turn results and canvas updates (`src/studio/events.rs`) (2026-02-19).
- [x] Defined native `studio` + canvas v0 roadmap/architecture baseline and constraints (2026-02-19).
- [x] Expanded tool metadata contract assertions to validate v1 tool names, descriptions, and full parameter schemas (review finding 8) (2026-02-19).
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
- [x] Extracted shared test support utilities for temp paths/env setup and reused them across eval/tools/integration tests (2026-02-19).
- [x] Added optional safe redirect-following for `fetch_url`, restricted to allowlisted hosts (`FETCH_URL_FOLLOW_REDIRECTS`) (2026-02-19).

## Board rules

- Keep only active/backlog items here.
- Move detailed historical logs to `docs/legacy/`.
