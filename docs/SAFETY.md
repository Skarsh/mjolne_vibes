# Safety

Safety policy for runtime behavior.

## Objectives

- Prevent unsafe side effects.
- Prevent runaway loops and unbounded execution.
- Keep answers grounded in tool outputs when tools are used.

## Enforced limits

- `AGENT_MAX_STEPS`
- `AGENT_MAX_TOOL_CALLS`
- `AGENT_MAX_TOOL_CALLS_PER_STEP`
- `AGENT_MAX_CONSECUTIVE_TOOL_STEPS`
- `AGENT_MAX_INPUT_CHARS`
- `AGENT_MAX_OUTPUT_CHARS`
- `TOOL_TIMEOUT_MS`
- `FETCH_URL_MAX_BYTES`
- model request timeout/retries (`MODEL_TIMEOUT_MS`, `MODEL_MAX_RETRIES`)

## Tool policies

`fetch_url(url: string)`
- allow only `http`/`https`
- host must match `FETCH_URL_ALLOWED_DOMAINS`
- optional redirect-following (`FETCH_URL_FOLLOW_REDIRECTS=true`) is restricted to `http`/`https` targets whose hosts also match `FETCH_URL_ALLOWED_DOMAINS`
- enforce timeout, content-type checks, byte cap

`save_note(title: string, body: string)`
- write only inside `NOTES_DIR`
- reject unsafe/empty titles
- block overwrite unless `SAVE_NOTE_ALLOW_OVERWRITE=true`

`search_notes(query: string, limit: u8)`
- typed inputs only
- bounded result count (`u8`)

## Validation and block behavior

- Reject unknown fields in tool args.
- Return explicit machine-readable errors for policy/validation failures.
- HTTP `POST /chat` accepts only `{"message": string}` and rejects unknown fields.

## Transport parity

CLI (`chat`, `chat --json`), eval, and HTTP (`POST /chat`) must use the same loop and safety path.

## Legacy detail

Expanded safety notes were archived to `docs/legacy/2026-02/SAFETY.md`.
