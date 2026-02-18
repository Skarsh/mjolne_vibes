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

- [ ] Add model client wrapper module.
- [ ] Implement Ollama request path (`MODEL_PROVIDER=ollama`).
- [ ] Implement OpenAI request path (`MODEL_PROVIDER=openai`).
- [ ] Add basic system/user prompt request path.
- [ ] Add retry/backoff for transient failures.
- [ ] Add request timeout handling.

## Upcoming: Phase 2

- [ ] Define tool schema types for three v1 tools.
- [ ] Add tool registry and dispatcher.
- [ ] Implement tool-call iteration loop.
- [ ] Add per-tool timeout and tool-call cap.

## Usage notes

- Keep this file current as tasks move.
- Add completion date and short note when checking off items.
- Split large items into smaller tasks before starting implementation.
