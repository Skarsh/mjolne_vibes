# Roadmap

Execution roadmap for v1. This is the working implementation sequence.

## Current phase

- Active phase: `Phase 3 - Guardrails + Safety`

## Phase 0: Skeleton + Config

Goals:

- Add config loader with defaults.
- Add structured logging initialization.
- Define `AgentSettings` (`model`, `max_steps`, timeouts).
- Add provider selection config (`MODEL_PROVIDER`: `ollama` or `openai`), defaulting to `ollama`.
- Add CLI command: `chat "<message>"`.

Acceptance criteria:

- `cargo run -- chat "hello"` loads config and returns placeholder output.

Status:

- [x] Completed (2026-02-18)

## Phase 1: Basic Model Chat (No tools)

Goals:

- Ensure local Ollama dev runtime is set up (`ollama serve` + pulled model) for manual validation.
- Implement model client wrapper.
- Implement Ollama chat path first for local development.
- Add OpenAI path as fallback using same client abstraction.
- Send system and user messages to model and print response.
- Add retry/backoff for transient failures.
- Add total request timeout.

Acceptance criteria:

- Stable plain-prompt answers across 10 manual runs.
- Works with local Ollama profile.
- Actionable logs for failures.

Status:

- [x] Completed (2026-02-18)

Implementation notes:

- Phase 1 validation depends on local Ollama setup and reachable `OLLAMA_BASE_URL`.
- Model client abstraction and both provider paths are implemented.
- Retry/backoff and total request timeout are implemented.
- Manual 10-run local stability validation completed on local Ollama with `10/10` successful runs (2026-02-18).

## Phase 2: Tool Calling Loop

Goals:

- Define schemas for `search_notes`, `fetch_url`, `save_note`.
- Implement tool registry and dispatcher.
- Implement iterative loop until final output.
- Add per-tool timeout and max tool-call count per turn.

Acceptance criteria:

- Tool-requiring prompt succeeds end-to-end.
- Invalid args are rejected with clear errors.

Status:

- [x] Completed (2026-02-18)

Implementation notes:

- Tool schema types for `search_notes`, `fetch_url`, and `save_note` are implemented in `src/tools/mod.rs` with `serde(deny_unknown_fields)` and unit tests (2026-02-18).
- Tool registry and dispatcher are implemented in `src/tools/mod.rs` (`tool_definitions`, `dispatch_tool_call`) with structured unknown-tool/invalid-arg errors and stubbed phase-2 tool payloads (2026-02-18).
- Iterative tool-call loop is implemented in `src/agent/mod.rs`, including model tool-call handling and tool output feedback messages until final response or `max_steps` (2026-02-18).
- Per-tool timeout handling and per-turn tool-call cap are implemented in `src/agent/mod.rs` (timeout-wrapped dispatch + `AGENT_MAX_TOOL_CALLS` enforcement with explicit limit errors) and configurable through `src/config.rs` (2026-02-18).

## Phase 3: Guardrails + Safety

Goals:

- Add a minimal interactive CLI REPL mode for faster manual validation.
- Add URL/domain allowlist for `fetch_url`.
- Add confirmation gate for sensitive writes via `save_note`.
- Add input/output length limits.
- Add `max_steps` and loop protections.

Acceptance criteria:

- Disallowed operations are refused with explicit reason.
- Runaway loop behavior is prevented.

Status:

- [ ] In progress

Implementation notes:

- Interactive REPL mode is implemented via `cargo run -- repl` with multi-turn session history and slash commands (`/help`, `/reset`, `/exit`) in `src/main.rs` and `src/agent/mod.rs` (2026-02-18).
- REPL terminal logging is quiet by default (`warn`+), with opt-in verbose terminal logging via `cargo run -- repl --verbose`; logs are also written to rolling files under `logs/` (2026-02-18).

## Phase 4: Observability + Evaluations

Goals:

- Emit trace logs for step count, tools used, and latency.
- Add evaluation harness driven by `eval/cases.yaml`.
- Track pass/fail criteria:
  - required tool usage
  - no invented tool output
  - correct answer format

Acceptance criteria:

- >= 80% pass rate over initial 20-30 cases.
- Failures diagnosable from logs.

Status:

- [ ] Not started

## Phase 5: Packaging + Next Step

Goals:

- Finalize runbook and safety notes in docs.
- Add `--json` output mode.
- Optionally add HTTP endpoint (`axum`) reusing agent loop.

Acceptance criteria:

- New-machine setup from docs in <15 minutes.
- CLI and HTTP path (if enabled) behave equivalently.

Status:

- [ ] Not started

## Notes

- v1 definition of done remains aligned to `AI_AGENT_RUST_PLAN.md`.
- Optional Phase 2 upgrades are tracked after v1 completion.
