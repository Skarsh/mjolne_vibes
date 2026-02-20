# mjolne_vibes

`mjolne_vibes` is a CLI-first Rust AI agent.

It supports:
- one-shot chat (`chat "..."`)
- one-shot JSON output (`chat "..." --json`)
- interactive REPL (`repl`)
- evaluation runs (`eval`)
- optional HTTP transport (`serve`)
- native desktop studio UI (`studio`) with collapsible chat rail and interactive canvas

## Core behavior

- Provider selection via config (`ollama` default, `openai` fallback).
- Shared agent loop across CLI, eval, and HTTP.
- `studio` uses a generic-first canvas shell with viewport controls (pan/zoom/fit) and surface-adapter dispatch (`CanvasSurfaceAdapter`) for a clean, canvas-first view.
- Canvas direction is a complete draw-command surface (tldraw-style primitives and scene mutations) where higher-level renderers compile domain state into generic draw ops.
- First renderer target is architecture/workflow visibility for agent activity: show what the agent changed, is changing, and plans to change without requiring raw diff reading.
- `studio` shell visuals are tuned for readability with guide-grid and lane cues in the canvas stage while keeping canvas chrome minimal.
- Canvas update intents are target-oriented (`SetSceneData`, `SetHighlightedTargets`, `SetFocusedTarget`, `UpsertAnnotation`) with legacy graph-op aliases kept during transition.
- Canvas metadata/telemetry panels are intentionally minimized so the central surface remains focused on the graph scene.
- Graph refresh handling stays failure-isolated: refresh failures retry in the background, and UI drains graph updates in bounded batches per frame to preserve chat responsiveness.
- Strict, typed v1 tools:
  - `search_notes(query: string, limit: u8)`
  - `fetch_url(url: string)`
  - `save_note(title: string, body: string)`
- Safety limits for steps, tool-call budgets, input/output size, and tool timeouts.

## Quickstart

`compose.yaml` is configured to run Ollama with an NVIDIA GPU. Ensure NVIDIA Container Toolkit is installed and Docker has the `nvidia` runtime available before bootstrap.

1. Bootstrap:

```bash
./scripts/install.sh
```

2. Run:

```bash
cargo run -- chat "hello"
cargo run -- chat "hello" --json
cargo run -- repl
cargo run -- eval
cargo run -- serve --bind 127.0.0.1:8080
cargo run -- studio
```

## Quality checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Documentation

- Agent instructions: `AGENTS.md`
- Operational setup: `docs/RUNBOOK.md`
- Current scope/architecture/safety/testing/workflow: `docs/ROADMAP.md`, `docs/ARCHITECTURE.md`, `docs/SAFETY.md`, `docs/TESTING.md`, `docs/WORKFLOW.md`
- Active tasks: `docs/TASK_BOARD.md`
- Archived historical docs: `docs/legacy/2026-02/`
