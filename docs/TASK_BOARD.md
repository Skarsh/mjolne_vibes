# Task Board

Current execution board for maintenance work.

## Current focus

- `Maintenance / Hardening`
- `Native Studio Canvas Prototype (v0)`
- `Studio Canvas Genericization (multi-session track)`

## Active tasks

- [ ] [SG-07] Evolve `CanvasOp`/canvas state toward generic drawing-intent primitives while preserving compatibility with current graph operations during transition (`src/studio/events.rs`, `src/studio/canvas.rs`).
- [ ] [SG-08] Keep graph watch + diff/highlight logic isolated from generic canvas shell behavior; preserve non-blocking chat turn behavior and failure isolation (`src/graph/watch.rs`, `src/studio/mod.rs`).
- [ ] [SG-09] Add/adjust reducer and rendering tests for the new generic-vs-graph boundaries and toolbar/inspector behavior (`src/studio/canvas.rs`, `src/studio/mod.rs` tests).
- [ ] [SG-10] Add integration coverage for studio event flow and non-blocking refresh behavior under active turns (promoted from backlog) (`tests/` and/or `src/studio/*` integration harness).
- [ ] [SG-11] Update docs to reflect the generic canvas contract and surface adapter model (`docs/ARCHITECTURE.md`, `README.md`).
- [ ] [SG-12] Final hardening pass: run full quality gates, polish copy/labels to generic canvas language, and move completed SG tasks to history (`docs/TASK_BOARD.md`, `docs/TESTING.md` commands).

## Backlog candidates

- [ ] Add optional cost/usage counters in turn trace output.

## Recently completed

- [x] [SG-06] Introduced a canvas surface adapter path so graph rendering is dispatched as one surface implementation (`CanvasSurfaceAdapter`/`GraphSurfaceAdapterOptions`) rather than direct canvas-core coupling (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-20).
- [x] [SG-05] Refactored canvas rendering boundaries by extracting reusable canvas frame/layout + viewport input handling from graph-specific drawing, and added focused layout boundary tests (`src/studio/canvas.rs`) (2026-02-20).
- [x] [SG-04] Added an opt-in graph inspector card with graph-specific telemetry (revision, node/edge counts, changed/impact counts, refresh trigger) and kept it hidden by default behind graph options (`src/studio/mod.rs`) (2026-02-20).
- [x] [SG-03] Kept the primary canvas toolbar viewport-only and moved graph-specific visual affordances behind graph options (including opt-in legend/hover hint rendering) (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-20).
- [x] [SG-02] Minimized studio header chrome to navigation/session essentials (chat toggle + session status), removed top-row model/title metadata, and reduced header visual weight (`src/studio/mod.rs`) (2026-02-20).
- [x] [SG-01] Baseline canvas cleanup: removed graph-heavy metadata chips from the top canvas row, simplified canvas status copy, and moved impact toggle into collapsed graph options to keep default canvas view generic (`src/studio/mod.rs`) (2026-02-20).
- [x] Advanced `studio` into an interaction-first canvas shell with collapsible chat rail, pan/zoom/fit controls, and tool-call cards rendered directly on canvas for agent-driven workflows (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-19).
- [x] Reworked `studio` into a cleaner canvas-first layout with a simplified chat sidebar, dynamic full-height canvas surface rendering, and extensible canvas-surface dispatch for future non-graph views (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-19).
- [x] Refreshed native `studio` visual design with a lighter branded theme, stronger status/metadata chips, updated chat/canvas card hierarchy, and harmonized graph palette styling (`src/studio/mod.rs`, `src/studio/canvas.rs`) (2026-02-19).
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
