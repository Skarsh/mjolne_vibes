use std::error::Error as StdError;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use reqwest::header::{CONTENT_TYPE, HeaderMap, LOCATION};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SEARCH_NOTES_TOOL_NAME: &str = "search_notes";
pub const FETCH_URL_TOOL_NAME: &str = "fetch_url";
pub const SAVE_NOTE_TOOL_NAME: &str = "save_note";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub signature: &'static str,
    pub description: &'static str,
}

const TOOL_DEFINITIONS: [ToolDefinition; 3] = [
    ToolDefinition {
        name: SEARCH_NOTES_TOOL_NAME,
        signature: "search_notes(query: string, limit: u8)",
        description: "Search local notes by text query.",
    },
    ToolDefinition {
        name: FETCH_URL_TOOL_NAME,
        signature: "fetch_url(url: string)",
        description: "Fetch a URL and return extracted page content.",
    },
    ToolDefinition {
        name: SAVE_NOTE_TOOL_NAME,
        signature: "save_note(title: string, body: string)",
        description: "Save a note with a title and body.",
    },
];

pub fn tool_definitions() -> &'static [ToolDefinition] {
    &TOOL_DEFINITIONS
}

pub fn tool_parameters_schema(tool_name: &str) -> Value {
    match tool_name {
        SEARCH_NOTES_TOOL_NAME => json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 0, "maximum": 255}
            },
            "required": ["query", "limit"],
            "additionalProperties": false
        }),
        FETCH_URL_TOOL_NAME => json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"}
            },
            "required": ["url"],
            "additionalProperties": false
        }),
        SAVE_NOTE_TOOL_NAME => json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"}
            },
            "required": ["title", "body"],
            "additionalProperties": false
        }),
        _ => json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
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
    pub fetch_url_follow_redirects: bool,
}

impl ToolRuntimeConfig {
    pub fn new(
        fetch_url_allowed_domains: Vec<String>,
        notes_dir: PathBuf,
        save_note_allow_overwrite: bool,
        tool_timeout_ms: u64,
        fetch_url_max_bytes: usize,
        fetch_url_follow_redirects: bool,
    ) -> Self {
        Self {
            fetch_url_allowed_domains,
            notes_dir,
            save_note_allow_overwrite,
            tool_timeout_ms,
            fetch_url_max_bytes,
            fetch_url_follow_redirects,
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
        SEARCH_NOTES_TOOL_NAME => {
            run_search_notes(parse_args(tool_name, raw_args)?, &runtime.notes_dir)
        }
        FETCH_URL_TOOL_NAME => {
            run_fetch_url(
                parse_args(tool_name, raw_args)?,
                &runtime.fetch_url_allowed_domains,
                runtime.tool_timeout_ms,
                runtime.fetch_url_max_bytes,
                runtime.fetch_url_follow_redirects,
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

#[derive(Debug)]
struct SearchNoteMatch {
    title: String,
    path: String,
    score: u32,
    snippet: String,
}

fn run_search_notes(args: SearchNotesArgs, notes_dir: &Path) -> Result<Value, ToolDispatchError> {
    let query = args.query.trim();
    if query.is_empty() {
        return Err(ToolDispatchError::InvalidArgs {
            tool_name: SEARCH_NOTES_TOOL_NAME.to_owned(),
            reason: "query cannot be empty".to_owned(),
        });
    }

    let limit = args.limit as usize;
    if limit == 0 {
        return Ok(json!({
            "query": query,
            "limit": args.limit,
            "total_matches": 0,
            "results": []
        }));
    }

    let query_lower = query.to_ascii_lowercase();
    let mut matches = Vec::new();

    for path in list_searchable_note_paths(notes_dir)? {
        let raw = fs::read(&path).map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: SEARCH_NOTES_TOOL_NAME.to_owned(),
            reason: format!("failed to read note `{}`: {error}", path.display()),
        })?;
        let content = String::from_utf8_lossy(&raw).to_string();
        let title = extract_note_title(&content, &path);
        let score = count_occurrences_case_insensitive(&title, &query_lower)
            .saturating_mul(2)
            .saturating_add(count_occurrences_case_insensitive(&content, &query_lower));
        if score == 0 {
            continue;
        }

        matches.push(SearchNoteMatch {
            title,
            path: path.display().to_string(),
            score,
            snippet: extract_note_snippet(&content, &query_lower),
        });
    }

    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.path.cmp(&right.path))
    });
    let total_matches = matches.len();
    matches.truncate(limit);

