use std::error::Error as StdError;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SEARCH_NOTES_TOOL_NAME: &str = "search_notes";
pub const FETCH_URL_TOOL_NAME: &str = "fetch_url";
pub const SAVE_NOTE_TOOL_NAME: &str = "save_note";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
}

const TOOL_DEFINITIONS: [ToolDefinition; 3] = [
    ToolDefinition {
        name: SEARCH_NOTES_TOOL_NAME,
    },
    ToolDefinition {
        name: FETCH_URL_TOOL_NAME,
    },
    ToolDefinition {
        name: SAVE_NOTE_TOOL_NAME,
    },
];

pub fn tool_definitions() -> &'static [ToolDefinition] {
    &TOOL_DEFINITIONS
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchNotesArgs {
    pub query: String,
    pub limit: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchUrlArgs {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveNoteArgs {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolDispatchOutput {
    pub tool_name: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRuntimeConfig {
    pub fetch_url_allowed_domains: Vec<String>,
    pub notes_dir: PathBuf,
    pub save_note_allow_overwrite: bool,
    pub tool_timeout_ms: u64,
    pub fetch_url_max_bytes: usize,
}

impl ToolRuntimeConfig {
    pub fn new(
        fetch_url_allowed_domains: Vec<String>,
        notes_dir: PathBuf,
        save_note_allow_overwrite: bool,
        tool_timeout_ms: u64,
        fetch_url_max_bytes: usize,
    ) -> Self {
        Self {
            fetch_url_allowed_domains,
            notes_dir,
            save_note_allow_overwrite,
            tool_timeout_ms,
            fetch_url_max_bytes,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolDispatchError {
    #[error("unknown tool `{tool_name}`")]
    UnknownTool { tool_name: String },

    #[error("invalid args for tool `{tool_name}`: {reason}")]
    InvalidArgs { tool_name: String, reason: String },

    #[error("policy block for tool `{tool_name}`: {reason}")]
    PolicyViolation { tool_name: String, reason: String },

    #[error("execution failed for tool `{tool_name}`: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },
}

pub async fn dispatch_tool_call(
    tool_name: &str,
    raw_args: Value,
    runtime: &ToolRuntimeConfig,
) -> Result<ToolDispatchOutput, ToolDispatchError> {
    let payload = match tool_name {
        SEARCH_NOTES_TOOL_NAME => Ok(run_search_notes(parse_args(tool_name, raw_args)?)),
        FETCH_URL_TOOL_NAME => {
            run_fetch_url(
                parse_args(tool_name, raw_args)?,
                &runtime.fetch_url_allowed_domains,
                runtime.tool_timeout_ms,
                runtime.fetch_url_max_bytes,
            )
            .await
        }
        SAVE_NOTE_TOOL_NAME => run_save_note(
            parse_args(tool_name, raw_args)?,
            &runtime.notes_dir,
            runtime.save_note_allow_overwrite,
        ),
        _ => {
            return Err(ToolDispatchError::UnknownTool {
                tool_name: tool_name.to_owned(),
            });
        }
    }?;

    Ok(ToolDispatchOutput {
        tool_name: tool_name.to_owned(),
        payload,
    })
}

fn parse_args<T: for<'de> Deserialize<'de>>(
    tool_name: &str,
    raw_args: Value,
) -> Result<T, ToolDispatchError> {
    serde_json::from_value(raw_args).map_err(|error| ToolDispatchError::InvalidArgs {
        tool_name: tool_name.to_owned(),
        reason: error.to_string(),
    })
}

fn run_search_notes(args: SearchNotesArgs) -> Value {
    json!({
        "query": args.query,
        "limit": args.limit,
        "results": []
    })
}

async fn run_fetch_url(
    args: FetchUrlArgs,
    fetch_url_allowed_domains: &[String],
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
) -> Result<Value, ToolDispatchError> {
    run_fetch_url_with_fetcher(
        args,
        fetch_url_allowed_domains,
        tool_timeout_ms,
        fetch_url_max_bytes,
        fetch_url_over_http,
    )
    .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FetchResponse {
    final_url: String,
    status_code: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

async fn run_fetch_url_with_fetcher<F, Fut>(
    args: FetchUrlArgs,
    fetch_url_allowed_domains: &[String],
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
    fetcher: F,
) -> Result<Value, ToolDispatchError>
where
    F: Fn(Url, u64, usize) -> Fut,
    Fut: std::future::Future<Output = Result<FetchResponse, ToolDispatchError>>,
{
    let parsed = parse_fetch_url(&args.url, fetch_url_allowed_domains)?;
    let fetched = fetcher(parsed, tool_timeout_ms, fetch_url_max_bytes).await?;

    if !status_is_success(fetched.status_code) {
        return Err(ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "received non-success HTTP status {} for `{}`",
                fetched.status_code, args.url
            ),
        });
    }

    if let Some(value) = fetched.content_type.as_deref()
        && !content_type_allowed(value)
    {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("content type `{value}` is not allowed"),
        });
    }

    if fetched.body.len() > fetch_url_max_bytes {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "response body exceeded FETCH_URL_MAX_BYTES limit: {} bytes (max {fetch_url_max_bytes})",
                fetched.body.len()
            ),
        });
    }

    let content = String::from_utf8_lossy(&fetched.body).to_string();
    Ok(json!({
        "url": args.url,
        "final_url": fetched.final_url,
        "status_code": fetched.status_code,
        "content_type": fetched.content_type,
        "bytes": fetched.body.len(),
        "content": content,
    }))
}

async fn fetch_url_over_http(
    parsed_url: Url,
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
) -> Result<FetchResponse, ToolDispatchError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(tool_timeout_ms))
        // Keep redirects disabled to prevent cross-domain hops outside allowlist intent.
        .redirect(Policy::none())
        .build()
        .map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to build HTTP client: {}",
                describe_reqwest_error(&error)
            ),
        })?;

    let mut response = client
        .get(parsed_url.clone())
        .send()
        .await
        .map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to fetch `{parsed_url}`: {}",
                describe_reqwest_error(&error)
            ),
        })?;

    let status_code = response.status().as_u16();
    let content_type = extract_content_type(response.headers())?;
    let mut body = Vec::new();
    while let Some(chunk) =
        response
            .chunk()
            .await
            .map_err(|error| ToolDispatchError::ExecutionFailed {
                tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                reason: format!(
                    "failed to read response body from `{parsed_url}`: {}",
                    describe_reqwest_error(&error)
                ),
            })?
    {
        let next_len = body.len().checked_add(chunk.len()).ok_or_else(|| {
            ToolDispatchError::ExecutionFailed {
                tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                reason: "response body length overflowed while reading".to_owned(),
            }
        })?;
        if next_len > fetch_url_max_bytes {
            return Err(ToolDispatchError::PolicyViolation {
                tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                reason: format!(
                    "response body exceeded FETCH_URL_MAX_BYTES limit: {next_len} bytes (max {fetch_url_max_bytes})"
                ),
            });
        }
        body.extend_from_slice(&chunk);
    }

    Ok(FetchResponse {
        final_url: response.url().as_str().to_owned(),
        status_code,
        content_type,
        body,
    })
}

