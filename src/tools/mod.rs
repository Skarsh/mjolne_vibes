use std::fs;
use std::path::{Path, PathBuf};

use reqwest::Url;
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
}

impl ToolRuntimeConfig {
    pub fn new(
        fetch_url_allowed_domains: Vec<String>,
        notes_dir: PathBuf,
        save_note_allow_overwrite: bool,
    ) -> Self {
        Self {
            fetch_url_allowed_domains,
            notes_dir,
            save_note_allow_overwrite,
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

pub fn dispatch_tool_call(
    tool_name: &str,
    raw_args: Value,
    runtime: &ToolRuntimeConfig,
) -> Result<ToolDispatchOutput, ToolDispatchError> {
    let payload = match tool_name {
        SEARCH_NOTES_TOOL_NAME => Ok(run_search_notes(parse_args(tool_name, raw_args)?)),
        FETCH_URL_TOOL_NAME => run_fetch_url(
            parse_args(tool_name, raw_args)?,
            &runtime.fetch_url_allowed_domains,
        ),
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

fn run_fetch_url(
    args: FetchUrlArgs,
    fetch_url_allowed_domains: &[String],
) -> Result<Value, ToolDispatchError> {
    let parsed = Url::parse(&args.url).map_err(|error| ToolDispatchError::InvalidArgs {
        tool_name: FETCH_URL_TOOL_NAME.to_owned(),
        reason: format!("invalid url `{}`: {error}", args.url),
    })?;

    let host = parsed
        .host_str()
        .ok_or_else(|| ToolDispatchError::InvalidArgs {
            tool_name: FETCH_URL_TOOL_NAME.to_owned(),
            reason: format!("url `{}` must include a host", args.url),
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

    Ok(json!({
        "url": args.url,
        "content": null,
        "status": "stubbed_in_phase_2"
    }))
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::{
        FETCH_URL_TOOL_NAME, SAVE_NOTE_TOOL_NAME, SEARCH_NOTES_TOOL_NAME, ToolDispatchError,
        ToolRuntimeConfig, dispatch_tool_call, normalize_note_title, tool_definitions,
    };

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
        let runtime = test_runtime_config("fetch_url_allowed", false);
        let output = dispatch_tool_call(
            FETCH_URL_TOOL_NAME,
            json!({
                "url": "https://example.com"
            }),
            &runtime,
        )
        .expect("should dispatch");

        assert_eq!(output.tool_name, FETCH_URL_TOOL_NAME);
        assert_eq!(
            output.payload,
            json!({
                "url": "https://example.com",
                "content": null,
                "status": "stubbed_in_phase_2"
            })
        );
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
    fn dispatch_fetch_url_allows_subdomains_in_allowlist() {
        let runtime = test_runtime_config("fetch_url_subdomain", false);
        let output = dispatch_tool_call(
            FETCH_URL_TOOL_NAME,
            json!({
                "url": "https://docs.example.com/page"
            }),
            &runtime,
        )
        .expect("allowed subdomain should dispatch");

        assert_eq!(output.tool_name, FETCH_URL_TOOL_NAME);
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

    fn test_allowlist() -> Vec<String> {
        vec!["example.com".to_owned(), "docs.rs".to_owned()]
    }

    fn test_runtime_config(test_name: &str, save_note_allow_overwrite: bool) -> ToolRuntimeConfig {
        ToolRuntimeConfig::new(
            test_allowlist(),
            temp_notes_dir(test_name),
            save_note_allow_overwrite,
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
