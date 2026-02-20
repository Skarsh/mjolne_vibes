# Roadmap

Execution roadmap for the current codebase.

## Current status

- v1 scope is complete.
- Active phase: `Maintenance / Hardening`.
- Approved prototype direction (user-directed): native `studio` UI evolving into a generic draw-command canvas platform.

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

## Approved prototype track: Native `studio` + draw-command canvas platform

Goals:
- Add a native desktop `studio` command with chat + canvas panes.
- Establish a complete generic canvas command model (scene + draw ops) decoupled from graph-specific state.
- Support higher-level renderers that compile domain state into generic draw commands.
- Deliver architecture/workflow renderer first so users can understand agent changes and implementation flow faster than raw diffs.
- Auto-refresh renderer outputs on file changes and at chat-turn completion.
- Allow the agent runtime to emit typed canvas update intents alongside text.

Execution plan:
1. Define stable draw-command contract:
   - Introduce typed scene primitives and mutations (create/update/delete/order/style/annotation).
   - Keep unknown-field rejection and deterministic ordering guarantees.
2. Build canvas command runtime in `studio`:
   - Extend reducer/state to apply generic draw commands directly.
   - Keep viewport/input/render loop independent from any specific renderer.
3. Add renderer pipeline:
   - Add architecture overview renderer that translates graph + change events into draw commands.
   - Preserve graph refresh isolation and bounded per-frame update draining.
4. Surface agent-work context:
   - Project turn/tool activity into canvas annotations/cards/lanes as draw commands.
   - Keep this as renderer output, not special-cased canvas-core logic.
5. Hardening:
   - Add reducer invariants, renderer translation tests, and studio integration tests for event flow.
   - Keep CLI/eval/HTTP behavior stable and safety policies unchanged.

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
