# Architecture

## System intent

- CLI-first Rust agent with optional HTTP transport.
- Single shared agent loop for one-turn execution paths.
- Strong tool typing, explicit policies, deterministic limits.

## Module map

```text
src/
  main.rs          # CLI entrypoint
  config.rs        # env parsing + defaults
  agent/mod.rs     # orchestration loop + REPL + JSON mode
  model/client.rs  # provider adapters (ollama/openai)
  tools/mod.rs     # tool schemas + dispatch + policy checks
  eval/mod.rs      # eval harness and checks
  graph/mod.rs     # deterministic Rust file/module graph builder
  server/mod.rs    # HTTP transport; delegates to agent loop
  studio/mod.rs    # native egui shell; chat pane + canvas pane
  studio/events.rs # typed UI/runtime command and event channels
```

## Native `studio` status (v0)

Implemented in-repo:

```text
src/
  graph/mod.rs     # deterministic Rust file/module graph builder
  studio/mod.rs    # native egui shell; chat pane + canvas pane
  studio/events.rs # typed UI/runtime command and event channels
```

Still planned:

```text
src/
  studio/canvas.rs # canvas state reducer and rendering helpers
  graph/watch.rs   # filesystem watch + debounced graph refresh
```

Planned contracts:
- `ArchitectureGraph`:
  - `nodes` (stable id + display label + kind + optional path)
  - `edges` (from, to, relation kind)
  - `revision` (monotonic refresh id)
  - `generated_at` (timestamp)
- `CanvasOp`:
  - `SetGraph`
  - `HighlightNodes`
  - `FocusNode`
  - `AddAnnotation`
  - `ClearAnnotations`

Runtime flow (implemented + planned):
1. User sends chat input from `studio`.
2. Shared agent loop (`agent/mod.rs`) executes turn and returns text outcome.
3. `studio/events.rs` carries typed turn and canvas update events.
4. Background graph worker (planned) refreshes architecture graph on:
   - file-watch events (debounced)
   - chat-turn completion
5. `studio` applies typed `CanvasOp` updates and re-renders canvas without blocking chat.

Planned failure handling:
- Canvas or graph refresh failures must not fail chat turns.
- UI thread must stay responsive; heavy work runs off the render thread.
- Missing/invalid canvas ops are ignored with diagnostics, not hard failures.

## Agent loop contract

1. Build request from system prompt + conversation + user input.
2. Call model provider through `model/client.rs`.
3. If tool calls are returned:
   - validate tool name + typed args
   - enforce policy limits
   - execute tools with timeout
   - append tool outputs and continue
4. Stop on final text or guardrail/limit trigger.
5. Return final text + trace metadata.

## v1 tool contracts (fixed)

- `search_notes(query: string, limit: u8)`
- `fetch_url(url: string)`
- `save_note(title: string, body: string)`

## Boundary rules

- `model/client.rs`: provider protocol only; no business/safety policy.
- `tools/mod.rs`: tool-level logic and validation only.
- `agent/mod.rs`: loop control, limits, and step accounting.
- `server/mod.rs`: transport-only; no duplicated loop logic.
- `config.rs`: runtime limits and provider settings source.
- `graph/mod.rs`: deterministic code graphing only; no model/provider coupling.
- `graph/watch.rs` (planned): watch/debounce refresh orchestration only.
- `studio/*`: UI orchestration/presentation only; do not bypass agent/tool safety path.

## Legacy detail

Expanded architecture notes were archived to `docs/legacy/2026-02/ARCHITECTURE.md`.
