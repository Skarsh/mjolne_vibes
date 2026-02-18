# Testing

Testing strategy for v1 implementation.

## Quality gates

Run before considering a task complete:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Test layers

## 1) Unit tests

Focus:

- Config parsing defaults and env overrides.
- Tool argument validation (including unknown field rejection).
- Policy checks (allowlist, limits, blocking behavior).

Suggested locations:

- `src/config.rs`
- `src/tools/*`
- `src/state/*`
- `src/agent/*` (loop limit logic)

## 2) Integration tests

Focus:

- Chat turn with no tool call.
- Provider selection path:
  - local Ollama profile
  - OpenAI fallback profile (when credentials are available)
- Single tool call flow.
- Multi-step tool call flow.
- Tool timeout and retry behavior.

Suggested layout:

- `tests/chat_no_tool.rs`
- `tests/tool_single_step.rs`
- `tests/tool_multi_step.rs`
- `tests/timeouts_and_retries.rs`

## 3) Evaluation harness

Dataset:

- `eval/cases.yaml` with 20-30 representative prompts.

Required checks:

- Uses required tool when needed.
- Does not invent tool output.
- Produces expected final answer format.

Target:

- >= 80% pass rate on baseline suite.

## 4) Manual smoke tests

Minimum manual pass before v1:

- 10 normal prompts.
- 5 malformed/hostile prompts.
- 5 prompts forcing blocked actions.

## Failure handling expectations

- Failures should include actionable logs.
- Timeouts/retries should be visible in trace output.
- Policy blocks should include explicit refusal reasons.