fn status_is_success(status_code: u16) -> bool {
    (200..300).contains(&status_code)
}

fn describe_reqwest_error(error: &reqwest::Error) -> String {
    let mut details = vec![format_error_chain(error)];
    let mut classes = Vec::new();

    if error.is_builder() {
        classes.push("builder");
    }
    if error.is_connect() {
        classes.push("connect");
    }
    if error.is_timeout() {
        classes.push("timeout");
    }
    if error.is_request() {
        classes.push("request");
    }
    if error.is_body() {
        classes.push("body");
    }
    if error.is_decode() {
        classes.push("decode");
    }
    if error.is_redirect() {
        classes.push("redirect");
    }
    if error.is_status() {
        classes.push("status");
    }

    if !classes.is_empty() {
        details.push(format!("class={}", classes.join(",")));
    }
    if let Some(url) = error.url() {
        details.push(format!("url={url}"));
    }

    details.join(" | ")
}

fn format_error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        chain.push_str(": ");
        chain.push_str(&next.to_string());
        source = next.source();
    }
    chain
}

fn parse_fetch_url(
    url: &str,
    fetch_url_allowed_domains: &[String],
) -> Result<Url, ToolDispatchError> {
    let parsed = Url::parse(url).map_err(|error| ToolDispatchError::InvalidArgs {
        tool_name: FETCH_URL_TOOL_NAME.to_owned(),
        reason: format!("invalid url `{url}`: {error}"),
    })?;

    let host = parsed
        .host_str()
        .ok_or_else(|| ToolDispatchError::InvalidArgs {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("url `{url}` must include a host"),
        })?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("url scheme `{}` is not allowed", parsed.scheme()),
        });
    }

    let host = host.to_ascii_lowercase();
    if !host_allowed(&host, fetch_url_allowed_domains) {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("url host `{host}` is not in allowlist"),
        });
    }

    Ok(parsed)
}