    Ok(json!({
        "query": query,
        "limit": args.limit,
        "total_matches": total_matches,
        "results": matches.into_iter().map(|matched| {
            json!({
                "title": matched.title,
                "path": matched.path,
                "score": matched.score,
                "snippet": matched.snippet,
            })
        }).collect::<Vec<_>>()
    }))
}

fn list_searchable_note_paths(notes_dir: &Path) -> Result<Vec<PathBuf>, ToolDispatchError> {
    if !notes_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    let entries = fs::read_dir(notes_dir).map_err(|error| ToolDispatchError::ExecutionFailed {
        tool_name: SEARCH_NOTES_TOOL_NAME.to_owned(),
        reason: format!(
            "failed to read notes directory `{}`: {error}",
            notes_dir.display()
        ),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: SEARCH_NOTES_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to list entry in notes directory `{}`: {error}",
                notes_dir.display()
            ),
        })?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| ToolDispatchError::ExecutionFailed {
                tool_name: SEARCH_NOTES_TOOL_NAME.to_owned(),
                reason: format!("failed to inspect note path `{}`: {error}", path.display()),
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        if !is_searchable_note_extension(&path) {
            continue;
        }
        paths.push(path);
    }

    paths.sort();
    Ok(paths)
}

fn is_searchable_note_extension(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    let normalized = extension.to_ascii_lowercase();
    normalized == "md" || normalized == "markdown" || normalized == "txt"
}

fn extract_note_title(content: &str, path: &Path) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("# ") {
            let title = stripped.trim();
            if !title.is_empty() {
                return title.to_owned();
            }
        }
    }

    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "untitled".to_owned())
}

fn extract_note_snippet(content: &str, query_lower: &str) -> String {
    let mut fallback: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(trimmed.to_owned());
        }
        if trimmed.to_ascii_lowercase().contains(query_lower) {
            return truncate_chars(trimmed, 160);
        }
    }

    fallback
        .map(|value| truncate_chars(&value, 160))
        .unwrap_or_default()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect()
}

fn count_occurrences_case_insensitive(haystack: &str, needle_lower: &str) -> u32 {
    if needle_lower.is_empty() {
        return 0;
    }

    let haystack_lower = haystack.to_ascii_lowercase();
    let mut count = 0_u32;
    let mut offset = 0_usize;
    while let Some(index) = haystack_lower[offset..].find(needle_lower) {
        count = count.saturating_add(1);
        offset = offset.saturating_add(index + needle_lower.len());
    }

    count
}

