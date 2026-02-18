# Task Board

Execution task board for v1.

## Current focus: Phase 3

Completed in Phase 0 (2026-02-18):

- [x] Add `config.rs` with env defaults and typed settings.
- [x] Add model provider setting with `ollama` default and `openai` fallback.
- [x] Add CLI parsing with `chat "<message>"` command.
- [x] Add structured logging initialization.
- [x] Add placeholder `agent` module and return path for chat command.

## Completed tasks: Phase 1

- [x] Set up local Ollama for validation (install/start service, run `ollama pull qwen2.5:3b`, verify `OLLAMA_BASE_URL` is reachable). (2026-02-18, Docker container + model pull + reachable API)
- [x] Add model client wrapper module. (2026-02-18)
- [x] Implement Ollama request path (`MODEL_PROVIDER=ollama`). (2026-02-18)
- [x] Implement OpenAI request path (`MODEL_PROVIDER=openai`). (2026-02-18)
- [x] Add basic system/user prompt request path. (2026-02-18)
- [x] Add retry/backoff for transient failures. (2026-02-18)
- [x] Add request timeout handling. (2026-02-18)
- [x] Run manual stability validation across 10 prompts with local Ollama reachable. (2026-02-18, `10/10` passed, no failed runs)

Progress notes (2026-02-18):

- Confirmed phase-1 code path and retries/logging are working; all quality gates passed (`fmt`, `clippy -D warnings`, `test`).
- Ollama is running in Docker on `http://localhost:11434` (`ollama/ollama:latest`).
- Pulled model `qwen2.5:3b` in container and verified with `/api/tags` and `ollama list`.
- Verified end-to-end CLI call: `cargo run -- chat "hello"` returns model output.
- Completed manual stability validation on local Ollama: `10/10` successful prompts (`cargo run --quiet -- chat ...`), with non-empty model responses on every run.

## Completed tasks: Phase 2

- [x] Define tool schema types for three v1 tools. (2026-02-18, strict typed args with unknown-field rejection tests in `src/tools/mod.rs`)
- [x] Add tool registry and dispatcher. (2026-02-18, `dispatch_tool_call` + registry list + unit coverage for unknown tool/invalid args/valid dispatch)
- [x] Implement tool-call iteration loop. (2026-02-18, `agent::run_chat` now iterates model responses, executes tool calls, and feeds tool outputs back into context until final text or `max_steps`)
- [x] Add per-tool timeout and tool-call cap. (2026-02-18, `AGENT_MAX_TOOL_CALLS` enforced per turn in `agent::run_chat`; per-tool timeout applied during dispatch with structured timeout errors)

## Upcoming: Phase 3

- [ ] Add URL/domain allowlist for `fetch_url`.
- [ ] Add confirmation gate for sensitive writes via `save_note`.
- [ ] Add input/output length limits.
- [ ] Add additional loop protections for runaway behavior.

## Usage notes

- Keep this file current as tasks move.
- Add completion date and short note when checking off items.
- Split large items into smaller tasks before starting implementation.
