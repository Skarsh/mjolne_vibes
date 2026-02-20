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
  graph/watch.rs   # debounced graph refresh worker + turn-completion trigger handling
  server/mod.rs    # HTTP transport; delegates to agent loop
  studio/mod.rs    # native egui shell; chat pane + canvas pane
  studio/canvas.rs # canvas state reducer + generic canvas frame/viewport primitives + draw-command rendering
  studio/renderer.rs # renderer translation layer (domain state -> canvas draw-command batches)
  studio/events.rs # typed UI/runtime command and event channels
```

## Native `studio` status (v0)

Implemented in-repo:

```text
src/
  graph/mod.rs     # deterministic Rust file/module graph builder
  graph/watch.rs   # debounced graph refresh worker + turn-completion trigger handling
  studio/mod.rs    # native egui shell; collapsible chat rail + generic-first canvas stage
  studio/canvas.rs # canvas reducer + generic canvas surface shell (frame/viewport) + draw-command rendering
  studio/renderer.rs # architecture overview renderer translating graph/change context to draw commands
  studio/events.rs # typed UI/runtime command and event channels
```

Canvas operation contract:
- `ArchitectureGraph`:
  - `nodes` (stable id + display label + kind + optional path)
  - `edges` (from, to, relation kind)
  - `revision` (monotonic refresh id)
  - `generated_at` (timestamp)
- `CanvasOp`:
  - `SetSceneData` (`CanvasSceneData::ArchitectureGraph` in v0)
  - `SetHighlightedTargets`
  - `SetFocusedTarget`
  - `UpsertAnnotation`
  - `ClearAnnotations`
  - Legacy transition aliases still accepted:
    - `SetGraph`
    - `HighlightNodes`
    - `FocusNode`
    - `AddAnnotation`

Canvas surface adapter contract:
- `CanvasSurfaceAdapterKind::ArchitectureGraph`
- `CanvasSurfaceAdapter::ArchitectureGraph { GraphSurfaceAdapterOptions }`
- `CanvasSurfaceAdapter::render(...)` is the surface dispatch point used by `studio/mod.rs`.

Planned draw-command contract direction:
- Canvas core owns a renderer-agnostic scene model and typed draw mutations.
- Renderer modules translate domain models into draw commands (canvas core does not interpret domain-specific semantics).
- Initial draw primitives/mutations:
  - `UpsertShape` (rect/ellipse/line/path/text with style)
  - `UpsertConnector` (typed endpoints and stroke style)
  - `UpsertGroup` (hierarchy/layer placement)
  - `UpsertAnnotation` (reused for callouts/status chips)
  - `DeleteObject` / `ClearScene` / `SetViewportHint`
- Contract is defined in `src/studio/events.rs` via:
  - `CanvasDrawCommandBatch`
  - `CanvasDrawCommand`
  - `CanvasShapeObject` / `CanvasConnectorObject` / `CanvasGroupObject` / `CanvasViewportHint`
- `CanvasState` in `src/studio/canvas.rs` now applies batch updates via `CanvasOp::ApplyDrawCommandBatch` into draw-scene storage with:
  - stable-id upsert/delete semantics across object types
  - stale-sequence rejection (`sequence` must be monotonic)
  - deterministic object ordering for rendering (`layer` + object type + id)
- Contract requirements:
  - typed payloads only
  - unknown-field rejection
  - deterministic render order and stable object ids
  - bounded command batch size per frame to protect UI responsiveness

Renderer pipeline direction:
- `ArchitectureOverviewRenderer` becomes a first-class adapter translating:
  - graph snapshots
  - graph deltas
  into generic draw commands.
  - current output focuses on lane labels plus module/file node + connector scene content.
- Future renderers (timeline, task map, note clusters) should plug into the same pipeline.

Runtime flow (implemented + planned):
1. User sends chat input from `studio`.
2. Shared agent loop (`agent/mod.rs`) executes turn and returns text outcome.
3. `studio/events.rs` carries typed turn and canvas update events.
4. Background graph worker refreshes architecture graph on:
   - file-watch events (debounced)
   - chat-turn completion
   - refresh failures are isolated and retried on the debounce interval without failing chat turns
5. `studio` drains graph updates in bounded batches per frame to keep the canvas/chat shell responsive under update bursts.
6. `studio` applies typed `CanvasOp` updates and re-renders canvas without blocking chat.
7. On each post-startup graph refresh, `studio` (via `GraphSurfaceState`) diffs old/new graph snapshots to:
   - highlight changed nodes
   - compute 1-hop impact nodes for renderer-side use
8. `studio/canvas.rs` provides generic canvas surface behavior (shared frame sizing, content insets, viewport drag/zoom input), and renderer adapters layer draw-command output on top with:
  - pan + scroll-wheel zoom viewport controls
  - fit/reset controls surfaced in the canvas toolbar (generic default controls)
  - draw-scene rendering of shapes/connectors/groups generated by renderer batches
  - clip-rect constrained scene rendering so objects do not bleed outside the visible stage
9. `studio/mod.rs` now regenerates renderer output (`ArchitectureOverviewRenderer`) on graph refreshes and tool/turn updates; resulting `CanvasDrawCommandBatch` is applied through `CanvasOp::ApplyDrawCommandBatch`.
10. `studio/mod.rs` dispatches canvas rendering through `CanvasSurfaceAdapter`/`CanvasSurfaceAdapterKind` so additional renderer modules can be added without changing runtime/tool contracts.

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
- `graph/watch.rs`: watch/debounce refresh orchestration only.
- `studio/*`: UI orchestration/presentation only; do not bypass agent/tool safety path.

## Legacy detail

Expanded architecture notes were archived to `docs/legacy/2026-02/ARCHITECTURE.md`.
