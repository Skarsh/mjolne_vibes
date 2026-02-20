# Runbook

Operational setup and runtime commands.

## Prerequisites

- Rust stable (`cargo`)
- Docker
- NVIDIA Container Toolkit configured for Docker (`nvidia` runtime available)
- `curl`
- Optional: OpenAI API key (only for OpenAI provider)

## Bootstrap

```bash
./scripts/install.sh
```

This script sets up `.env`, starts Ollama (Docker), pulls the model, and installs git hooks.
The repo `compose.yaml` requests an NVIDIA GPU for the Ollama container.

## Default environment

```env
MODEL_PROVIDER=ollama
MODEL=qwen2.5:3b
OLLAMA_BASE_URL=http://localhost:11434
AGENT_MAX_STEPS=8
AGENT_MAX_TOOL_CALLS=8
AGENT_MAX_TOOL_CALLS_PER_STEP=4
AGENT_MAX_CONSECUTIVE_TOOL_STEPS=4
AGENT_MAX_INPUT_CHARS=4000
AGENT_MAX_OUTPUT_CHARS=8000
TOOL_TIMEOUT_MS=5000
FETCH_URL_MAX_BYTES=100000
FETCH_URL_FOLLOW_REDIRECTS=false
FETCH_URL_ALLOWED_DOMAINS=example.com
NOTES_DIR=notes
SAVE_NOTE_ALLOW_OVERWRITE=false
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
# Optional: studio-only subsystem grouping overrides.
# STUDIO_SUBSYSTEM_RULES_FILE=.mjolne/subsystem_rules.json
```

OpenAI fallback:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
```

Optional web-fetch profile (larger/redirecting sites):

```env
# Keep allowlist scoped to trusted domains.
FETCH_URL_ALLOWED_DOMAINS=example.com
FETCH_URL_FOLLOW_REDIRECTS=true
FETCH_URL_MAX_BYTES=250000
AGENT_MAX_OUTPUT_CHARS=150000
MODEL_TIMEOUT_MS=120000
MODEL_MAX_RETRIES=1
```

## Commands

```bash
cargo run -- chat "hello"
cargo run -- chat "hello" --json
cargo run -- repl
cargo run -- repl --verbose
cargo run -- eval
cargo run -- serve --bind 127.0.0.1:8080
cargo run -- studio
```

`studio` opens a native desktop window and requires a graphical session.
When running, it auto-refreshes workspace graph stats after chat-turn completion and debounced Rust file changes.
The UI is canvas-first with a collapsible chat rail and canvas controls for pan/zoom/fit plus mode toggles (`Live`, `Before/After`, `Focus`).
Current studio visuals keep shell chrome minimal and focus the stage on subsystem-structured topology and change overlays.
Roadmap direction is a full draw-command canvas platform: renderer modules will translate domain state (starting with architecture + agent-work context) into generic draw commands consumed by the canvas core.

Optional studio subsystem mapping rules:
- Set `STUDIO_SUBSYSTEM_RULES_FILE` to a JSON file path (absolute or workspace-relative).
- Rules are evaluated in order; first match wins.
- If no rule matches, studio falls back to built-in path/module heuristics.
- The repo ships a starter file at `.mjolne/subsystem_rules.json`.

Example `subsystem_rules.json`:

```json
{
  "rules": [
    { "subsystem": "runtime-core", "module_prefix": "crate::agent" },
    { "subsystem": "runtime-core", "module_prefix": "crate::tools" },
    { "subsystem": "ui-shell", "file_path_prefix": "src/studio/" }
  ]
}
```

## Studio Change-Review Workflow

Use this sequence when reviewing what an agent turn changed:

1. Keep canvas mode at `Live` while the turn runs.
   - Purpose: see current topology and ensure graph refreshes are healthy.
2. Switch to `Before/After` after a turn completes.
   - Purpose: compare baseline vs outcome for the selected snapshot.
   - Read: `Δ +A -R ~C` summary (`A` added, `R` removed, `C` changed).
3. Switch to `Focus`.
   - Purpose: dim unchanged topology and concentrate on changed + impact targets.
4. Use snapshot navigation (`←` / `→`) in the canvas toolbar.
   - Purpose: move across recorded turns and compare architectural drift over time.
5. Use zoom/fit after snapshot changes.
   - Purpose: keep changed regions in-frame for quick inspection.

Operator guidance:
- If `Before/After` shows minimal deltas but behavior changed, inspect edge differences and changed node labels first.
- If `Focus` still looks noisy, step snapshots one-by-one and inspect high fan-out systems first.

HTTP endpoints:
- `GET /health`
- `POST /chat` with `{"message":"hello"}`

## Quality gates

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Troubleshooting

- Provider errors: verify model/provider settings and connectivity.
- `could not select device driver "nvidia" with capabilities: [[gpu]]`: Docker cannot access NVIDIA runtime yet; install/configure NVIDIA Container Toolkit and restart Docker.
- `fetch_url` blocks: check allowlist/content-type/size limits.
- `fetch_url` invalid URL errors: include full scheme in prompts (for example, `https://example.com`).
- Loop/limit errors: check guardrail env values and turn trace logs.

## Legacy detail

Expanded operational notes were archived to `docs/legacy/2026-02/RUNBOOK.md`.