async fn run_fetch_url(
    args: FetchUrlArgs,
    fetch_url_allowed_domains: &[String],
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
    fetch_url_follow_redirects: bool,
) -> Result<Value, ToolDispatchError> {
    run_fetch_url_with_fetcher(
        args,
        fetch_url_allowed_domains,
        fetch_url_follow_redirects,
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
    fetch_url_follow_redirects: bool,
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
    fetcher: F,
) -> Result<Value, ToolDispatchError>
where
    F: Fn(Url, Vec<String>, bool, u64, usize) -> Fut,
    Fut: std::future::Future<Output = Result<FetchResponse, ToolDispatchError>>,
{
    let parsed = parse_fetch_url(&args.url, fetch_url_allowed_domains)?;
    let fetched = fetcher(
        parsed,
        fetch_url_allowed_domains.to_vec(),
        fetch_url_follow_redirects,
        tool_timeout_ms,
        fetch_url_max_bytes,
    )
    .await?;

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
    fetch_url_allowed_domains: Vec<String>,
    fetch_url_follow_redirects: bool,
    tool_timeout_ms: u64,
    fetch_url_max_bytes: usize,
) -> Result<FetchResponse, ToolDispatchError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(tool_timeout_ms))
        // Redirects are handled explicitly below so we can enforce allowlist policy per hop.
        .redirect(Policy::none())
        .build()
        .map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to build HTTP client: {}",
                describe_reqwest_error(&error)
            ),
        })?;

    let mut current_url = parsed_url.clone();
    let mut redirects_followed = 0_usize;
    loop {
        let response = client
            .get(current_url.clone())
            .send()
            .await
            .map_err(|error| ToolDispatchError::ExecutionFailed {
                tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                reason: format!(
                    "failed to fetch `{current_url}`: {}",
                    describe_reqwest_error(&error)
                ),
            })?;

        if fetch_url_follow_redirects && response.status().is_redirection() {
            if redirects_followed >= 10 {
                return Err(ToolDispatchError::ExecutionFailed {
                    tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                    reason: format!(
                        "redirect chain exceeded maximum of 10 hops for `{parsed_url}`"
                    ),
                });
            }

            current_url = resolve_redirect_target(
                &current_url,
                response.headers(),
                &fetch_url_allowed_domains,
            )?;
            redirects_followed = redirects_followed.saturating_add(1);
            continue;
        }

        let status_code = response.status().as_u16();
        let content_type = extract_content_type(response.headers())?;
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) =
            response
                .chunk()
                .await
                .map_err(|error| ToolDispatchError::ExecutionFailed {
                    tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                    reason: format!(
                        "failed to read response body from `{current_url}`: {}",
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

        return Ok(FetchResponse {
            final_url: current_url.as_str().to_owned(),
            status_code,
            content_type,
            body,
        });
    }
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

fn resolve_redirect_target(
    current_url: &Url,
    headers: &HeaderMap,
    fetch_url_allowed_domains: &[String],
) -> Result<Url, ToolDispatchError> {
    let location = headers
        .get(LOCATION)
        .ok_or_else(|| ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("received redirect response without Location header for `{current_url}`"),
        })?
        .to_str()
        .map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "received redirect response with invalid Location header for `{current_url}`: {error}"
            ),
        })?;

    let target =
        current_url
            .join(location)
            .map_err(|error| ToolDispatchError::ExecutionFailed {
                tool_name: FETCH_URL_TOOL_NAME.to_owned(),
                reason: format!(
                    "failed to resolve redirect target `{location}` from `{current_url}`: {error}"
                ),
            })?;

    if target.scheme() != "http" && target.scheme() != "https" {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!(
                "redirect target scheme `{}` is not allowed",
                target.scheme()
            ),
        });
    }

    let host = target
        .host_str()
        .ok_or_else(|| ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("redirect target `{target}` must include a host"),
        })?
        .to_ascii_lowercase();

    if !host_allowed(&host, fetch_url_allowed_domains) {
        return Err(ToolDispatchError::PolicyViolation {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("redirect target host `{host}` is not in allowlist"),
        });
    }

    Ok(target)
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
    fs::create_dir_all(notes_dir).map_err(|error| ToolDispatchError::ExecutionFailed {
        tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
        reason: format!(
            "failed to create notes directory `{}`: {error}",
            notes_dir.display()
        ),
    })?;

    let note_filename = format!("{note_slug}.md");
    let note_path = notes_dir.join(&note_filename);
    let existing_metadata = fs::symlink_metadata(&note_path)
        .map(Some)
        .or_else(|error| match error.kind() {
            ErrorKind::NotFound => Ok(None),
            _ => Err(error),
        });
    let existing_metadata =
        existing_metadata.map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to inspect existing note `{}`: {error}",
                note_path.display()
            ),
        })?;

    if let Some(metadata) = existing_metadata.as_ref() {
        if metadata.file_type().is_symlink() {
            return Err(ToolDispatchError::PolicyViolation {
                tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
                reason: format!(
                    "refusing to write note `{}` because target is a symlink",
                    note_path.display()
                ),
            });
        }

        if !metadata.is_file() {
            return Err(ToolDispatchError::PolicyViolation {
                tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
                reason: format!(
                    "refusing to overwrite non-file note path `{}`",
                    note_path.display()
                ),
            });
        }

        if !save_note_allow_overwrite {
            return Err(ToolDispatchError::PolicyViolation {
                tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
                reason: format!(
                    "refusing to overwrite existing note `{}` without confirmation; set SAVE_NOTE_ALLOW_OVERWRITE=true to confirm overwrite",
                    note_path.display()
                ),
            });
        }
    }

    let file_content = format!("# {title}\n\n{}\n", args.body);
    let temp_path = create_temp_note_path(notes_dir, &note_slug);
    write_new_file(&temp_path, &file_content).map_err(|error| {
        ToolDispatchError::ExecutionFailed {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to write temp note file `{}`: {error}",
                temp_path.display()
            ),
        }
    })?;

    if existing_metadata.is_some() {
        fs::remove_file(&note_path).map_err(|error| ToolDispatchError::ExecutionFailed {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to remove existing note `{}` before overwrite: {error}",
                note_path.display()
            ),
        })?;
    }

    fs::rename(&temp_path, &note_path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        ToolDispatchError::ExecutionFailed {
            tool_name: SAVE_NOTE_TOOL_NAME.to_owned(),
            reason: format!(
                "failed to move temp note `{}` into `{}`: {error}",
                temp_path.display(),
                note_path.display()
            ),
        }
    })?;

    Ok(json!({
        "title": title,
        "path": note_path.display().to_string(),
        "bytes": file_content.len(),
        "status": if existing_metadata.is_some() { "overwritten" } else { "created" }
    }))
}