fn extract_content_type(headers: &HeaderMap) -> Result<Option<String>, ToolDispatchError> {
    let Some(value) = headers.get(CONTENT_TYPE) else {
        return Ok(None);
    };

    let raw = value
        .to_str()
        .map_err(|error| ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("invalid response content type header: {error}"),
        })?;

    let normalized = raw
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalized))
    }
}

fn content_type_allowed(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || content_type == "application/json"
        || content_type.ends_with("+json")
        || content_type == "application/xml"
        || content_type == "text/xml"
        || content_type.ends_with("+xml")
}

fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|allowed_domain| {
        host == allowed_domain || host.ends_with(&format!(".{allowed_domain}"))
    })
}

fn run_save_note(
    args: SaveNoteArgs,
    notes_dir: &Path,
    save_note_allow_overwrite: bool,
) -> Result<Value, ToolDispatchError> {
    let title = args.title.trim();
    if title.is_empty() {
        return Err(ToolDispatchError::InvalidArgs {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: "title cannot be empty".to_owned(),
        });
    }

    let note_slug = normalize_note_title(title).ok_or_else(|| ToolDispatchError::InvalidArgs {
        tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
        reason: "title must include at least one alphanumeric character".to_owned(),
    })?;
    let note_filename = format!("{note_slug}.md");
    let note_path = notes_dir.join(&note_filename);
    let is_overwrite = note_path.exists();

    if is_overwrite && !save_note_allow_overwrite {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: format!(
                "refusing to overwrite existing note `{}` without confirmation; set SAVE_NOTE_ALLOW_OVERWRITE=true to confirm overwrite",
                note_path.display()
            ),
        });
    }

    fs::create_dir_all(notes_dir).map_err(|error| ToolDispatchError::ExecutionFailed {
        tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
        reason: format!(
            "failed to create notes directory `{}`: {error}",
            notes_dir.display()
        ),
    })?;

    let file_content = format!("# {title}\n\n{}\n", args.body);
    fs::write(&note_path, &file_content).map_err(|error| ToolDispatchError::ExecutionFailed {
        tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
        reason: format!("failed to write note `{}`: {error}", note_path.display()),
    })?;

    Ok(json!({
        "title": title,
        "path": note_path.display().to_string(),
        "bytes": file_content.len(),
        "status": if is_overwrite { "overwritten" } else { "created" }
    }))
}

