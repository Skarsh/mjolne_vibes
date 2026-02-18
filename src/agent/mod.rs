use anyhow::{Context, Result, anyhow};
use serde_json::json;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use tracing::{info, warn};

use crate::config::AgentSettings;
use crate::model::client::{
    ChatResponse, ModelClient, ModelMessage, ModelToolCall, ModelToolDefinition,
};
use crate::tools::{
    FETCH_URL_TOOL_NAME, SAVE_NOTE_TOOL_NAME, SEARCH_NOTES_TOOL_NAME, ToolRuntimeConfig,
    dispatch_tool_call, tool_definitions,
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
        max_tool_calls_per_step = settings.max_tool_calls_per_step,
        max_consecutive_tool_steps = settings.max_consecutive_tool_steps,
        max_input_chars = settings.max_input_chars,
        max_output_chars = settings.max_output_chars,
        notes_dir = %settings.notes_dir,
        save_note_allow_overwrite = settings.save_note_allow_overwrite,
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
        max_tool_calls_per_step = settings.max_tool_calls_per_step,
        max_consecutive_tool_steps = settings.max_consecutive_tool_steps,
        max_input_chars = settings.max_input_chars,
        max_output_chars = settings.max_output_chars,
        notes_dir = %settings.notes_dir,
        save_note_allow_overwrite = settings.save_note_allow_overwrite,
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
                for line in repl_help_lines() {
                    println!("{line}");
                }
            }
            "/tools" => {
                for line in build_repl_tools_lines() {
                    println!("{line}");
                }
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
    tool_runtime: ToolRuntimeConfig,
    conversation: Vec<ModelMessage>,
}

#[derive(Debug, Default)]
struct TurnTrace {
    input_chars: usize,
    output_chars: Option<usize>,
    steps_executed: u32,
    model_calls: u32,
    tool_calls: u32,
    total_model_latency: Duration,
    total_tool_latency: Duration,
    tool_names: Vec<String>,
}

impl TurnTrace {
    fn with_input(input: &str) -> Self {
        Self {
            input_chars: input.chars().count(),
            ..Self::default()
        }
    }
}

impl ChatSession {
    fn new(settings: &AgentSettings) -> Self {
        let settings = settings.clone();
        let client = ModelClient::new(settings.clone());
        let tools = build_model_tool_definitions();
        let tool_runtime = ToolRuntimeConfig::new(
            settings.fetch_url_allowed_domains.clone(),
            PathBuf::from(settings.notes_dir.clone()),
            settings.save_note_allow_overwrite,
            settings.tool_timeout_ms,
            settings.fetch_url_max_bytes as usize,
        );
        let conversation = vec![ModelMessage::system(SYSTEM_PROMPT)];

        Self {
            settings,
            client,
            tools,
            tool_runtime,
            conversation,
        }
    }

    fn reset(&mut self) {
        self.conversation = vec![ModelMessage::system(SYSTEM_PROMPT)];
    }

    async fn run_turn(&mut self, message: &str) -> Result<String> {
        let turn_started_at = Instant::now();
        let mut trace = TurnTrace::with_input(message);
        let result = self.run_turn_inner(message, &mut trace).await;
        log_turn_trace(&trace, turn_started_at.elapsed(), result.as_ref().err());
        result
    }

