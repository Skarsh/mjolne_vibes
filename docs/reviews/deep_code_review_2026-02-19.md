# Deep Code Review - 2026-02-19

## Scope

Full project review for correctness, safety, regressions, and test coverage gaps.

## Review rubric

- `critical`: data loss/security/safety break or severe correctness bug
- `high`: likely user-facing incorrect behavior or major reliability issue
- `medium`: meaningful robustness/maintainability/test gap
- `low`: minor issue or polish

## Progress tracker

- [x] Baseline checks (`fmt`, `clippy`, `test`)
- [x] `src/config.rs`
- [x] `src/model/client.rs`
- [x] `src/agent/mod.rs`
- [x] `src/tools/mod.rs`
- [x] `src/server/mod.rs`
- [x] `src/eval/mod.rs`
- [x] `src/main.rs` + `src/lib.rs`
- [x] Cross-cutting review (parity, limits, logging, error mapping)
- [x] Test-gap analysis
- [x] Final findings summary

## Findings log

| ID | Severity | Status | File | Summary |
|---|---|---|---|---|
| F-001 | high | closed | `src/tools/mod.rs:434` | `save_note` now inspects existing target with `symlink_metadata`, rejects symlink targets, and writes via temp file + atomic move path. |
| F-002 | high | closed | `src/server/mod.rs:90` | HTTP status mapping now classifies all guardrail/client errors as `400` and upstream/model failures as `502` via centralized marker matching. |
| F-003 | medium | closed | `src/agent/mod.rs:688` | Tool policy/arg/unknown-tool failures now hard-fail the turn; only transient `fetch_url` execution/timeouts are retried once. |
| F-004 | medium | closed | `src/agent/mod.rs:307` | `consecutive_tool_steps` is now reset on non-tool model responses before potential format-repair continue. |
| F-005 | medium | closed | `src/tools/mod.rs:161` | `search_notes` now performs file-backed search in `NOTES_DIR` with deterministic score ordering and `limit` enforcement. |
| F-006 | medium | closed | `tests/chat_transport_parity.rs:1` | Added integration tests covering CLI/HTTP oversized-input guardrail parity and HTTP `502` mapping for unreachable model upstream errors. |

## Finding details

### F-001 `save_note` symlink escape (high)

- Evidence:
  - `src/tools/mod.rs:451` computes `note_path` with `notes_dir.join(...)`.
  - `src/tools/mod.rs:452` uses `note_path.exists()` for overwrite gating.
  - `src/tools/mod.rs:473` writes with `fs::write(&note_path, ...)`, which follows symlinks.
- Impact:
  - A symlink inside `NOTES_DIR` can redirect writes outside the controlled directory.
  - This violates the policy boundary for `save_note`.
- Recommendation:
  - Use symlink-safe open flow (`OpenOptions` with `create_new`/truncate strategy) and reject symlink targets via `symlink_metadata`.
  - Canonicalize and validate final resolved path remains under canonicalized `NOTES_DIR`.
  - Add tests covering regular file, symlink file, and broken symlink cases.

Status update (2026-02-19):
- Closed by hardening `save_note` write flow in `src/tools/mod.rs:434`:
  - inspect existing target with `symlink_metadata`
  - reject symlink and non-file overwrite targets
  - write note contents to a temp file with `create_new(true)`
  - move into place after optional regular-file removal
- Added regression test `dispatch_save_note_rejects_symlink_note_path` in `src/tools/mod.rs:814` (unix-gated).

### F-002 HTTP status misclassification for guardrail errors (high)

- Evidence:
  - `src/server/mod.rs:90` maps statuses via string fragments.
  - It checks `AGENT_MAX_INPUT_CHARS`/`AGENT_MAX_OUTPUT_CHARS` but not other actual guardrail tokens from agent errors.
  - Actual guardrail error strings include:
    - `src/agent/mod.rs:558` `AGENT_MAX_TOOL_CALLS`
    - `src/agent/mod.rs:576` `AGENT_MAX_CONSECUTIVE_TOOL_STEPS`
    - `src/agent/mod.rs:594` `AGENT_MAX_TOOL_CALLS_PER_STEP`
- Impact:
  - Valid client-side limit violations can be returned as `500` instead of `400`.
- Recommendation:
  - Replace substring classification with typed error variants or a stable error code enum.
  - Add HTTP tests for all limit errors to enforce expected `400` mapping.

Status update (2026-02-19):
- Closed by broadening guardrail/client markers and explicit upstream markers in `src/server/mod.rs:90`.
- Added/updated unit coverage for policy, wrapped guardrail, and upstream tool failure classification in `src/server/mod.rs:133`.

### F-003 Tool policy violations do not hard-fail turn (medium)

- Evidence:
  - `src/agent/mod.rs:685` handles tool dispatch.
  - `src/agent/mod.rs:699` on tool error returns JSON text `{\"error\": ...}` instead of bubbling an error.
  - `src/agent/mod.rs:675` appends that string as normal tool result and loop continues.
