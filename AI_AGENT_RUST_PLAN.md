# First AI Agent in Rust: Detailed Build Plan

Generated: February 18, 2026

## 1) Goal

Build a production-minded but beginner-friendly AI agent in Rust that can:

- Chat with the user.
- Call a small set of tools.
- Return grounded answers using tool outputs.
- Enforce basic safety and execution limits.

Target form factor: terminal CLI first, optional HTTP service later.

## 2) Scope for v1

In-scope:

- Single-agent loop (no multi-agent orchestration).
- 2-3 tools with strict typed inputs.
- Conversation state for current session.
- Logging, retries, timeouts, and max-step limits.
- Basic evaluation set (20-30 prompts).

Out-of-scope (for v1):

- Long-term memory database.
- Autonomous background task runners.
- GUI frontend.
- Multi-tenant auth and billing.

## 3) Tech Choice (Recommended)

Recommended baseline stack:

- Model API client: `async-openai`
- Runtime: `tokio`
- Serialization: `serde`, `serde_json`
- Error handling: `anyhow`, `thiserror`
- Logging/tracing: `tracing`, `tracing-subscriber`
- CLI parsing: `clap`
- Config/env: `dotenvy`
- Testing helpers: `insta` (optional snapshots), `assert_cmd` (optional CLI tests)

Why this stack:

- Fastest path to a reliable first agent in Rust.
- Full control of the tool loop.
- Easier to debug than adopting a higher-level framework on day one.

Alternative:

- `rig` can reduce orchestration boilerplate, but is changing quickly; better as a step-2 migration after your loop is stable.

## 4) System Design (v1)

Core components:

1. `main.rs` / CLI
2. `agent/mod.rs` (agent loop orchestration)
3. `model/client.rs` (Responses API wrapper)
4. `tools/` (tool registry + tool executors)
5. `state/` (session history + limits)
6. `config.rs` (model, limits, env loading)
7. `eval/` (prompt cases and expected behavior)

Agent loop contract:

1. Build request from system instructions + history + user input.
2. Send to model.
3. If model returns tool calls:
4. Validate args against schema and policy.
5. Execute tool with timeout/retry policy.
6. Send tool outputs back to model.
7. Repeat until final text output or step limit hit.
8. Return answer + trace metadata.

Stop conditions:

- Final text response emitted.
- `max_steps` reached.
- Hard timeout reached.
- Policy block triggered.

## 5) Project Setup Steps

## 5.1 Prerequisites

- Rust stable (latest)
- `cargo` and `rustup`
- OpenAI API key in environment
- Optional: `just` for task running

## 5.2 Bootstrap

```bash
cargo new first-ai-agent --bin
cd first-ai-agent
cargo add tokio --features full
cargo add async-openai serde serde_json anyhow thiserror clap dotenvy tracing tracing-subscriber
```

Create `.env`:

```env
OPENAI_API_KEY=...
MODEL=gpt-4.1-mini
AGENT_MAX_STEPS=8
TOOL_TIMEOUT_MS=5000
```

## 5.3 Initial file layout

```text
src/
  main.rs
  config.rs
  agent/mod.rs
  model/client.rs
  tools/mod.rs
  tools/search_notes.rs
  tools/fetch_url.rs
  tools/save_note.rs
  state/mod.rs
eval/
  cases.yaml
```

## 6) Implementation Phases

## Phase 0: Skeleton + Config

Tasks:

- Add config loader for env vars with defaults.
- Add structured logging initialization.
- Add `AgentSettings` (`model`, `max_steps`, timeouts).
- Add a simple CLI command: `chat "<message>"`.

Acceptance criteria:

- Running `cargo run -- chat "hello"` loads config and prints placeholder output.

## Phase 1: Basic Model Chat (No tools)

Tasks:

- Implement model client wrapper.
- Send user/system messages and print model response.
- Add retry/backoff for transient failures.
- Add total request timeout.

Acceptance criteria:

