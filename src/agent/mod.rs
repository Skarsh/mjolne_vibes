use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::io::{self, Write};
use std::time::Duration;
use tokio::time::timeout;
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
        max_tool_calls = settings.max_tool_calls,
        tool_timeout_ms = settings.tool_timeout_ms,
        "executing one-shot chat turn"
    );

    let mut session = ChatSession::new(settings);
    let text = session
        .run_turn(message)
        .await
        .context("chat turn failed in one-shot mode")?;
    println!("{text}");
    Ok(())
}

pub async fn run_repl(settings: &AgentSettings) -> Result<()> {
    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        model_timeout_ms = settings.model_timeout_ms,
        model_max_retries = settings.model_max_retries,
        max_steps = settings.max_steps,
        max_tool_calls = settings.max_tool_calls,
        tool_timeout_ms = settings.tool_timeout_ms,
        "starting interactive repl session"
    );

    println!("Interactive mode started. Type /help for commands.");
    let mut session = ChatSession::new(settings);
    let stdin = io::stdin();

    loop {
        print!("> ");
        io::stdout().flush().context("failed to flush prompt")?;

        let mut input = String::new();
        let bytes_read = stdin
            .read_line(&mut input)
            .context("failed to read input line")?;
        if bytes_read == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        match input {
            "/exit" | "/quit" => break,
            "/help" => {
                println!("/help  Show commands");
                println!("/reset Reset session history");
                println!("/exit  Exit interactive mode");
            }
            "/reset" => {
                session.reset();
                println!("Session history cleared.");
            }
            _ => match session.run_turn(input).await {
                Ok(text) => println!("{text}"),
                Err(error) => eprintln!("error: {error}"),
            },
        }
    }

    Ok(())
}

struct ChatSession {
    settings: AgentSettings,
    client: ModelClient,
    tools: Vec<ModelToolDefinition>,
    conversation: Vec<ModelMessage>,
}

impl ChatSession {
    fn new(settings: &AgentSettings) -> Self {
        let settings = settings.clone();
        let client = ModelClient::new(settings.clone());
        let tools = build_model_tool_definitions();
        let conversation = vec![ModelMessage::system(SYSTEM_PROMPT)];

        Self {
            settings,
            client,
            tools,
            conversation,
        }
    }

    fn reset(&mut self) {
        self.conversation = vec![ModelMessage::system(SYSTEM_PROMPT)];
    }

