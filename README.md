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
- typed runtime config from environment,
- local-first model provider setup (`ollama` default, `openai` fallback),
- model client integration with retry/backoff and request timeout.

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
TOOL_TIMEOUT_MS=5000
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

Optional OpenAI profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
MODEL_TIMEOUT_MS=20000
MODEL_MAX_RETRIES=2
```

4. Run:

```bash
cargo run -- chat "hello"
```

## Development quality checks

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Automation

- GitHub Actions CI: `.github/workflows/ci.yml`
- GitHub Actions pre-commit checks: `.github/workflows/pre-commit.yml`
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
