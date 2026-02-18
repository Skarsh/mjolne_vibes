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

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolDispatchError {
    #[error("unknown tool `{tool_name}`")]
    UnknownTool { tool_name: String },

    #[error("invalid args for tool `{tool_name}`: {reason}")]
    InvalidArgs { tool_name: String, reason: String },
}

pub fn dispatch_tool_call(
    tool_name: &str,
    raw_args: Value,
) -> Result<ToolDispatchOutput, ToolDispatchError> {
    let payload = match tool_name {
        SEARCH_NOTES_TOOL_NAME => run_search_notes(parse_args(tool_name, raw_args)?),
        FETCH_URL_TOOL_NAME => run_fetch_url(parse_args(tool_name, raw_args)?),
        SAVE_NOTE_TOOL_NAME => run_save_note(parse_args(tool_name, raw_args)?),
        _ => {
            return Err(ToolDispatchError::UnknownTool {
                tool_name: tool_name.to_owned(),
            });
        }
    };

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

fn run_fetch_url(args: FetchUrlArgs) -> Value {
    json!({
        "url": args.url,
        "content": null,
        "status": "stubbed_in_phase_2"
    })
}

fn run_save_note(args: SaveNoteArgs) -> Value {
    json!({
        "title": args.title,
        "bytes": args.body.len(),
        "status": "stubbed_in_phase_2"
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        FETCH_URL_TOOL_NAME, SAVE_NOTE_TOOL_NAME, SEARCH_NOTES_TOOL_NAME, ToolDispatchError,
        dispatch_tool_call, tool_definitions,
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
        let error = dispatch_tool_call("unknown_tool", json!({})).expect_err("should fail");
        assert_eq!(
            error,
            ToolDispatchError::UnknownTool {
                tool_name: "unknown_tool".to_owned()
            }
        );
    }

    #[test]
    fn dispatch_search_notes_returns_structured_payload() {
        let output = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 5
            }),
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
        let output = dispatch_tool_call(
            FETCH_URL_TOOL_NAME,
            json!({
                "url": "https://example.com"
            }),
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
        let output = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "daily note",
                "body": "hello"
            }),
        )
        .expect("should dispatch");

        assert_eq!(output.tool_name, SAVE_NOTE_TOOL_NAME);
        assert_eq!(
            output.payload,
            json!({
                "title": "daily note",
                "bytes": 5,
                "status": "stubbed_in_phase_2"
            })
        );
    }

    #[test]
    fn dispatch_rejects_unknown_fields_for_search_notes() {
        let error = dispatch_tool_call(
            SEARCH_NOTES_TOOL_NAME,
            json!({
                "query": "rust",
                "limit": 3,
                "extra": true
            }),
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
        let error = dispatch_tool_call(
            SAVE_NOTE_TOOL_NAME,
            json!({
                "title": "note",
                "body": 123
            }),
        )
        .expect_err("type mismatch should fail");

        let ToolDispatchError::InvalidArgs { reason, .. } = error else {
            panic!("expected invalid args error");
        };
        assert!(reason.contains("invalid type"));
    }
}
