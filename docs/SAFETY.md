# Safety

Safety and guardrail policy for v1.

## Objectives

- Prevent unsafe tool side effects.
- Prevent runaway loops and unbounded cost.
- Keep model outputs grounded in actual tool results.

## Core limits

- `max_steps` per turn.
- Max tool calls per turn (`AGENT_MAX_TOOL_CALLS`).
- Max tool calls per model response step (`AGENT_MAX_TOOL_CALLS_PER_STEP`).
- Max consecutive tool-call steps per turn (`AGENT_MAX_CONSECUTIVE_TOOL_STEPS`).
- Input character limit (`AGENT_MAX_INPUT_CHARS`).
- Output character limit (`AGENT_MAX_OUTPUT_CHARS`).
- Per-tool timeout (`TOOL_TIMEOUT_MS`).
- Fetch response byte limit (`FETCH_URL_MAX_BYTES`).
- Overall turn timeout.

## Tool safety policy

## `fetch_url(url: string)`

- Enforce strict domain allowlist (`FETCH_URL_ALLOWED_DOMAINS`).
- Enforce byte-size cap (`FETCH_URL_MAX_BYTES`).
- Enforce timeout.
- Validate content type where applicable.

## `save_note(title: string, body: string)`

- Write only inside controlled notes directory.
- Treat overwrite of an existing note as sensitive and require confirmation.
- Confirmation path: set `SAVE_NOTE_ALLOW_OVERWRITE=true` to permit overwrite.
- Reject path traversal and disallowed paths.

## `search_notes(query: string, limit: u8)`

- Limit result count.
- Return structured output only.

## Validation policy

- Reject unknown fields in tool arguments.
- Reject invalid enum/range/type values.
- Return machine-readable tool errors with explicit reason.
- For HTTP transport (`POST /chat`), reject unknown request fields and require typed JSON input (`{"message": string}`).

## Block conditions

Block and return refusal reason when:

- Tool violates allowlist/path policy.
- Input/output exceeds configured size limits.
- Execution exceeds configured limits.
- Tool args fail schema/policy validation.

## Logging requirements

Log per turn:

- step count
- tool calls and outcomes
- model and tool latency metrics
- retry count
- timeout events
- policy block events

No sensitive key material should be logged.

## Transport parity policy

- CLI (`chat`, `chat --json`) and HTTP (`POST /chat`) must reuse the same agent loop and tool-policy enforcement path.
- Adding a new transport must not bypass existing loop limits, tool limits, or tool safety policies.
