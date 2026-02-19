# mjolne_vibes

`mjolne_vibes` is a Rust project for building a practical, CLI-first AI agent.

The target agent should:

- chat with the user,
- call a small set of typed tools,
- produce grounded responses from tool outputs,
- enforce execution and safety limits.

## Current state

Current implementation includes:

- CLI entrypoint with `chat "<message>"`,
- machine-readable one-shot output mode with `chat "<message>" --json`,
- interactive CLI REPL mode with slash commands (`/help`, `/tools`, `/reset`, `/exit`) and optional `--verbose` terminal logs,
- evaluation CLI mode (`eval --cases eval/cases.yaml`) with case-driven pass/fail checks,
- optional HTTP server mode (`serve --bind 127.0.0.1:8080`) with `POST /chat` and `GET /health`,
- typed runtime config from environment,
- local-first model provider setup (`ollama` default, `openai` fallback),
- model client integration with retry/backoff and request timeout,
- typed v1 tool argument schemas with strict unknown-field rejection,
- phase-2 tool registry/dispatcher with structured dispatch errors,
- iterative agent loop that handles model tool calls and feeds tool outputs back to the model,
- per-tool timeout handling and max tool-call cap per turn,
- max tool-calls per model response step (`AGENT_MAX_TOOL_CALLS_PER_STEP`),
- max consecutive tool-step cap per turn (`AGENT_MAX_CONSECUTIVE_TOOL_STEPS`),
- turn trace logging for step count, tool usage, and model/tool/turn latency,
- configurable input/output character limits (`AGENT_MAX_INPUT_CHARS`, `AGENT_MAX_OUTPUT_CHARS`),
- `save_note` writes markdown into a controlled directory (`NOTES_DIR`) with overwrite confirmation gating (`SAVE_NOTE_ALLOW_OVERWRITE`),
- live `fetch_url` retrieval with domain allowlist guardrail (`FETCH_URL_ALLOWED_DOMAINS`), response size cap (`FETCH_URL_MAX_BYTES`), and content-type checks,
- evaluation harness with `eval/cases.yaml` (20 baseline cases), checks for required tool usage/grounding/answer format, and isolated temporary notes directory handling for reproducible `save_note` cases.

## Provider strategy

Default development flow is local Ollama for cheap/free iteration.

- Recommended starter models:
  - `qwen2.5:3b`
  - `llama3.2:3b`
- OpenAI is supported as a fallback path when needed.

## Quickstart

1. Install prerequisites: Rust stable (`rustup`, `cargo`) and Docker.

2. Run the bootstrap script:

```bash
./scripts/install.sh
```

This bootstrap also installs repository git hooks from `.githooks/` so local pre-commit and pre-push checks run automatically.

3. Run:

```bash
cargo run -- chat "hello"
# JSON output mode for integrations:
cargo run -- chat "hello" --json
# or interactive mode:
cargo run -- repl
# interactive mode with terminal logs:
cargo run -- repl --verbose
# evaluation harness:
cargo run -- eval
# optional HTTP endpoint:
cargo run -- serve --bind 127.0.0.1:8080
```

## Manual setup (optional)

1. Start Ollama and pull a model:

```bash
# Native install path:
ollama pull qwen2.5:3b

# Docker path (no local ollama binary required):
docker run -d \
  --name ollama \
  --restart unless-stopped \
  -p 11434:11434 \
  -e OLLAMA_HOST=0.0.0.0:11434 \
  -v ollama-data:/root/.ollama \
  ollama/ollama:latest
docker exec ollama ollama pull qwen2.5:3b
```

2. Copy the env template:

```bash
cp .env.example .env
```

3. Configure environment (local-first profile):

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
FETCH_URL_ALLOWED_DOMAINS=example.com
NOTES_DIR=notes
SAVE_NOTE_ALLOW_OVERWRITE=false
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

Optional OpenAI profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
AGENT_MAX_STEPS=8
AGENT_MAX_TOOL_CALLS=8
AGENT_MAX_TOOL_CALLS_PER_STEP=4
AGENT_MAX_CONSECUTIVE_TOOL_STEPS=4
AGENT_MAX_INPUT_CHARS=4000
AGENT_MAX_OUTPUT_CHARS=8000
TOOL_TIMEOUT_MS=5000
FETCH_URL_MAX_BYTES=100000
FETCH_URL_ALLOWED_DOMAINS=example.com
NOTES_DIR=notes
SAVE_NOTE_ALLOW_OVERWRITE=false
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

4. Run:

```bash
cargo run -- chat "hello"
# JSON output mode for integrations:
cargo run -- chat "hello" --json
# or interactive mode:
cargo run -- repl
# interactive mode with terminal logs:
cargo run -- repl --verbose
# evaluation harness:
cargo run -- eval
# optional HTTP endpoint:
cargo run -- serve --bind 127.0.0.1:8080
```

HTTP endpoints:

- `GET /health`
- `POST /chat` with JSON body `{"message":"hello"}`

## Logging behavior

- REPL defaults to quiet terminal logging (`warn`+).
- Use `cargo run -- repl --verbose` to see info/debug logs in terminal.
- Detailed logs are written to `logs/mjolne_vibes.log.YYYY-MM-DD` by default.
- Turn trace summary logs include step count, tool usage, and model/tool/turn latency.
- Optional env overrides:
  - `RUST_LOG=...` (console filter)
  - `MJOLNE_FILE_LOG=...` (file filter)
  - `MJOLNE_LOG_DIR=...` (file log directory)

## Development quality checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Automation

- GitHub Actions CI: `.github/workflows/ci.yml`
- CI workflow also runs pre-commit-stage hygiene checks (`pre-commit run --all-files`).
- Repository git hooks: `.githooks/pre-commit`, `.githooks/pre-push`
- Hook installer: `./scripts/install_hooks.sh`
- Local pre-commit config (optional CI parity): `.pre-commit-config.yaml`

## Contributing docs

- `AGENTS.md`
- `docs/ARCHITECTURE.md`
- `docs/TESTING.md`
- `docs/SAFETY.md`
- `docs/RUNBOOK.md`
- `docs/ROADMAP.md`
- `docs/TASK_BOARD.md`
