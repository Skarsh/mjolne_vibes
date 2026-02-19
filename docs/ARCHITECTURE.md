# Architecture

## System intent

- CLI-first Rust agent with optional HTTP transport.
- Single shared agent loop for one-turn execution paths.
- Strong tool typing, explicit policies, deterministic limits.

## Module map

```text
src/
  main.rs          # CLI entrypoint
  config.rs        # env parsing + defaults
  agent/mod.rs     # orchestration loop + REPL + JSON mode
  model/client.rs  # provider adapters (ollama/openai)
  tools/mod.rs     # tool schemas + dispatch + policy checks
  eval/mod.rs      # eval harness and checks
  server/mod.rs    # HTTP transport; delegates to agent loop
```

## Agent loop contract

1. Build request from system prompt + conversation + user input.
2. Call model provider through `model/client.rs`.
3. If tool calls are returned:
   - validate tool name + typed args
   - enforce policy limits
   - execute tools with timeout
   - append tool outputs and continue
4. Stop on final text or guardrail/limit trigger.
5. Return final text + trace metadata.

## v1 tool contracts (fixed)

- `search_notes(query: string, limit: u8)`
- `fetch_url(url: string)`
- `save_note(title: string, body: string)`

## Boundary rules

- `model/client.rs`: provider protocol only; no business/safety policy.
- `tools/mod.rs`: tool-level logic and validation only.
- `agent/mod.rs`: loop control, limits, and step accounting.
- `server/mod.rs`: transport-only; no duplicated loop logic.
- `config.rs`: runtime limits and provider settings source.

## Legacy detail

Expanded architecture notes were archived to `docs/legacy/2026-02/ARCHITECTURE.md`.