    async fn run_turn_inner(&mut self, message: &str, trace: &mut TurnTrace) -> Result<String> {
        enforce_input_char_limit(message, self.settings.max_input_chars)?;
        self.conversation.push(ModelMessage::user(message));
        let mut total_tool_calls: u32 = 0;
        let mut consecutive_tool_steps: u32 = 0;

        for step in 1..=self.settings.max_steps {
            trace.steps_executed = step;
            let model_call_started_at = Instant::now();
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
            let model_call_latency = model_call_started_at.elapsed();
            trace.model_calls = trace.model_calls.saturating_add(1);
            trace.total_model_latency =
                trace.total_model_latency.saturating_add(model_call_latency);

            match response {
                ChatResponse::FinalText { text } => {
                    enforce_output_char_limit(
                        "assistant final response",
                        &text,
                        self.settings.max_output_chars,
                    )?;
                    trace.output_chars = Some(text.chars().count());
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
                        model_call_latency_ms = model_call_latency.as_millis(),
                        tool_call_count = calls.len(),
                        "model requested tool calls"
                    );

                    if calls.is_empty() {
                        return Err(anyhow!(
                            "model returned an empty tool call list at step {step}"
                        ));
                    }

                    enforce_tool_calls_per_step_cap(
                        calls.len(),
                        self.settings.max_tool_calls_per_step,
                        step,
                    )?;

                    consecutive_tool_steps = enforce_consecutive_tool_step_cap(
                        consecutive_tool_steps,
                        self.settings.max_consecutive_tool_steps,
                        step,
                    )?;

                    total_tool_calls = enforce_tool_call_cap(
                        total_tool_calls,
                        calls.len(),
                        self.settings.max_tool_calls,
                        step,
                    )?;

                    let assistant_content = assistant_content.unwrap_or_default();
                    enforce_output_char_limit(
                        "assistant tool-call content",
                        &assistant_content,
                        self.settings.max_output_chars,
                    )?;

                    self.conversation.push(ModelMessage::assistant_tool_calls(
                        assistant_content,
                        calls.clone(),
                    ));
                    let tool_trace = append_tool_results(
                        &mut self.conversation,
                        calls,
                        step,
                        self.settings.tool_timeout_ms,
                        self.settings.max_output_chars,
                        &self.tool_runtime,
                    )
                    .await
                    .with_context(|| {
                        format!("failed while appending tool results at step {step}")
                    })?;
                    trace.tool_calls = trace.tool_calls.saturating_add(tool_trace.tool_calls);
                    trace.total_tool_latency = trace
                        .total_tool_latency
                        .saturating_add(tool_trace.total_tool_latency);
                    trace.tool_names.extend(tool_trace.tool_names);
                }
            }
        }

        Err(anyhow!(
            "agent stopped after reaching max_steps={} without final text response",
            self.settings.max_steps
        ))
    }
}

#[derive(Debug, Default)]
struct ToolExecutionTrace {
    tool_calls: u32,
    total_tool_latency: Duration,
    tool_names: Vec<String>,
}

fn log_turn_trace(trace: &TurnTrace, turn_latency: Duration, error: Option<&anyhow::Error>) {
    let tool_names_summary = summarize_tool_names(&trace.tool_names);

    match error {
        Some(error) => warn!(
            turn_latency_ms = turn_latency.as_millis(),
            steps_executed = trace.steps_executed,
            model_calls = trace.model_calls,
            tool_calls = trace.tool_calls,
            total_model_latency_ms = trace.total_model_latency.as_millis(),
            total_tool_latency_ms = trace.total_tool_latency.as_millis(),
            input_chars = trace.input_chars,
            output_chars = trace.output_chars.unwrap_or(0),
            tools = %tool_names_summary,
            error = %error,
            "turn trace summary (failed)"
        ),
        None => info!(
            turn_latency_ms = turn_latency.as_millis(),
            steps_executed = trace.steps_executed,
            model_calls = trace.model_calls,
            tool_calls = trace.tool_calls,
            total_model_latency_ms = trace.total_model_latency.as_millis(),
            total_tool_latency_ms = trace.total_tool_latency.as_millis(),
            input_chars = trace.input_chars,
            output_chars = trace.output_chars.unwrap_or(0),
            tools = %tool_names_summary,
            "turn trace summary"
        ),
    }
}

fn summarize_tool_names(tool_names: &[String]) -> String {
    if tool_names.is_empty() {
        return "none".to_owned();
    }

    let mut unique = tool_names.to_vec();
    unique.sort();
    unique.dedup();
    unique.join(",")
}

