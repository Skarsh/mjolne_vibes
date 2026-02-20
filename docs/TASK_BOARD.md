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
- [ ] Studio visual hierarchy + polish pass (Rerun-inspired, non-generic UI).
  - Goal: reduce “flat/bland” appearance and improve information hierarchy in `studio` while preserving current behavior and safety contracts.
  - Problem statement:
    - current shell uses a narrow color/luminance range, near-uniform stroke weights, and limited type hierarchy
    - interaction states and panel boundaries are readable but visually understated
  - Scope (phase 1 visual refresh):
    - implement a stronger 3-tier surface system:
      - app background
      - panel background
      - canvas stage background
    - tighten semantic color usage:
      - neutral controls by default
      - accent color reserved for selected/active/high-value states
    - improve typography hierarchy:
      - more distinct heading/body/meta scales
      - clearer “status” text treatment vs actionable controls
    - add compact status strip:
      - provider/model
      - runtime state (`Idle` / running / failed)
      - graph refresh state
    - add minimal purposeful motion:
      - mode switch transition cue
      - snapshot-selection visual cue
  - Non-goals:
    - no changes to agent loop, tool contracts, or safety policy behavior
    - no canvas data-model changes in this task
  - Implementation notes:
    - primary edit targets: `src/studio/mod.rs`, optionally `src/studio/canvas.rs` for stage styling hooks
    - preserve current layout structure (collapsible chat rail + canvas-first shell)
    - avoid heavy animation or distracting effects
  - Acceptance criteria:
    - visible separation between shell chrome and canvas stage at a glance
    - active mode/buttons/status are immediately distinguishable from inactive controls
    - no regressions in current studio tests
    - docs updated if new visual toggles/settings are introduced
  - Quality gates:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all-targets --all-features -- -D warnings`
    - `cargo test --all-targets --all-features`
  - Handoff context:
    - baseline references discussed: Rerun demo/example aesthetic (hierarchy, density, semantic accents)
    - user feedback: current UI feels “bland / boring”; prioritize bold-but-pragmatic styling improvements
  - Progress snapshot (2026-02-20):
    - completed:
      - introduced stronger tiered surface palette (app/panel/stage separation)
      - increased header typography hierarchy and overall contrast between chrome vs content
      - added compact status strip in top bar (provider/model, graph refresh trigger, canvas status)
      - upgraded mode controls (`Before/After`, `Focus`) to animated high-contrast pills
      - wrapped canvas render region in explicit stage frame for clearer visual boundary
      - added snapshot-navigation motion cue via pulsed snapshot metadata emphasis on selection changes
      - tightened small-width density:
        - compact toolbar labels (`B/A`, `F On`) with wrapped controls
        - chat pane hides workspace chip under narrow widths and reduces composer reservation
    - pending:
      - optional second-pass visual tune after user review feedback

## Backlog candidates

- [ ] Add optional cost/usage counters in turn trace output.
- [ ] Add timeline/snapshot scrubber UI once turn snapshot model is stable.

## Archive

- Prior completed-item history is archived in `docs/legacy/2026-02/TASK_BOARD_HISTORY_2026-02-20.md`.

## Board rules

- Keep only active/backlog items here.
- Move detailed historical logs to `docs/legacy/`.
