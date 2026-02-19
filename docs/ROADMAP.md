# Roadmap

Execution roadmap for the current codebase.

## Current status

- v1 scope is complete.
- Active phase: `Maintenance / Hardening`.

## v1 scope (implemented)

- CLI modes: `chat`, `chat --json`, `repl`, `eval`, `serve`.
- Shared one-turn loop across CLI/eval/HTTP.
- Provider abstraction: `ollama` (default) and `openai`.
- v1 typed tools with strict arg validation.
- Safety guardrails and loop/tool budgets.
- Eval harness with case-driven pass/fail checks.

## Maintenance priorities

1. Keep docs, tests, and behavior in sync.
2. Preserve transport parity (CLI/eval/HTTP).
3. Improve reliability/observability without expanding v1 scope by default.

## Change constraints

- Keep tool contracts unchanged unless explicitly requested.
- Keep policy enforcement in the shared loop/tool path.
- Prefer small, reviewable tasks from `docs/TASK_BOARD.md`.

## Legacy detail

Detailed phase-by-phase historical notes were archived to `docs/legacy/2026-02/ROADMAP.md`.
