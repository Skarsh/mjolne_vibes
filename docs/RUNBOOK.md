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
TOOL_TIMEOUT_MS=5000
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

Optional OpenAI fallback profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
AGENT_MAX_STEPS=8
TOOL_TIMEOUT_MS=5000
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
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

## Pre-commit hooks

Install hooks:

```bash
pip install pre-commit
pre-commit install
pre-commit install --hook-type pre-push
```

Run all hooks manually:

```bash
pre-commit run --all-files
pre-commit run --all-files --hook-stage pre-push
```

Hook policy:

- `pre-commit`: file hygiene + `cargo fmt` check.
- `pre-push`: `cargo clippy` and `cargo test`.

## Current CLI command

```bash
cargo run -- chat "hello"
```

## Troubleshooting

- If env values are missing, ensure shell exports or `.env` loading is configured.
- If using Ollama, ensure service is reachable at `OLLAMA_BASE_URL` and model is pulled.
- If using OpenAI, verify API key and model string.
- If loops/timeouts occur, inspect configured limits and trace logs.
