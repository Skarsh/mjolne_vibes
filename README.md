# mjolne_vibes

Rust-first AI agent project with a CLI-first v1 and optional HTTP service later.

## Current status

- Project stage: bootstrap (pre-Phase 0 implementation).
- Source plan: `AI_AGENT_RUST_PLAN.md` (generated February 18, 2026).
- Current code: minimal Rust binary scaffold.

## v1 goals

- Chat with the user.
- Call a small set of typed tools.
- Return grounded answers from tool outputs.
- Enforce guardrails: retries, timeouts, step limits, safety policies.

## v1 non-goals

- Long-term memory database.
- Background autonomous workers.
- GUI frontend.
- Multi-tenant auth and billing.

## Documentation index

- `AGENTS.md`: operating contract for AI coding agents in this repo.
- `docs/ROADMAP.md`: phased implementation plan and acceptance criteria.
- `docs/ARCHITECTURE.md`: system boundaries and agent loop contract.
- `docs/WORKFLOW.md`: agentic execution workflow for tasks and changes.
- `docs/TESTING.md`: test strategy, eval criteria, and quality gates.
- `docs/SAFETY.md`: tool safety policy and blocked-action behavior.
- `docs/RUNBOOK.md`: local setup and common development commands.
- `docs/TASK_BOARD.md`: active task queue and execution order.

## Model provider strategy

- Default dev provider: local Ollama (cheap/free local iteration).
- Recommended starter local models:
  - `qwen2.5:3b`
  - `llama3.2:3b`
- OpenAI remains a supported fallback for higher-quality evals and comparison.
- As of February 18, 2026, `gpt-4.1-mini` is still available.

## Quickstart

1. Install Rust stable (`rustup`, `cargo`).
2. Install Ollama and pull a local model:

```bash
ollama pull qwen2.5:3b
```

3. Add environment variables for local-first development:

```env
MODEL_PROVIDER=ollama
MODEL=qwen2.5:3b
OLLAMA_BASE_URL=http://localhost:11434
AGENT_MAX_STEPS=8
TOOL_TIMEOUT_MS=5000
```

4. Optional OpenAI fallback profile:

```env
MODEL_PROVIDER=openai
MODEL=gpt-4.1-mini
OPENAI_API_KEY=...
```

5. Build and run:

```bash
cargo check
cargo run
```

Note: `chat` CLI command and model/tool loop are planned in Phase 0-2.

## Quality gates (target before v1)

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Working model

Use `AGENTS.md` + docs in `docs/` as the primary execution system.  
`AI_AGENT_RUST_PLAN.md` remains the baseline source that these docs operationalize.