fn create_temp_note_path(notes_dir: &Path, note_slug: &str) -> PathBuf {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    notes_dir.join(format!(
        ".tmp-{note_slug}-{}-{now_ns}.mdtmp",
        std::process::id()
    ))
}

fn write_new_file(path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(content.as_bytes())
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

    use reqwest::Url;
    use reqwest::header::{HeaderMap, HeaderValue, LOCATION};
    use serde_json::{Value, json};

    use super::{
        FETCH_URL_TOOL_NAME, FetchResponse, FetchUrlArgs, SAVE_NOTE_TOOL_NAME,
        SEARCH_NOTES_TOOL_NAME, ToolDispatchError, ToolDispatchOutput, ToolRuntimeConfig,
        dispatch_tool_call as dispatch_tool_call_async, host_allowed, normalize_note_title,
        resolve_redirect_target, run_fetch_url_with_fetcher, tool_definitions,
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
        let definitions = tool_definitions();
        let names: Vec<_> = definitions.iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            vec![
                SEARCH_NOTES_TOOL_NAME,
                FETCH_URL_TOOL_NAME,
                SAVE_NOTE_TOOL_NAME
            ]
        );

        assert_eq!(
            definitions[0].signature,
            "search_notes(query: string, limit: u8)"
        );
        assert_eq!(
            definitions[0].description,
            "Search local notes by text query."
        );
        assert_eq!(definitions[1].signature, "fetch_url(url: string)");
        assert_eq!(
            definitions[1].description,
            "Fetch a URL and return extracted page content."
        );
        assert_eq!(
            definitions[2].signature,
            "save_note(title: string, body: string)"
        );
        assert_eq!(
            definitions[2].description,
            "Save a note with a title and body."
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
    fn dispatch_search_notes_returns_ranked_results_with_limit() {
        let runtime = test_runtime_config("search_notes_ranked", false);
        cleanup_dir(&runtime.notes_dir);
        fs::create_dir_all(&runtime.notes_dir).expect("notes dir should be creatable");
        fs::write(
            runtime.notes_dir.join("rust-guide.md"),
            "# Rust Guide\n\nRust ownership and memory safety.\nRust performance details.\n",
        )
        .expect("note should be writable");
        fs::write(
            runtime.notes_dir.join("async-tips.md"),
            "# Async Tips\n\nTokio helps with rust async workflows.\n",
        )
        .expect("note should be writable");
        fs::write(
            runtime.notes_dir.join("other.md"),
            "# Other\n\nNo matches here.\n",
        )
        .expect("note should be writable");

        let output = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 2
            }),
            &runtime,
        )
        .expect("should dispatch");

        assert_eq!(output.tool_name, SEARCH_NOTES_TOOL_NAME);
        assert_eq!(output.payload.get("query"), Some(&json!("rust")));
        assert_eq!(output.payload.get("limit"), Some(&json!(2)));
        assert_eq!(output.payload.get("total_matches"), Some(&json!(2)));

        let results = output
            .payload
            .get("results")
            .and_then(|value| value.as_array())
            .expect("results should be an array");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].get("title"), Some(&json!("Rust Guide")));
        assert_eq!(results[1].get("title"), Some(&json!("Async Tips")));
        assert!(
            results[0]
                .get("score")
                .and_then(|value| value.as_u64())
                .expect("score should be u64")
                >= results[1]
                    .get("score")
                    .and_then(|value| value.as_u64())
                    .expect("score should be u64")
        );

        cleanup_dir(&runtime.notes_dir);
    }

    #[test]
    fn dispatch_search_notes_returns_empty_when_notes_dir_is_missing() {
        let runtime = test_runtime_config("search_notes_missing_dir", false);
        cleanup_dir(&runtime.notes_dir);

        let output = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 3
            }),
            &runtime,
        )
        .expect("should dispatch");

        assert_eq!(output.payload.get("total_matches"), Some(&json!(0)));
        assert_eq!(output.payload.get("results"), Some(&json!([])));
    }

    #[test]
    fn dispatch_search_notes_rejects_empty_query() {
        let runtime = test_runtime_config("search_notes_empty_query", false);
        let error = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "   ",
                "limit": 3
            }),
            &runtime,
        )
        .expect_err("empty query should fail");

        let ToolDispatchError::InvalidArgs { reason, .. } = error else {
            panic!("expected invalid args error");
        };
        assert!(reason.contains("query cannot be empty"));
    }

    #[test]
    fn dispatch_fetch_url_returns_structured_payload() {
        let output = block_on(run_fetch_url_with_fetcher(
            FetchUrlArgs {
                url: "https://example.com".to_owned(),
            },
            &test_allowlist(),
            false,
            5_000,
            100_000,
            |_url, _allowlist, _follow_redirects, _timeout_ms, _max_bytes| async {
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

    #[cfg(unix)]
    #[test]
    fn dispatch_save_note_rejects_symlink_note_path() {
        use std::os::unix::fs::symlink;

        let runtime = test_runtime_config("save_note_symlink_blocked", true);
        cleanup_dir(&runtime.notes_dir);
        fs::create_dir_all(&runtime.notes_dir).expect("notes dir should be creatable");

        let target_dir = temp_notes_dir("save_note_symlink_target");
        cleanup_dir(&target_dir);
        fs::create_dir_all(&target_dir).expect("target dir should be creatable");
        let target_file = target_dir.join("outside.md");
        fs::write(&target_file, "do not overwrite").expect("target file should be writable");

        let symlink_path = runtime.notes_dir.join("daily-note.md");
        symlink(&target_file, &symlink_path).expect("symlink should be creatable");

        let error = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "new body"
            }),
            &runtime,
        )
        .expect_err("symlink path should be rejected");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("symlink"));

        let unchanged = fs::read_to_string(&target_file).expect("target file should remain");
        assert_eq!(unchanged, "do not overwrite");

        cleanup_dir(&runtime.notes_dir);
        cleanup_dir(&target_dir);
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
            false,
            5_000,
            100_000,
            |_url, _allowlist, _follow_redirects, _timeout_ms, _max_bytes| async {
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
            false,
            5_000,
            4,
            |_url, _allowlist, _follow_redirects, _timeout_ms, _max_bytes| async {
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

    #[test]
    fn resolve_redirect_target_allows_relative_location_on_allowlisted_host() {
        let mut headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_static("/docs"));
        let current = Url::parse("https://example.com/start").expect("url should parse");

        let target = resolve_redirect_target(&current, &headers, &test_allowlist())
            .expect("redirect target should resolve");

        assert_eq!(target.as_str(), "https://example.com/docs");
    }

    #[test]
    fn resolve_redirect_target_rejects_disallowed_host() {
        let mut headers = HeaderMap::new();
        headers.insert(
            LOCATION,
            HeaderValue::from_static("https://evil.example.net/redirect"),
        );
        let current = Url::parse("https://example.com/start").expect("url should parse");

        let error = resolve_redirect_target(&current, &headers, &test_allowlist())
            .expect_err("disallowed redirect host should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("redirect target host"));
        assert!(reason.contains("allowlist"));
    }

    #[test]
    fn resolve_redirect_target_rejects_non_http_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_static("ftp://example.com/file"));
        let current = Url::parse("https://example.com/start").expect("url should parse");

        let error = resolve_redirect_target(&current, &headers, &test_allowlist())
            .expect_err("non-http redirect scheme should fail");

        let ToolDispatchError::PolicyViolation { reason, .. } = error else {
            panic!("expected policy violation");
        };
        assert!(reason.contains("redirect target scheme"));
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
            false,
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