fn repl_help_lines() -> &'static [&'static str] {
    &[
        "/help   Show commands",
        "/tools  Show available tools",
        "/reset  Reset session history",
        "/exit   Exit interactive mode",
    ]
}

fn build_repl_tools_lines() -> Vec<String> {
    let mut lines = vec!["Available tools:".to_owned()];

    for tool in tool_definitions() {
        lines.push(format!(
            "- {}: {}",
            tool_signature(tool.name),
            tool_description(tool.name)
        ));
    }

    lines
}

fn tool_signature(tool_name: &str) -> &'static str {
    match tool_name {
        SEARCH_NOTES_TOOL_NAME => "search_notes(query: string, limit: u8)",
        FETCH_URL_TOOL_NAME => "fetch_url(url: string)",
        SAVE_NOTE_TOOL_NAME => "save_note(title: string, body: string)",
        _ => "unknown()",
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

fn enforce_consecutive_tool_step_cap(
    consecutive_tool_steps: u32,
    max_consecutive_tool_steps: u32,
    step: u32,
) -> Result<u32> {
    let next_total = consecutive_tool_steps.checked_add(1).ok_or_else(|| {
        anyhow!("consecutive tool-step counter overflowed while enforcing limits at step {step}")
    })?;

    if next_total > max_consecutive_tool_steps {
        return Err(anyhow!(
            "consecutive tool-step cap exceeded at step {step}: consecutive {next_total}, limit {max_consecutive_tool_steps} (AGENT_MAX_CONSECUTIVE_TOOL_STEPS)"
        ));
    }

    Ok(next_total)
}

fn enforce_tool_calls_per_step_cap(
    requested_calls: usize,
    max_tool_calls_per_step: u32,
    step: u32,
) -> Result<()> {
    let requested_calls = u32::try_from(requested_calls).map_err(|_| {
        anyhow!("model requested too many tool calls to track in one step at step {step}")
    })?;

    if requested_calls > max_tool_calls_per_step {
        return Err(anyhow!(
            "tool-calls-per-step cap exceeded at step {step}: requested {requested_calls}, limit {max_tool_calls_per_step} (AGENT_MAX_TOOL_CALLS_PER_STEP)"
        ));
    }

    Ok(())
}

fn enforce_input_char_limit(input: &str, max_input_chars: u32) -> Result<()> {
    enforce_char_limit(
        "user input",
        input,
        max_input_chars,
        "AGENT_MAX_INPUT_CHARS",
    )
}

fn enforce_output_char_limit(output_name: &str, output: &str, max_output_chars: u32) -> Result<()> {
    enforce_char_limit(
        output_name,
        output,
        max_output_chars,
        "AGENT_MAX_OUTPUT_CHARS",
    )
}

fn enforce_char_limit(subject: &str, text: &str, max_chars: u32, env_var: &str) -> Result<()> {
    let text_chars = text.chars().count();
    if text_chars > max_chars as usize {
        return Err(anyhow!(
            "{subject} exceeded {env_var} limit: {text_chars} chars (max {max_chars})"
        ));
    }

    Ok(())
}

async fn append_tool_results(
    messages: &mut Vec<ModelMessage>,
    calls: Vec<ModelToolCall>,
    step: u32,
    tool_timeout_ms: u64,
    max_output_chars: u32,
    tool_runtime: &ToolRuntimeConfig,
) -> Result<ToolExecutionTrace> {
    let mut trace = ToolExecutionTrace::default();

    for call in calls {
        let tool_name = call.name.clone();
        let tool_call_id = call.id.clone();
        let tool_started_at = Instant::now();
        let content = dispatch_tool_call_with_timeout(
            &tool_name,
            &tool_call_id,
            call.arguments,
            tool_timeout_ms,
            tool_runtime,
        )
        .await;
        let tool_latency = tool_started_at.elapsed();

        enforce_output_char_limit(
            &format!("tool `{tool_name}` output"),
            &content,
            max_output_chars,
        )?;

        info!(
            step,
            tool_name = %tool_name,
            tool_call_id = %tool_call_id,
            tool_latency_ms = tool_latency.as_millis(),
            "tool call completed"
        );
        trace.tool_calls = trace.tool_calls.saturating_add(1);
        trace.total_tool_latency = trace.total_tool_latency.saturating_add(tool_latency);
        trace.tool_names.push(tool_name.clone());

        messages.push(ModelMessage::tool_result(
            content,
            Some(tool_call_id),
            Some(tool_name),
        ));
    }

    Ok(trace)
}

async fn dispatch_tool_call_with_timeout(
    tool_name: &str,
    tool_call_id: &str,
    raw_args: serde_json::Value,
    tool_timeout_ms: u64,
    tool_runtime: &ToolRuntimeConfig,
) -> String {
    let timeout_result = with_timeout(
        dispatch_tool_call(tool_name, raw_args, tool_runtime),
        tool_timeout_ms,
    )
    .await;
    match timeout_result {
        Ok(Ok(output)) => output.payload.to_string(),
        Ok(Err(error)) => {
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

    use super::{
        build_model_tool_definitions, build_repl_tools_lines, enforce_consecutive_tool_step_cap,
        enforce_input_char_limit, enforce_output_char_limit, enforce_tool_call_cap,
        enforce_tool_calls_per_step_cap, repl_help_lines, with_timeout,
    };
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

    #[test]
    fn enforce_tool_calls_per_step_cap_accepts_within_limit() {
        enforce_tool_calls_per_step_cap(2, 3, 1).expect("should stay within cap");
    }

    #[test]
    fn enforce_tool_calls_per_step_cap_rejects_over_limit() {
        let error =
            enforce_tool_calls_per_step_cap(4, 3, 2).expect_err("should reject cap overrun");
        assert!(error.to_string().contains("AGENT_MAX_TOOL_CALLS_PER_STEP"));
    }

    #[test]
    fn enforce_consecutive_tool_step_cap_accepts_within_limit() {
        let next_total =
            enforce_consecutive_tool_step_cap(2, 4, 3).expect("should stay within cap");
        assert_eq!(next_total, 3);
    }

    #[test]
    fn enforce_consecutive_tool_step_cap_rejects_over_limit() {
        let error =
            enforce_consecutive_tool_step_cap(4, 4, 5).expect_err("should reject cap overrun");
        assert!(
            error
                .to_string()
                .contains("AGENT_MAX_CONSECUTIVE_TOOL_STEPS")
        );
    }

    #[test]
    fn enforce_input_char_limit_rejects_oversized_input() {
        let error = enforce_input_char_limit("12345", 4).expect_err("input should fail");
        assert!(error.to_string().contains("AGENT_MAX_INPUT_CHARS"));
    }

    #[test]
    fn enforce_output_char_limit_rejects_oversized_output() {
        let error = enforce_output_char_limit("assistant final response", "hello", 4)
            .expect_err("output should fail");
        assert!(error.to_string().contains("AGENT_MAX_OUTPUT_CHARS"));
    }

    #[test]
    fn repl_help_lists_tools_command() {
        let help = repl_help_lines();
        assert!(help.iter().any(|line| line.contains("/tools")));
    }

    #[test]
    fn repl_tools_lists_v1_tool_signatures() {
        let tools = build_repl_tools_lines().join("\n");
        assert!(tools.contains("search_notes(query: string, limit: u8)"));
        assert!(tools.contains("fetch_url(url: string)"));
        assert!(tools.contains("save_note(title: string, body: string)"));
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
            max_tool_calls_per_step: 4,
            max_consecutive_tool_steps: 4,
            max_input_chars: 4_000,
            max_output_chars: 8_000,
            tool_timeout_ms: 5_000,
            fetch_url_max_bytes: 100_000,
            fetch_url_allowed_domains: vec!["example.com".to_owned()],
            notes_dir: "notes".to_owned(),
            save_note_allow_overwrite: false,
            model_timeout_ms: 20_000,
            model_max_retries: 0,
        }
    }
}
