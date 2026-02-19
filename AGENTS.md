# AGENTS.md

Instructions for coding agents in this repository.

## Purpose

Maintain and extend the v1 Rust agent with small, safe, test-backed changes.

## Required read order (concise)

1. `README.md`
2. `docs/ROADMAP.md`
3. `docs/TASK_BOARD.md`
4. `docs/ARCHITECTURE.md`

Read as needed:
- `docs/SAFETY.md` when touching limits/policies/tools.
- `docs/TESTING.md` when changing behavior/tests.
- `docs/WORKFLOW.md` for change protocol.
- `docs/RUNBOOK.md` for environment/runtime operations.

Historical reference:
- `docs/legacy/2026-02/` (snapshot docs)

## Conflict precedence

1. `docs/ROADMAP.md` for current scope and phase.
2. `docs/ARCHITECTURE.md` for module boundaries and tool contracts.
3. `docs/SAFETY.md` for runtime guardrails and policy blocks.
4. `README.md` and `docs/RUNBOOK.md` for current run/config defaults.

## Execution rules

- Work on the active roadmap phase unless explicitly directed otherwise.
- Default local dev provider is Ollama.
- Keep v1 tool interfaces exact:
  - `search_notes(query: string, limit: u8)`
  - `fetch_url(url: string)`
  - `save_note(title: string, body: string)`
- Enforce typed tool args and reject unknown fields.
- Keep provider-specific behavior inside model/provider layers.
- Avoid out-of-scope v1 features unless requested.
- Prefer small, reviewable changes with matching test updates.

## Change protocol

1. Pick a task from `docs/TASK_BOARD.md`.
2. Confirm it matches `docs/ROADMAP.md`.
3. Implement using `docs/ARCHITECTURE.md` boundaries.
4. Apply `docs/SAFETY.md` constraints.
5. Run checks from `docs/TESTING.md`.
6. Update docs for behavior/interface/policy changes.
7. Update task status in `docs/TASK_BOARD.md`.

## Done criteria

- Code compiles.
- Relevant tests pass.
- No safety policy regressions.
- Docs updated if behavior changed.
- Task board updated.
