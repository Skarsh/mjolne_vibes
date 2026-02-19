# Testing

## Required quality gates

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Test expectations

Unit tests should cover:
- config parsing/defaults
- tool arg validation (including unknown-field rejection)
- policy checks and limit enforcement

Integration/behavior coverage should include:
- one-turn chat success path
- tool-call loop behavior
- timeout/retry behavior
- `chat --json` structure
- HTTP `POST /chat` parity with CLI safety/limits

## Eval harness

- cases file: `eval/cases.yaml`
- run: `cargo run -- eval`
- optional custom suite: `cargo run -- eval --cases path/to/cases.yaml`
- target pass rate: `>= 80%`

## Failure requirements

- failures must be diagnosable from logs/check output
- policy blocks must include explicit reasons

## Legacy detail

Expanded testing notes were archived to `docs/legacy/2026-02/TESTING.md`.
