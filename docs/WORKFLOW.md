# Workflow

Agentic workflow for implementing this project.

## Task cycle

1. Pick one task from `docs/TASK_BOARD.md`.
2. Confirm phase alignment in `docs/ROADMAP.md`.
3. Implement smallest viable change.
4. Run relevant checks from `docs/TESTING.md`.
5. Update docs/tests as needed.
6. Mark task status and record notes.

## Definition of ready

Task is ready when:

- It belongs to current active phase.
- Inputs/outputs are clear.
- A validation method is defined.

## Definition of done

Task is done when:

- Code compiles.
- Relevant tests pass.
- Safety constraints remain satisfied.
- Related docs are updated.
- Task status is updated in `docs/TASK_BOARD.md`.

## Change sizing guidance

- Prefer one focused behavior change per task.
- Avoid coupling refactors with feature additions unless required.
- Keep PR/review scope understandable in one pass.

## Documentation update rule

Update docs immediately when changing:

- public CLI behavior
- tool schema/contracts
- safety policy
- architecture boundaries
- test strategy or quality gates