    async fn run_turn(&mut self, message: &str) -> Result<String> {
        self.conversation.push(ModelMessage::user(message));
        let mut total_tool_calls: u32 = 0;

        for step in 1..=self.settings.max_steps {
            let response = self
                .client
                .chat_with_messages(&self.conversation, &self.tools)
                .await
                .with_context(|| {
                    format!(
                        "model chat failed for provider {} at step {step}",
                        self.settings.model_provider
                    )
                })?;

            match response {
                ChatResponse::FinalText { text } => {
                    self.conversation
                        .push(ModelMessage::assistant_text(text.clone()));
                    return Ok(text);
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

                    total_tool_calls = enforce_tool_call_cap(
                        total_tool_calls,
                        calls.len(),
                        self.settings.max_tool_calls,
                        step,
                    )?;

                    self.conversation.push(ModelMessage::assistant_tool_calls(
                        assistant_content.unwrap_or_default(),
                        calls.clone(),
                    ));
                    append_tool_results(
                        &mut self.conversation,
                        calls,
                        self.settings.tool_timeout_ms,
                        &self.settings.fetch_url_allowed_domains,
                    )
                    .await;
                }
            }
        }

        Err(anyhow!(
            "agent stopped after reaching max_steps={} without final text response",
            self.settings.max_steps
        ))
    }
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

fn enforce_tool_call_cap(
    used_calls: u32,
    requested_calls: usize,
    max_tool_calls: u32,
    step: u32,
) -> Result<u32> {
    let requested_calls = u32::try_from(requested_calls)
        .map_err(|_| anyhow!("model requested too many tool calls to track at step {step}"))?;

    let next_total = used_calls.checked_add(requested_calls).ok_or_else(|| {
        anyhow!("tool call counter overflowed while enforcing limits at step {step}")
    })?;

    if next_total > max_tool_calls {
        return Err(anyhow!(
            "tool-call cap exceeded at step {step}: requested {requested_calls} calls, used {used_calls}, limit {max_tool_calls} (AGENT_MAX_TOOL_CALLS)"
        ));
    }

    Ok(next_total)
}

async fn append_tool_results(
    messages: &mut Vec<ModelMessage>,
    calls: Vec<ModelToolCall>,
    tool_timeout_ms: u64,
    fetch_url_allowed_domains: &[String],
) {
    for call in calls {
        let tool_name = call.name.clone();
        let tool_call_id = call.id.clone();
        let content = dispatch_tool_call_with_timeout(
            &tool_name,
            &tool_call_id,
            call.arguments,
            tool_timeout_ms,
            fetch_url_allowed_domains,
        )
        .await;

        messages.push(ModelMessage::tool_result(
            content,
            Some(tool_call_id),
            Some(tool_name),
        ));
    }
}

async fn dispatch_tool_call_with_timeout(
    tool_name: &str,
    tool_call_id: &str,
    raw_args: serde_json::Value,
    tool_timeout_ms: u64,
    fetch_url_allowed_domains: &[String],
) -> String {
    let tool_name_for_task = tool_name.to_owned();
    let allowlist = fetch_url_allowed_domains.to_vec();
    let dispatch_future = tokio::task::spawn_blocking(move || {
        dispatch_tool_call(&tool_name_for_task, raw_args, &allowlist)
    });

    let timeout_result = with_timeout(dispatch_future, tool_timeout_ms).await;
    match timeout_result {
        Ok(Ok(Ok(output))) => output.payload.to_string(),
        Ok(Ok(Err(error))) => {
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
        Ok(Err(join_error)) => {
            warn!(
                tool_name = %tool_name,
                tool_call_id = %tool_call_id,
                error = %join_error,
                "tool execution task failed"
            );

            json!({
                "error": format!("tool `{tool_name}` execution failed: {join_error}")
            })
            .to_string()
        }
        Err(()) => {
            warn!(
                tool_name = %tool_name,
                tool_call_id = %tool_call_id,
                tool_timeout_ms,
                "tool execution timed out"
            );

            json!({
                "error": format!("tool `{tool_name}` timed out after {tool_timeout_ms}ms")
            })
            .to_string()
        }
    }
}

async fn with_timeout<F, T>(future: F, timeout_ms: u64) -> std::result::Result<T, ()>
where
    F: std::future::Future<Output = T>,
{
    timeout(Duration::from_millis(timeout_ms), future)
        .await
        .map_err(|_| ())
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
    use std::time::Duration;

    use super::{build_model_tool_definitions, enforce_tool_call_cap, with_timeout};
    use crate::config::{AgentSettings, ModelProvider};
    use crate::model::client::{MessageRole, ModelMessage};
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

    #[test]
    fn enforce_tool_call_cap_accepts_totals_within_limit() {
        let next_total = enforce_tool_call_cap(2, 3, 8, 4).expect("should stay within cap");
        assert_eq!(next_total, 5);
    }

    #[test]
    fn enforce_tool_call_cap_rejects_over_limit() {
        let error = enforce_tool_call_cap(6, 3, 8, 2).expect_err("should reject cap overrun");
        assert!(error.to_string().contains("tool-call cap exceeded"));
    }

    #[tokio::test]
    async fn with_timeout_returns_value_before_deadline() {
        let value = with_timeout(async { 42_u8 }, 10)
            .await
            .expect("future should complete");
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn with_timeout_errors_when_deadline_expires() {
        let result = with_timeout(
            async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                7_u8
            },
            1,
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn chat_session_starts_with_system_prompt_message() {
        let session = super::ChatSession::new(&test_settings());
        assert_eq!(session.conversation.len(), 1);
        assert_eq!(session.conversation[0].role, MessageRole::System);
        assert_eq!(session.conversation[0].content, super::SYSTEM_PROMPT);
    }

    #[test]
    fn chat_session_reset_clears_turn_history() {
        let mut session = super::ChatSession::new(&test_settings());
        session.conversation.push(ModelMessage::user("hello"));
        session
            .conversation
            .push(ModelMessage::assistant_text("hi"));
        session.reset();

        assert_eq!(session.conversation.len(), 1);
        assert_eq!(session.conversation[0].role, MessageRole::System);
        assert_eq!(session.conversation[0].content, super::SYSTEM_PROMPT);
    }

    fn test_settings() -> AgentSettings {
        AgentSettings {
            model_provider: ModelProvider::Ollama,
            model: "qwen2.5:3b".to_owned(),
            ollama_base_url: "http://localhost:11434".to_owned(),
            openai_api_key: None,
            max_steps: 8,
            max_tool_calls: 8,
            tool_timeout_ms: 5_000,
            fetch_url_allowed_domains: vec!["example.com".to_owned()],
            model_timeout_ms: 20_000,
            model_max_retries: 0,
        }
    }
}