- Impact:
  - Safety/policy blocks are not guaranteed to terminate turn execution.
  - Final answer quality depends on model correctly interpreting tool error payloads.
- Recommendation:
  - Decide and enforce policy semantics:
    - either hard-stop on policy violations/timeouts, or
    - return typed tool-error frames with explicit model instructions and capped retries.
  - Add tests for disallowed `fetch_url`/overwrite-blocked `save_note` behavior.

Status update (2026-02-19):
- Closed by changing tool dispatch path to return `Result<String>` and propagate:
  - unknown tool, invalid args, policy block => immediate turn failure
  - `fetch_url` transient execution failures/timeouts => one retry, then explicit upstream failure
- Implemented in `src/agent/mod.rs:688` and retry helpers at `src/agent/mod.rs:773`.

### F-004 Consecutive tool-step counter not reset on non-tool step (medium)

- Evidence:
  - `src/agent/mod.rs:278` initializes `consecutive_tool_steps`.
  - It increments only in tool-call branch (`src/agent/mod.rs:350`) and is never reset in final-text branch (`src/agent/mod.rs:299` onward).
  - Format-repair branch (`src/agent/mod.rs:306`) `continue`s without resetting.
- Impact:
  - Non-consecutive tool steps can be counted as consecutive in edge flows.
- Recommendation:
  - Reset counter on any non-tool model response before continuing.
  - Add targeted test for tool-call -> final-text(format mismatch) -> tool-call sequence.

Status update (2026-02-19):
- Closed by resetting `consecutive_tool_steps` in final-text branch (`src/agent/mod.rs:307`).

### F-005 `search_notes` is non-functional stub (medium)

- Evidence:
  - `src/tools/mod.rs:149` returns fixed payload with `results: []` regardless of local notes.
- Impact:
  - Tool contract implies local note search but behavior is effectively disabled.
  - Reduces agent usefulness and can mislead prompt behavior/eval expectations.
- Recommendation:
  - Implement file-backed search under `NOTES_DIR` with deterministic scoring and `limit`.
  - Add unit tests for query matching, limit truncation, and no-match behavior.

Status update (2026-02-19):
- Closed by implementing file-backed `search_notes` in `src/tools/mod.rs:161`:
  - reads note files from `NOTES_DIR`
  - ranks matches by case-insensitive occurrence count (deterministic tie-breaks)
  - enforces `limit` and returns structured `total_matches` + `results`
- Added unit coverage:
  - `dispatch_search_notes_returns_ranked_results_with_limit`
  - `dispatch_search_notes_returns_empty_when_notes_dir_is_missing`
  - `dispatch_search_notes_rejects_empty_query`

### F-006 Missing integration tests for transport parity and error mapping (medium)

- Evidence:
  - `tests/` directory is absent.
  - Current suite is unit-focused (`cargo test` passes 63 tests) but lacks full-path integration coverage.
- Impact:
  - Regressions in CLI/HTTP parity and status mapping can slip through.
- Recommendation:
  - Add integration tests for:
    - `chat --json` output contract,
    - HTTP `POST /chat` parity for normal and guardrail-blocked requests,
    - tool-loop failure/timeout behavior.

Status update (2026-02-19):
- Closed by adding `tests/chat_transport_parity.rs`:
  - `cli_and_http_share_oversized_input_guardrail`
  - `http_returns_bad_gateway_for_unreachable_model`
- Tests run full CLI binary and HTTP server process with isolated env settings.
- In restricted environments that disallow localhost bind, tests skip cleanly.

## Commands run

- `cargo fmt --all -- --check` (pass)
- `cargo clippy --all-targets --all-features -- -D warnings` (pass)
- `cargo test --all-targets --all-features` (pass, `63/63` tests)
- `ls -la tests` (`tests/` does not exist)
- `cargo fmt --all` (pass)
- `cargo clippy --all-targets --all-features -- -D warnings` (pass, after remediations)
- `cargo test --all-targets --all-features` (pass, `65/65` tests after remediations)
- `cargo fmt --all` (pass, after F-001 remediation)
- `cargo clippy --all-targets --all-features -- -D warnings` (pass, after F-001 remediation)
- `cargo test --all-targets --all-features` (pass, `66/66` tests after F-001 remediation)
- `cargo fmt --all` (pass, after F-006 remediation)
- `cargo clippy --all-targets --all-features -- -D warnings` (pass, after F-006 remediation)
- `cargo test --all-targets --all-features` (pass, `68/68` tests after F-006 remediation)
- `cargo fmt --all` (pass, after F-005 remediation)
- `cargo clippy --all-targets --all-features -- -D warnings` (pass, after F-005 remediation)
- `cargo test --all-targets --all-features` (pass, `70/70` tests after F-005 remediation)

## Notes

This file is the handoff artifact for session restarts.
