# Roadmap

Execution roadmap for the current codebase.

## Current status

- v1 scope is complete.
- Active phase: `Maintenance / Hardening`.
- Approved prototype direction (user-directed): native `studio` UI with optional canvas-driven architecture visualization.

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

## Approved prototype track: Native `studio` + canvas (v0)

Goals:
- Add a native desktop `studio` command with chat + canvas panes.
- Support file/module-level architecture visualization for Rust workspaces.
- Auto-refresh visualization on file changes and at chat-turn completion.
- Allow the agent runtime to emit typed canvas update intents alongside text.

Constraints:
- Keep v1 tool contracts unchanged unless explicitly requested:
  - `search_notes(query: string, limit: u8)`
  - `fetch_url(url: string)`
  - `save_note(title: string, body: string)`
- Keep policy enforcement in the shared agent/tool path.
- Keep existing `chat`, `chat --json`, `repl`, `eval`, and `serve` behavior stable.

Deferred beyond v0:
- Per-turn timeline/snapshot scrubber.
- Voice input/output.
- Fine-grained (function-level) architecture graphing.

## Change constraints

- Keep tool contracts unchanged unless explicitly requested.
- Keep policy enforcement in the shared loop/tool path.
- Prefer small, reviewable tasks from `docs/TASK_BOARD.md`.

## Legacy detail

Detailed phase-by-phase historical notes were archived to `docs/legacy/2026-02/ROADMAP.md`.
