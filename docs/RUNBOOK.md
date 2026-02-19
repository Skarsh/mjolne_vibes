# Runbook

Operational setup and runtime commands.

## Prerequisites

- Rust stable (`cargo`)
- Docker
- `curl`
- Optional: OpenAI API key (only for OpenAI provider)

## Bootstrap

```bash
./scripts/install.sh
```

This script sets up `.env`, starts Ollama (Docker), pulls the model, and installs git hooks.

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
The UI is canvas-first with a collapsible chat rail, canvas pan/zoom/fit controls, and tool-call cards rendered directly on the canvas stage.

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
- `fetch_url` blocks: check allowlist/content-type/size limits.
- `fetch_url` invalid URL errors: include full scheme in prompts (for example, `https://example.com`).
- Loop/limit errors: check guardrail env values and turn trace logs.

## Legacy detail

Expanded operational notes were archived to `docs/legacy/2026-02/RUNBOOK.md`.
