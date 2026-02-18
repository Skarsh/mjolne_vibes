use anyhow::{Context, Result, anyhow};
use serde_json::json;
use tracing::{info, warn};

use crate::config::AgentSettings;
use crate::model::client::{
    ChatResponse, ModelClient, ModelMessage, ModelToolCall, ModelToolDefinition,
};
use crate::tools::{
    FETCH_URL_TOOL_NAME, SAVE_NOTE_TOOL_NAME, SEARCH_NOTES_TOOL_NAME, dispatch_tool_call,
    tool_definitions,
};

const SYSTEM_PROMPT: &str = "You are a concise, reliable Rust AI assistant. Be helpful, truthful, and use tools when needed.";

pub async fn run_chat(settings: &AgentSettings, message: &str) -> Result<()> {
    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        model_timeout_ms = settings.model_timeout_ms,
        model_max_retries = settings.model_max_retries,
        max_steps = settings.max_steps,
        "executing phase-2 chat loop"
    );

    let client = ModelClient::new(settings.clone());
    let tools = build_model_tool_definitions();

    let mut conversation = vec![
        ModelMessage::system(SYSTEM_PROMPT),
        ModelMessage::user(message),
    ];

    for step in 1..=settings.max_steps {
        let response = client
            .chat_with_messages(&conversation, &tools)
            .await
            .with_context(|| {
                format!(
                    "model chat failed for provider {} at step {step}",
                    settings.model_provider
                )
            })?;

        match response {
            ChatResponse::FinalText { text } => {
                println!("{text}");
                return Ok(());
            }
            ChatResponse::ToolCalls {
                assistant_content,
                calls,
            } => {
                info!(
                    step,
                    tool_call_count = calls.len(),
                    "model requested tool calls"
                );

                if calls.is_empty() {
                    return Err(anyhow!(
                        "model returned an empty tool call list at step {step}"
                    ));
                }

                conversation.push(ModelMessage::assistant_tool_calls(
                    assistant_content.unwrap_or_default(),
                    calls.clone(),
                ));
                append_tool_results(&mut conversation, calls);
            }
        }
    }

    Err(anyhow!(
        "agent stopped after reaching max_steps={} without final text response",
        settings.max_steps
    ))
}

fn build_model_tool_definitions() -> Vec<ModelToolDefinition> {
    tool_definitions()
        .iter()
        .map(|tool| ModelToolDefinition {
            name: tool.name.to_owned(),
            description: tool_description(tool.name).to_owned(),
            parameters: tool_parameters_schema(tool.name),
        })
        .collect()
}

fn append_tool_results(messages: &mut Vec<ModelMessage>, calls: Vec<ModelToolCall>) {
    for call in calls {
        let tool_name = call.name.clone();
        let tool_call_id = call.id.clone();

        let content = match dispatch_tool_call(&tool_name, call.arguments) {
            Ok(output) => output.payload.to_string(),
            Err(error) => {
                warn!(
                    tool_name = %tool_name,
                    tool_call_id = %tool_call_id,
                    error = %error,
                    "tool dispatch failed"
                );

                json!({
                    "error": error.to_string()
                })
                .to_string()
            }
        };

        messages.push(ModelMessage::tool_result(
            content,
            Some(tool_call_id),
            Some(tool_name),
        ));
    }
}

fn tool_description(tool_name: &str) -> &'static str {
    match tool_name {
        SEARCH_NOTES_TOOL_NAME => "Search local notes by text query.",
        FETCH_URL_TOOL_NAME => "Fetch a URL and return extracted page content.",
        SAVE_NOTE_TOOL_NAME => "Save a note with a title and body.",
        _ => "Unknown tool.",
    }
}

fn tool_parameters_schema(tool_name: &str) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::build_model_tool_definitions;
    use crate::tools::{FETCH_URL_TOOL_NAME, SAVE_NOTE_TOOL_NAME, SEARCH_NOTES_TOOL_NAME};

    #[test]
    fn model_tool_definitions_match_v1_contract() {
        let defs = build_model_tool_definitions();

        let names: Vec<_> = defs.iter().map(|def| def.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                SEARCH_NOTES_TOOL_NAME,
                FETCH_URL_TOOL_NAME,
                SAVE_NOTE_TOOL_NAME
            ]
        );

        for def in defs {
            assert!(
                def.parameters
                    .get("additionalProperties")
                    .and_then(|v| v.as_bool())
                    == Some(false)
            );
        }
    }
}