- Agent can answer plain prompts reliably across 10 manual runs.
- Failures are logged with actionable error messages.

## Phase 2: Tool Calling Loop

Tasks:

- Define tool schemas for `search_notes`, `fetch_url`, `save_note`.
- Implement tool registry and dispatcher.
- Implement iterative loop for tool calls until final answer.
- Add per-tool timeout and max tool call count per turn.

Acceptance criteria:

- Prompt requiring at least one tool executes correctly end-to-end.
- Invalid tool arguments are rejected with clear error responses.

## Phase 3: Guardrails + Safety

Tasks:

- Add URL/domain allowlist for `fetch_url`.
- Add confirmation gate for file writes (`save_note`) if path is sensitive.
- Add input/output length limits.
- Add `max_steps` and max consecutive tool-call protections.

Acceptance criteria:

- Agent refuses disallowed operations with explicit reason.
- Infinite-loop tool call behavior is prevented.

## Phase 4: Observability + Evaluations

Tasks:

- Emit trace logs per turn (step count, tools used, latencies).
- Add evaluation harness using `eval/cases.yaml`.
- Track pass/fail criteria:
  - Uses required tool when needed.
  - Does not invent tool output.
  - Provides final answer format expected.

Acceptance criteria:

- At least 80% pass rate on initial 20-30 case set.
- Failures can be diagnosed from logs without rerunning in debug mode.

## Phase 5: Packaging + Next Step

Tasks:

- Add `README.md` with runbook and safety notes.
- Add `--json` output mode for integrations.
- Optionally expose simple HTTP endpoint (`axum`) reusing same loop.

Acceptance criteria:

- New machine setup from docs works in under 15 minutes.
- CLI and optional HTTP path produce equivalent behavior.

## 7) Tool Definitions for v1

Start with exactly these:

1. `search_notes(query: string, limit: u8)`  
Search local notes index (or a simple in-memory vector initially).

2. `fetch_url(url: string)`  
Fetch page content with strict allowlist and byte-size cap.

3. `save_note(title: string, body: string)`  
Save markdown note in a controlled directory.

Design rules:

- Keep args minimal and strongly typed.
- Reject unknown fields.
- Return machine-readable tool output JSON.

## 8) Testing Plan

Unit tests:

- Config parsing defaults and overrides.
- Tool argument validation.
- Policy checks (allowlist, step limits).

Integration tests:

- Chat without tool call.
- Single tool call flow.
- Multi-step tool call flow.
- Tool timeout and retry handling.

Manual smoke tests:

- 10 prompts normal usage.
- 5 prompts malformed/hostile input.
- 5 prompts forcing blocked actions.

Quality gates before v1 complete:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## 9) Risk Register and Mitigations

Risk: Tool-call loops or runaway iterations  
Mitigation: hard `max_steps`, max tool calls per turn, timeout budget.

Risk: Hallucinated tool outputs  
Mitigation: require explicit tool output echo in final response and validate references.

Risk: Unsafe external fetches  
Mitigation: domain allowlist + size/time limits + content-type checks.

Risk: API/model drift  
Mitigation: isolate model client module and keep integration tests around tool schema behavior.

## 10) Definition of Done (v1)

All conditions must be true:

- Single command runs agent from CLI.
- Tool-calling works for practical prompts.
- Safety controls block disallowed actions.
- Evaluation pass rate >= 80% on baseline suite.
- Reproducible setup documented in `README.md`.

## 11) Optional Phase 2 Upgrades

- Migrate orchestration to `rig` if it reduces boilerplate.
- Add memory backend (SQLite/Postgres + embeddings).
- Add retrieval pipeline (chunking + reranking).
- Add HTTP service + auth.
- Add telemetry dashboards and cost tracking.

## 12) Immediate Next Action

Start with Phase 0 and Phase 1 in a new crate named `first-ai-agent`, then run a single end-to-end prompt without tools before implementing the tool loop.
