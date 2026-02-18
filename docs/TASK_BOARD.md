# Task Board

Execution task board for v1.

## Current focus: Phase 1

Completed in Phase 0 (2026-02-18):

- [x] Add `config.rs` with env defaults and typed settings.
- [x] Add model provider setting with `ollama` default and `openai` fallback.
- [x] Add CLI parsing with `chat "<message>"` command.
- [x] Add structured logging initialization.
- [x] Add placeholder `agent` module and return path for chat command.

## Current tasks: Phase 1

- [ ] Set up local Ollama for validation (install/start service, run `ollama pull qwen2.5:3b`, verify `OLLAMA_BASE_URL` is reachable).
- [x] Add model client wrapper module. (2026-02-18)
- [x] Implement Ollama request path (`MODEL_PROVIDER=ollama`). (2026-02-18)
- [x] Implement OpenAI request path (`MODEL_PROVIDER=openai`). (2026-02-18)
- [x] Add basic system/user prompt request path. (2026-02-18)
- [x] Add retry/backoff for transient failures. (2026-02-18)
- [x] Add request timeout handling. (2026-02-18)
- [ ] Run manual stability validation across 10 prompts with local Ollama reachable.

## Upcoming: Phase 2

- [ ] Define tool schema types for three v1 tools.
- [ ] Add tool registry and dispatcher.
- [ ] Implement tool-call iteration loop.
- [ ] Add per-tool timeout and tool-call cap.

## Usage notes

- Keep this file current as tasks move.
- Add completion date and short note when checking off items.
- Split large items into smaller tasks before starting implementation.