fn normalize_note_title(title: &str) -> Option<String> {
    let mut output = String::new();
    let mut previous_was_dash = false;

    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
            previous_was_dash = false;
            continue;
        }

        if (ch.is_whitespace() || ch == '-' || ch == '_')
            && !output.is_empty()
            && !previous_was_dash
        {
            output.push('-');
            previous_was_dash = true;
        }
    }

    while output.ends_with('-') {
        output.pop();
    }

    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::Future;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{Value, json};

    use super::{
        FETCH_URL_TOOL_NAME, FetchResponse, FetchUrlArgs, SAVE_NOTE_TOOL_NAME,
        SEARCH_NOTES_TOOL_NAME, ToolDispatchError, ToolDispatchOutput, ToolRuntimeConfig,
        dispatch_tool_call as dispatch_tool_call_async, host_allowed, normalize_note_title,
        run_fetch_url_with_fetcher, tool_definitions,
    };

    fn dispatch_tool_call(
        tool_name: &str,
        raw_args: Value,
        tool_runtime: &ToolRuntimeConfig,
    ) -> Result<ToolDispatchOutput, ToolDispatchError> {
        block_on(dispatch_tool_call_async(tool_name, raw_args, tool_runtime))
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should initialize");
        runtime.block_on(future)
    }

    #[test]
    fn registry_contains_three_v1_tools() {
        let names: Vec<_> = tool_definitions().iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                SEARCH_NOTES_TOOL_NAME,
                FETCH_URL_TOOL_NAME,
                SAVE_NOTE_TOOL_NAME
            ]
        );
    }

    #[test]
    fn dispatch_rejects_unknown_tool_name() {
        let runtime = test_runtime_config("unknown_tool", false);
        let error =
            dispatch_tool_call("unknown_tool", json!({}), &runtime).expect_err("should fail");
        assert_eq!(
            error,
            ToolDispatchError::UnknownTool {
                tool_name: "unknown_tool".to_owned()
            }
        );
    }

    #[test]
    fn dispatch_search_notes_returns_structured_payload() {
        let runtime = test_runtime_config("search_notes", false);
        let output = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 5
            }),
            &runtime,
        )
        .expect("should dispatch");

        assert_eq!(output.tool_name, SEARCH_NOTES_TOOL_NAME);
        assert_eq!(
            output.payload,
            json!({
                "query": "rust",
                "limit": 5,
                "results": []
            })
        );
    }

    #[test]
    fn dispatch_fetch_url_returns_structured_payload() {
        let output = block_on(run_fetch_url_with_fetcher(
            FetchUrlArgs {
                url: "https://example.com".to_owned(),
            },
            &test_allowlist(),
            5_000,
            100_000,
            |_url, _timeout_ms, _max_bytes| async {
                Ok(FetchResponse {
                    final_url: "https://example.com/".to_owned(),
                    status_code: 200,
                    content_type: Some("text/plain".to_owned()),
                    body: b"hello".to_vec(),
                })
            },
        ))
        .expect("fetch should succeed");

        assert_eq!(output.get("status_code"), Some(&json!(200)));
        assert_eq!(output.get("content_type"), Some(&json!("text/plain")));
        assert_eq!(output.get("bytes"), Some(&json!(5)));
        assert_eq!(output.get("content"), Some(&json!("hello")));
    }

    #[test]
    fn dispatch_save_note_returns_structured_payload() {
        let runtime = test_runtime_config("save_note_create", false);
        cleanup_dir(&runtime.notes_dir);

        let output = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "hello"
            }),
            &runtime,
        )
        .expect("should dispatch");

        assert_eq!(output.tool_name, SAVE_NOTE_TOOL_NAME);
        assert_eq!(output.payload.get("title"), Some(&json!("daily note")));
        assert_eq!(output.payload.get("status"), Some(&json!("created")));

        let path = output
            .payload
            .get("path")
            .and_then(|value| value.as_str())
            .expect("path should be present");
        assert!(path.ends_with("daily-note.md"));

        let file_contents = fs::read_to_string(path).expect("note should be written");
        assert!(file_contents.contains("hello"));

        cleanup_dir(&runtime.notes_dir);
    }

    #[test]
    fn dispatch_save_note_blocks_overwrite_without_confirmation() {
        let runtime = test_runtime_config("save_note_overwrite_blocked", false);
        cleanup_dir(&runtime.notes_dir);

        dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "version one"
            }),
            &runtime,
        )
        .expect("initial write should succeed");

        let error = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "version two"
            }),
            &runtime,
        )
        .expect_err("overwrite should be blocked");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("SAVE_NOTE_ALLOW_OVERWRITE"));

        let note_path = runtime.notes_dir.join("daily-note.md");
        let file_contents = fs::read_to_string(note_path).expect("note should exist");
        assert!(file_contents.contains("version one"));

        cleanup_dir(&runtime.notes_dir);
    }

    #[test]
    fn dispatch_save_note_allows_overwrite_when_confirmed() {
        let runtime = test_runtime_config("save_note_overwrite_allowed", true);
        cleanup_dir(&runtime.notes_dir);

        dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "version one"
            }),
            &runtime,
        )
        .expect("initial write should succeed");

        let output = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "version two"
            }),
            &runtime,
        )
        .expect("overwrite should succeed");

        assert_eq!(output.payload.get("status"), Some(&json!("overwritten")));

        let note_path = runtime.notes_dir.join("daily-note.md");
        let file_contents = fs::read_to_string(note_path).expect("note should exist");
        assert!(file_contents.contains("version two"));

        cleanup_dir(&runtime.notes_dir);
    }

    #[test]
    fn dispatch_save_note_rejects_title_without_alphanumeric_characters() {
        let runtime = test_runtime_config("save_note_bad_title", false);
        let error = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "!!!",
                "body": "hello"
            }),
            &runtime,
        )
        .expect_err("invalid title should fail");

        let ToolDispatchError::InvalidArgs { reason, .. } = error else {
            panic!("expected invalid args");
        };
        assert!(reason.contains("alphanumeric"));
    }

    #[test]
    fn normalize_note_title_converts_text_to_safe_slug() {
        let slug = normalize_note_title("  Daily_Note: Rust v1  ").expect("title should normalize");
        assert_eq!(slug, "daily-note-rust-v1");
    }

    #[test]
    fn dispatch_rejects_unknown_fields_for_search_notes() {
        let runtime = test_runtime_config("search_notes_unknown_fields", false);
        let error = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 3,
                "extra": true
            }),
            &runtime,
        )
        .expect_err("unknown fields should be rejected");

        let ToolDispatchError::InvalidArgs { reason, .. } = error else {
            panic!("expected invalid args error");
        };
        assert!(reason.contains("unknown field"));
        assert!(reason.contains("extra"));
    }

    #[test]
    fn dispatch_rejects_invalid_arg_types() {
        let runtime = test_runtime_config("save_note_invalid_types", false);
        let error = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "note",
                "body": 123
            }),
            &runtime,
        )
        .expect_err("type mismatch should fail");

        let ToolDispatchError::InvalidArgs { reason, .. } = error else {
            panic!("expected invalid args error");
        };
        assert!(reason.contains("invalid type"));
    }

    #[test]
    fn host_allowed_allows_subdomains_in_allowlist() {
        assert!(host_allowed("docs.example.com", &test_allowlist()));
    }

    #[test]
    fn dispatch_fetch_url_rejects_disallowed_domain() {
        let runtime = test_runtime_config("fetch_url_disallowed", false);
        let error = dispatch_tool_call(
            FETCH_URL_TOOL_NAME,
            json!({
                "url": "https://evil.example.net"
            }),
            &runtime,
        )
        .expect_err("disallowed host should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("not in allowlist"));
    }

    #[test]
    fn dispatch_fetch_url_rejects_non_http_schemes() {
        let runtime = test_runtime_config("fetch_url_scheme", false);
        let error = dispatch_tool_call(
            FETCH_URL_TOOL_NAME,
            json!({
                "url": "ftp://example.com/file.txt"
            }),
            &runtime,
        )
        .expect_err("non-http scheme should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("scheme"));
    }

    #[test]
    fn dispatch_fetch_url_rejects_unsupported_content_type() {
        let error = block_on(run_fetch_url_with_fetcher(
            FetchUrlArgs {
                url: "https://example.com".to_owned(),
            },
            &test_allowlist(),
            5_000,
            100_000,
            |_url, _timeout_ms, _max_bytes| async {
                Ok(FetchResponse {
                    final_url: "https://example.com/".to_owned(),
                    status_code: 200,
                    content_type: Some("application/octet-stream".to_owned()),
                    body: b"hello".to_vec(),
                })
            },
        ))
        .expect_err("unsupported content type should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("content type"));
    }

    #[test]
    fn dispatch_fetch_url_rejects_oversized_response_body() {
        let error = block_on(run_fetch_url_with_fetcher(
            FetchUrlArgs {
                url: "https://example.com".to_owned(),
            },
            &test_allowlist(),
            5_000,
            4,
            |_url, _timeout_ms, _max_bytes| async {
                Ok(FetchResponse {
                    final_url: "https://example.com/".to_owned(),
                    status_code: 200,
                    content_type: Some("text/plain".to_owned()),
                    body: b"hello".to_vec(),
                })
            },
        ))
        .expect_err("oversized body should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("FETCH_URL_MAX_BYTES"));
    }

    fn test_allowlist() -> Vec<String> {
        vec!["example.com".to_owned(), "docs.rs".to_owned()]
    }

    fn test_runtime_config(test_name: &str, save_note_allow_overwrite: bool) -> ToolRuntimeConfig {
        ToolRuntimeConfig::new(
            test_allowlist(),
            temp_notes_dir(test_name),
            save_note_allow_overwrite,
            5_000,
            100_000,
        )
    }

    fn temp_notes_dir(test_name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        dir.push(format!(
            "mjolne_vibes_tools_{test_name}_{}_{}",
            std::process::id(),
            now_ns
        ));
        dir
    }

    fn cleanup_dir(path: &PathBuf) {
        let _ = fs::remove_dir_all(path);
    }
}
