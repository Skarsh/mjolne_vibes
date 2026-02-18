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
- placeholder agent module for Phase 1 integration.

## Provider strategy

Default development flow is local Ollama for cheap/free iteration.

- Recommended starter models:
  - `qwen2.5:3b`
  - `llama3.2:3b`
- OpenAI is supported as a fallback path when needed.

## Quickstart

1. Install Rust stable (`rustup`, `cargo`).
2. Install Ollama and pull a model:

```bash
ollama pull qwen2.5:3b
```

3. Copy the env template:

```bash
cp .env.example .env
```

4. Configure environment (local-first profile):

```env
MODEL_PROVIDER=ollama
MODEL=qwen2.5:3b
OLLAMA_BASE_URL=http://localhost:11434
AGENT_MAX_STEPS=8
TOOL_TIMEOUT_MS=5000
```

Optional OpenAI profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
```

5. Run:

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
- Local hook config: `.pre-commit-config.yaml`

## Contributing docs

- `AGENTS.md`
- `docs/ARCHITECTURE.md`
- `docs/TESTING.md`
- `docs/SAFETY.md`
- `docs/RUNBOOK.md`
- `docs/ROADMAP.md`
- `docs/TASK_BOARD.md`
