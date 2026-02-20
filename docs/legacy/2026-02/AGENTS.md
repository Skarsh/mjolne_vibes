# AGENTS.md

This file defines how AI coding agents should operate in this repository.

## Purpose

Build the v1 Rust agent described in `docs/ROADMAP.md` with a disciplined, test-first, safety-aware workflow.

## Read order (required)

1. `README.md`
2. `docs/ROADMAP.md`
3. `docs/ARCHITECTURE.md`
4. `docs/SAFETY.md`
5. `docs/TESTING.md`
6. `docs/WORKFLOW.md`
7. `docs/TASK_BOARD.md`

If documents conflict:

1. `docs/ROADMAP.md` is the baseline for v1 scope and acceptance intent.
2. `README.md` and `docs/RUNBOOK.md` control current provider/environment defaults.
3. `docs/ARCHITECTURE.md` and `docs/SAFETY.md` control implementation boundaries.

## Execution rules

- Work in the current roadmap phase unless explicitly asked to jump phases.
- Default dev provider is local Ollama unless explicitly overridden.
- Keep tool definitions aligned with v1 scope:
  - `search_notes(query: string, limit: u8)`
  - `fetch_url(url: string)`
  - `save_note(title: string, body: string)`
- Enforce typed inputs and reject unknown fields for tool args.
- Keep model-provider selection configurable (`ollama` and `openai`).
- Avoid provider-specific logic in core loop when an abstraction can isolate it.
- Do not implement out-of-scope v1 features unless explicitly requested.
- Prefer small, reviewable changes with clear test updates.

## Change protocol for agents

1. Select a task from `docs/TASK_BOARD.md`.
2. Confirm task is phase-appropriate in `docs/ROADMAP.md`.
3. Implement changes using boundaries in `docs/ARCHITECTURE.md`.
4. Apply policies in `docs/SAFETY.md`.
5. Validate with checks in `docs/TESTING.md`.
6. Update docs if behavior, interfaces, or policies changed.
7. Mark task status in `docs/TASK_BOARD.md`.

## Done criteria for each merged task

- Code compiles.
- Relevant tests pass.
- No policy regressions for safety/limits.
- Documentation updated where behavior changed.
- Task board status updated.

## Reference docs

- `docs/ROADMAP.md`
- `docs/ARCHITECTURE.md`
- `docs/WORKFLOW.md`
- `docs/TESTING.md`
- `docs/SAFETY.md`
- `docs/RUNBOOK.md`
- `docs/TASK_BOARD.md`
