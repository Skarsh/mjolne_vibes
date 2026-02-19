# Deep Code Review - Simplification and Unification (2026-02-19)

## Context

This document captures a deep maintenance review focused on:
- Simplifying complex paths.
- De-duplicating logic and metadata.
- Unifying behavior across CLI, eval, and HTTP transports.

Date: 2026-02-19
Repository: `mjolne_vibes`

## Scope Reviewed

- `src/agent/mod.rs`
- `src/model/client.rs`
- `src/tools/mod.rs`
- `src/server/mod.rs`
- `src/eval/mod.rs`
- `src/config.rs`
- `src/main.rs`
- `tests/chat_transport_parity.rs`
- Core docs (`README.md`, `docs/ROADMAP.md`, `docs/TASK_BOARD.md`, `docs/ARCHITECTURE.md`, `docs/SAFETY.md`, `docs/TESTING.md`, `docs/WORKFLOW.md`)

## Baseline Verification

All quality gates passed during review:
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`

## Execution Status Update (2026-02-19)

This section tracks what was implemented after this review was written.

- [x] Finding 1 completed: replaced string-based HTTP error classification with typed turn error kinds.
- [x] Finding 2 completed: centralized tool metadata and removed duplicated agent definitions.
- [x] Finding 3 completed: unified answer-format validation logic across runtime and eval.
- [x] Finding 4 completed: deduplicated provider conversion/parsing internals in `src/model/client.rs` and added focused adapter parsing tests.
- [x] Finding 5 completed: consolidated repeated runtime settings logging blocks in `src/agent/mod.rs` via shared helper.
- [x] Finding 6 completed: added helper constructors for `ToolDispatchError` and applied them across runtime tool paths in `src/tools/mod.rs`.
- [x] Finding 7 completed: extracted shared test support utilities for temp paths and common command env setup and reused them across eval/tools/integration tests.
- [ ] Finding 8 open: strengthen tool metadata contract test assertions.

Status source: `docs/TASK_BOARD.md` recently completed items and current code in `src/`.
Note: line references in the findings below are snapshot references from review time and may drift.

## Findings (Prioritized)

### 1. High: HTTP status classification depends on string matching

Current behavior in `src/server/mod.rs:90` classifies errors by scanning error text substrings from agent/tool paths.

Why this is risky:
- Classification silently depends on wording in other modules.
- Refactoring error messages can unintentionally change API status behavior (`400` vs `502` vs `500`).
- This weakens transport parity guarantees.

Examples of coupled message sources:
- `src/agent/mod.rs:290`
- `src/agent/mod.rs:561`
- `src/agent/mod.rs:735`

Recommendation:
- Introduce typed error categories for turn execution (for example: validation/policy/upstream/internal).
- Return typed category from agent path.
- Map category to HTTP status in server without substring parsing.

### 2. Medium: Tool contract metadata is duplicated across paths

Tool identity is split across:
- Registry names in `src/tools/mod.rs:23`
- REPL signatures in `src/agent/mod.rs:526`
- Descriptions in `src/agent/mod.rs:790`
- JSON schemas in `src/agent/mod.rs:799`

Why this is risky:
- Multiple edit points per tool.
- Drift risk between schema/description/signature and actual runtime contract.

Recommendation:
- Make `ToolDefinition` the single metadata source for name, signature, description, and schema.
- Build both REPL output and model tool definitions directly from this shared metadata.

### 3. Medium: Answer-format validation logic is duplicated

Same concept appears in two places:
- Runtime format checks in `src/agent/mod.rs:426`
- Eval format checks in `src/eval/mod.rs:341`

Why this is risky:
- Behavior can diverge between runtime and eval over time.
- Future format support requires parallel edits and tests.

Recommendation:
- Extract shared format validation helper(s) used by both runtime and eval.

### 4. Medium: Provider code paths have avoidable duplication

There are near-parallel conversion blocks in model client:
- Tool call parsing: `src/model/client.rs:452` and `src/model/client.rs:475`
- Request conversion: `src/model/client.rs:595` and `src/model/client.rs:723`

Why this matters:
- Larger surface area for bugs and inconsistencies.
- Slower maintenance for provider protocol updates.

Recommendation:
- Extract provider-agnostic conversion helpers where possible.
- Keep only unavoidable provider-specific shape handling local.

### 5. Low: Repeated runtime settings logging blocks

Large repeated logs in:
- `src/agent/mod.rs:77`
- `src/agent/mod.rs:104`
- `src/agent/mod.rs:138`

Recommendation:
- Factor to shared `log_runtime_settings(event_name, settings)` helper.

### 6. Low: `ToolDispatchError` construction is repetitive

Many repeated `tool_name.to_owned()` + `reason` formatting patterns across `src/tools/mod.rs` (for example `:164`, `:378`, `:622`, `:649`).

Recommendation:
- Add small constructors/helpers on `ToolDispatchError` for consistent, compact error creation.

### 7. Low: Test setup helpers are duplicated

Patterns appear in several places:
- Integration env setup: `tests/chat_transport_parity.rs:168`
- Temp directory helpers in both eval and tools tests.

Recommendation:
- Extract shared test utilities module for temp paths and common env setup.

### 8. Low: Contract tests for tool metadata can be stricter

Current contract test in `src/agent/mod.rs:853` checks names and `additionalProperties=false` but does not fully assert required fields and schema contract details.

Recommendation:
- Add stronger schema assertions once metadata is centralized.

## Refactor Plan (Suggested Order)

### Phase 1 (highest impact, lowest contract risk)

1. Replace string-based error classification with typed categories.
2. Keep existing HTTP behavior (`400`/`502`/`500`) unchanged.
3. Add explicit tests for category-to-status mapping.

### Phase 2

1. Centralize tool metadata in one module/structure.
2. Rewire REPL tool listing and model tool definition building to that source.
3. Add regression tests to lock tool signature/description/schema outputs.

### Phase 3

1. Extract shared answer-format validators.
2. Use shared validators in both agent runtime and eval.
3. Add parity tests proving runtime and eval enforce identical format rules.

### Phase 4

1. Deduplicate provider conversion/parsing internals in `model/client.rs`.
2. Preserve exact external provider behavior.
3. Add focused adapter tests around tool-call parsing and content extraction.

### Phase 5 (cleanup)

1. Consolidate repeated logging helpers.
2. Consolidate test setup helpers.
3. Add error-constructor helpers in tools module.

## Follow-up Checklist (Open)

- [x] Finding 4: Deduplicate provider conversion/parsing internals and add focused adapter tests.
- [x] Finding 5: Consolidate repeated runtime settings logs into a shared helper.
- [x] Finding 6: Add helper constructors for `ToolDispatchError` and apply them in tools paths.
- [x] Finding 7: Extract shared test utility helpers for temp paths and environment setup.
- [x] Finding 8: Expand contract tests for tool metadata beyond `additionalProperties = false`.

## Guardrails for Implementation

- Do not change v1 tool interfaces:
  - `search_notes(query: string, limit: u8)`
  - `fetch_url(url: string)`
  - `save_note(title: string, body: string)`
- Keep unknown-field rejection in tool args.
- Keep policy enforcement in shared agent/tool path.
- Preserve transport parity across CLI/eval/HTTP.
- Keep provider-specific behavior inside model/provider layer.

## Handoff Checklist

For each refactor phase:
- Compile and run tests.
- Run quality gates from `docs/TESTING.md`.
- Confirm no safety-policy regressions (`docs/SAFETY.md`).
- Update `docs/TASK_BOARD.md` with selected active task and status.
- Update docs if externally visible behavior changes.

## Notes

- This review intentionally stays within maintenance/hardening scope (`docs/ROADMAP.md`).
- No code behavior changes were applied as part of this review document creation.
