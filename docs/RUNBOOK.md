# Runbook

Local development runbook for this repository.

## Prerequisites

- Rust stable
- `cargo`
- Docker
- Ollama (optional if not using Docker runtime)
- OpenAI API key (optional, only for OpenAI profile)

## One-command bootstrap

```bash
./scripts/install.sh
```

What it does:

- checks required local commands (`cargo`, `docker`, `curl`)
- creates `.env` from `.env.example` if missing
- starts Ollama (`docker compose` when available, `docker run` fallback)
- waits for `OLLAMA_BASE_URL` to be reachable
- pulls local model (`MODEL` when `MODEL_PROVIDER=ollama`, otherwise `qwen2.5:3b`)
- installs repository git hooks (`.githooks`) for local commit/push checks

## Cleanup local Ollama data

If you want to wipe local Ollama model data managed by Docker:

```bash
./scripts/cleanup_ollama_data.sh
```

The script auto-detects the target Docker volume and host mountpoint, then
prints them. It only deletes data when re-run with `--yes`.

```bash
./scripts/cleanup_ollama_data.sh --yes
```

## Environment variables

Start from template:

```bash
cp .env.example .env
```

Recommended local-first profile:

```env
MODEL_PROVIDER=ollama
MODEL=qwen2.5:3b
OLLAMA_BASE_URL=http://localhost:11434
AGENT_MAX_STEPS=8
AGENT_MAX_TOOL_CALLS=8
TOOL_TIMEOUT_MS=5000
FETCH_URL_ALLOWED_DOMAINS=example.com
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

Optional OpenAI fallback profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
AGENT_MAX_STEPS=8
AGENT_MAX_TOOL_CALLS=8
TOOL_TIMEOUT_MS=5000
FETCH_URL_ALLOWED_DOMAINS=example.com
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

Optional logging overrides:

```env
# Console log filter (default depends on command; REPL is quiet unless --verbose)
RUST_LOG=info,mjolne_vibes=debug
# File log filter (default: info,mjolne_vibes=debug)
MJOLNE_FILE_LOG=info,mjolne_vibes=debug
# File log directory (default: logs)
MJOLNE_LOG_DIR=logs
```

## Local model setup (Ollama)

Native Ollama install:

```bash
ollama pull qwen2.5:3b
ollama serve
```

Docker runtime (no local Ollama install required):

```bash
docker run -d \
  --name ollama \
  --restart unless-stopped \
  -p 11434:11434 \
  -e OLLAMA_HOST=0.0.0.0:11434 \
  -v ollama-data:/root/.ollama \
  ollama/ollama:latest

docker exec ollama ollama pull qwen2.5:3b
```

Compose runtime:

- `compose.yaml` is included in repo.
- Use whichever command exists on your system:
  - `docker compose up -d ollama`
  - `docker-compose up -d ollama`

## Common commands

Build and run current binary:

```bash
cargo check
cargo run
```

Formatting/lint/tests:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Git hooks

Install repository-managed git hooks:

```bash
./scripts/install_hooks.sh
```

Installed hook behavior:

- `pre-commit`: blocks trailing whitespace/conflict markers in staged changes and runs `cargo fmt --all -- --check`.
- `pre-push`: blocks trailing whitespace/conflict markers across tracked files, then runs `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`.

Optional CI-parity setup with Python `pre-commit`:

```bash
pip install pre-commit
pre-commit run --all-files
pre-commit run --all-files --hook-stage pre-push
```

## Current CLI command

```bash
cargo run -- chat "hello"
```

Interactive multi-turn mode:

```bash
cargo run -- repl
```

Interactive mode with terminal logs enabled:

```bash
cargo run -- repl --verbose
```

REPL commands:

- `/help`
- `/reset`
- `/exit`

Log files:

- Default path pattern: `logs/mjolne_vibes.log.YYYY-MM-DD`

## Troubleshooting

- If env values are missing, ensure shell exports or `.env` loading is configured.
- If using Ollama, ensure service is reachable at `OLLAMA_BASE_URL` and model is pulled.
- If using OpenAI, verify API key and model string.
- If loops/timeouts occur, inspect configured limits and trace logs.
