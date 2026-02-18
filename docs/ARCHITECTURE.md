# Architecture

Target architecture for v1 Rust AI agent.

## System intent

- CLI-first agent that can chat and call a small tool set.
- Support both one-shot (`chat`) and interactive (`repl`) CLI usage with shared loop behavior.
- Keep interactive (`repl`) terminal output low-noise by default while preserving detailed trace logs in file output.
- Deterministic execution boundaries with limits and safety policies.
- Clear module boundaries to isolate model API drift and tool complexity.
- Provider-flexible model access: local Ollama default, OpenAI fallback.

## Planned module layout

```text
src/
  lib.rs                # shared crate modules for bin + tests
  main.rs               # CLI entry point
  config.rs             # env/config loading and defaults
  agent/mod.rs          # orchestration loop
  model/client.rs       # model API wrapper
  tools/mod.rs          # tool arg schemas, registry, and dispatch
  tools/search_notes.rs
  tools/fetch_url.rs
  tools/save_note.rs
  state/mod.rs          # session state and execution limits
eval/
  cases.yaml            # evaluation dataset
```

## Agent loop contract

1. Build request from system instructions, history, and user input.
2. Send request to model client.
3. If tool calls are returned:
   - validate tool name and typed args
   - apply policy checks
   - execute tools with timeout/retry policy
   - return tool outputs to model
4. Repeat until final text response or stop condition.
5. Return final answer plus trace metadata.

## Stop conditions

- Final text response produced.
- `max_steps` reached.
- Max tool-call budget reached.
- Hard timeout reached.
- Policy block triggered.

## Tool contracts (v1)

Use exactly these tool interfaces:

- `search_notes(query: string, limit: u8)`
- `fetch_url(url: string)`
- `save_note(title: string, body: string)`

Tool design rules:

- Minimal, strongly typed args.
- Reject unknown fields.
- Return machine-readable JSON.

Current Phase 2 implementation status:

- Typed tool args are implemented in `src/tools/mod.rs` with `serde(deny_unknown_fields)`.
- Tool registry and dispatcher are implemented in `src/tools/mod.rs` via `tool_definitions` and `dispatch_tool_call`.
- Agent loop integration for tool calls is implemented in `src/agent/mod.rs`.
- Per-tool timeout and per-turn tool-call cap are implemented in `src/agent/mod.rs` and configured through `src/config.rs` (`TOOL_TIMEOUT_MS`, `AGENT_MAX_TOOL_CALLS`).

Current Phase 3 safety implementation status:

- `fetch_url` domain allowlist enforcement is implemented in `src/tools/mod.rs`, with the allowed domains sourced from `src/config.rs` (`FETCH_URL_ALLOWED_DOMAINS`).
- Per-step tool-call batching protection is enforced in `src/agent/mod.rs` via configurable `AGENT_MAX_TOOL_CALLS_PER_STEP` from `src/config.rs`.
- Consecutive tool-call loop protection is enforced in `src/agent/mod.rs` via configurable `AGENT_MAX_CONSECUTIVE_TOOL_STEPS` from `src/config.rs`.
- Input/output character limits are enforced in `src/agent/mod.rs` using runtime settings from `src/config.rs` (`AGENT_MAX_INPUT_CHARS`, `AGENT_MAX_OUTPUT_CHARS`).
- `save_note` writes to a controlled notes directory (`NOTES_DIR`) with overwrite confirmation gating (`SAVE_NOTE_ALLOW_OVERWRITE`) in `src/tools/mod.rs`, with runtime config loaded via `src/config.rs`.

Current Phase 4 observability implementation status:

- Turn-level trace summary logs are emitted from `src/agent/mod.rs`, including step count, tool usage, and model/tool/turn latency metrics.

## Boundary rules

- `model/client.rs` must not encode business/tool policy.
- `model/client.rs` should hide provider-specific details from the orchestration loop.
- `tools/*` must not directly mutate global agent state.
- `agent/mod.rs` owns loop control and step accounting.
- `config.rs` is the source of runtime limits.
- `state/mod.rs` tracks per-session state only for v1.

## Model provider policy

- Default local development provider: Ollama.
- Supported fallback provider: OpenAI.
- Provider should be selected by configuration (`MODEL_PROVIDER` + `MODEL`), not hardcoded.

## Extension direction after v1

- Optional migration to `rig` if it clearly reduces orchestration boilerplate.
- Optional HTTP layer should reuse the same core loop and policies.
