# Workflow

## Task cycle

1. Pick one task from `docs/TASK_BOARD.md`.
2. Confirm scope in `docs/ROADMAP.md`.
3. Make the smallest viable change.
4. Run relevant checks from `docs/TESTING.md`.
5. Update docs for behavior/interface/policy changes.
6. Update task status in `docs/TASK_BOARD.md`.

## Definition of done

- Code compiles.
- Relevant tests pass.
- Safety constraints remain satisfied.
- Docs are updated where behavior changed.
- Task board status is current.

## Documentation style rules

- Keep active docs concise and task-oriented.
- Avoid duplicating env vars, tool contracts, or limits across many files.
- Prefer one source-of-truth doc section and link to it.
- Move historical narrative to `docs/legacy/`.
