# Task Board

Execution task board for v1.

## Current focus: Phase 0

- [ ] Add `config.rs` with env defaults and typed settings.
- [ ] Add model provider setting with `ollama` default and `openai` fallback.
- [ ] Add CLI parsing with `chat "<message>"` command.
- [ ] Add structured logging initialization.
- [ ] Add placeholder `agent` module and return path for chat command.

## Next: Phase 1

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
