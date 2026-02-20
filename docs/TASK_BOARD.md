# Task Board

Current execution board for maintenance work.

## Current focus

- `Maintenance / Hardening`
- `Draw-Command Canvas Platform`

## Active tasks

- [x] Configurable subsystem mapping rules for studio architecture grouping. (Completed: 2026-02-20)
  - Goal: allow project-specific subsystem grouping rules instead of relying only on path/module heuristics.
  - Scope:
    - add optional runtime setting for a studio rules file path
    - add strict typed rules parsing with unknown-field rejection
    - apply first-match-wins rules in architecture renderer before heuristic fallback
    - keep deterministic ordering and existing draw-command contracts unchanged
  - Rule matching (v1):
    - `module_prefix` matches module ids (after stripping `module:`)
    - `file_path_prefix` matches file paths (supports `path` and `file:` id fallback)
    - if no rule matches, fallback to existing heuristic subsystem key
  - Validation:
    - each rule requires non-empty `subsystem`
    - each rule requires at least one matcher (`module_prefix` and/or `file_path_prefix`)
    - reject unknown JSON fields at both root and rule-item levels
  - Quality gates:
    - add renderer unit tests for precedence/fallback/validation
    - run `cargo fmt --all -- --check`
    - run `cargo clippy --all-targets --all-features -- -D warnings`
    - run `cargo test --all-targets --all-features`
  - Deliverables:
    - repo sample rules file: `.mjolne/subsystem_rules.json`

## Backlog candidates

- [ ] Add optional cost/usage counters in turn trace output.
- [ ] Add timeline/snapshot scrubber UI once turn snapshot model is stable.

## Archive

- Prior completed-item history is archived in `docs/legacy/2026-02/TASK_BOARD_HISTORY_2026-02-20.md`.

## Board rules

- Keep only active/backlog items here.
- Move detailed historical logs to `docs/legacy/`.
