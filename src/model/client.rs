use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{sleep, timeout};
use tracing::{debug, warn};

use crate::config::{AgentSettings, ModelProvider};

const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const RETRY_BASE_DELAY_MS: u64 = 250;

#[derive(Debug, thiserror::Error)]
pub enum ModelClientError {
    #[error("model request timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("HTTP request failed: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("provider returned HTTP {status}: {body}")]
    HttpStatus { status: StatusCode, body: String },

    #[error("response missing field: {field}")]
    MissingField { field: &'static str },

    #[error("response format error: {0}")]
    ResponseFormat(String),

    #[error("configuration error: {0}")]
    Configuration(String),
}

impl ModelClientError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Timeout { .. } => true,
            Self::Transport(error) => {
                error.is_timeout() || error.is_connect() || error.is_request()
            }
            Self::HttpStatus { status, .. } => {
                *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            Self::MissingField { .. } | Self::ResponseFormat(_) | Self::Configuration(_) => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(MessageRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(MessageRole::User, content)
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self::new(MessageRole::Assistant, content)
    }

    pub fn assistant_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ModelToolCall>,
    ) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls,
        }
    }

    pub fn tool_result(
        content: impl Into<String>,
        tool_call_id: Option<String>,
        tool_name: Option<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id,
            tool_name,
            tool_calls: Vec::new(),
        }
    }

    fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ModelToolDefinition>,
}

impl ChatRequest {
    pub fn new(
        model: String,
        messages: Vec<ModelMessage>,
        tools: Vec<ModelToolDefinition>,
    ) -> Self {
        Self {
            model,
            messages,
            tools,
        }
    }

    pub fn from_prompts(model: &str, system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            model: model.to_owned(),
            messages: vec![
                ModelMessage::system(system_prompt),
                ModelMessage::user(user_prompt),
            ],
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatResponse {
    FinalText {
        text: String,
    },
    ToolCalls {
        assistant_content: Option<String>,
        calls: Vec<ModelToolCall>,
    },
}

#[derive(Debug, Clone)]
pub struct ModelClient {
    http_client: reqwest::Client,
    settings: AgentSettings,
}

impl ModelClient {
    pub fn new(settings: AgentSettings) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            settings,
        }
    }

    pub async fn chat(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<ChatResponse, ModelClientError> {
        let request = ChatRequest::from_prompts(&self.settings.model, system_prompt, user_prompt);
        self.chat_request(&request).await
    }

    pub async fn chat_with_messages(
        &self,
        messages: &[ModelMessage],
        tools: &[ModelToolDefinition],
    ) -> Result<ChatResponse, ModelClientError> {
        let request = ChatRequest::new(
            self.settings.model.clone(),
            messages.to_vec(),
            tools.to_vec(),
        );
        self.chat_request(&request).await
    }

    async fn chat_request(&self, request: &ChatRequest) -> Result<ChatResponse, ModelClientError> {
        let total_attempts = self.settings.model_max_retries.saturating_add(1);
        let mut attempt: u32 = 1;

        loop {
            let result = self.chat_once(request).await;
            match result {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let should_retry = attempt < total_attempts && error.is_retryable();
                    if !should_retry {
                        return Err(error);
                    }

                    let delay_ms = retry_delay_ms(attempt);
                    warn!(
                        attempt,
                        total_attempts,
                        delay_ms,
                        error = %error,
                        "model call failed; retrying"
                    );

                    sleep(Duration::from_millis(delay_ms)).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }

    async fn chat_once(&self, request: &ChatRequest) -> Result<ChatResponse, ModelClientError> {
        let timeout_duration = Duration::from_millis(self.settings.model_timeout_ms);
        match timeout(timeout_duration, self.chat_by_provider(request)).await {
            Ok(result) => result,
            Err(_) => Err(ModelClientError::Timeout {
                timeout_ms: self.settings.model_timeout_ms,
            }),
        }
    }

    async fn chat_by_provider(
        &self,
        request: &ChatRequest,
    ) -> Result<ChatResponse, ModelClientError> {
        match self.settings.model_provider {
            ModelProvider::Ollama => self.chat_ollama(request).await,
            ModelProvider::OpenAi => self.chat_openai(request).await,
        }
    }

    async fn chat_ollama(&self, request: &ChatRequest) -> Result<ChatResponse, ModelClientError> {
        let url = format!(
            "{}/api/chat",
            self.settings.ollama_base_url.trim_end_matches('/')
        );
        let provider_request = OllamaChatRequest::from_common_request(request);

        debug!(
            url = %url,
            model = %request.model,
            message_count = request.messages.len(),
            tool_count = request.tools.len(),
            "sending chat request to ollama"
        );

        let response = self.post_json(&url, None, &provider_request).await?;
        let payload: OllamaChatResponse = response.json().await?;
        if let Some(error_message) = payload.error {
            return Err(ModelClientError::ResponseFormat(error_message));
        }

        let message = payload
            .message
            .ok_or(ModelClientError::MissingField { field: "message" })?;

        if !message.tool_calls.is_empty() {
            let calls = parse_ollama_tool_calls(message.tool_calls)?;
            let assistant_content = normalize_optional_text(message.content);
            return Ok(ChatResponse::ToolCalls {
                assistant_content,
                calls,
            });
        }

        let text =
            normalize_optional_text(message.content).ok_or(ModelClientError::MissingField {
                field: "message.content",
            })?;
        Ok(ChatResponse::FinalText { text })
    }

    async fn chat_openai(&self, request: &ChatRequest) -> Result<ChatResponse, ModelClientError> {
        let api_key = self.settings.openai_api_key.as_deref().ok_or_else(|| {
            ModelClientError::Configuration("OPENAI_API_KEY is required".to_owned())
        })?;

        let url = format!("{OPENAI_BASE_URL}/chat/completions");
        let provider_request = OpenAiChatRequest::from_common_request(request);

        debug!(
            url = %url,
            model = %request.model,
            message_count = request.messages.len(),
            tool_count = request.tools.len(),
            "sending chat request to openai"
        );

        let response = self
            .post_json(&url, Some(api_key), &provider_request)
            .await?;
        let payload: OpenAiChatResponse = response.json().await?;
        let choice = payload
            .choices
            .first()
            .ok_or(ModelClientError::MissingField {
                field: "choices[0]",
            })?;

        if !choice.message.tool_calls.is_empty() {
            let calls = parse_openai_tool_calls(choice.message.tool_calls.clone())?;
            let assistant_content = choice
                .message
                .content
                .as_ref()
                .and_then(extract_openai_content_text)
                .and_then(normalize_text);

            return Ok(ChatResponse::ToolCalls {
                assistant_content,
                calls,
            });
        }

        let content = choice
            .message
            .content
            .as_ref()
            .and_then(extract_openai_content_text)
            .and_then(normalize_text)
            .ok_or_else(|| {
                ModelClientError::ResponseFormat(
                    "unable to extract assistant content from OpenAI response".to_owned(),
                )
            })?;

        Ok(ChatResponse::FinalText { text: content })
    }

    async fn post_json<T: Serialize>(
        &self,
        url: &str,
        bearer_token: Option<&str>,
        body: &T,
    ) -> Result<reqwest::Response, ModelClientError> {
        let mut request = self.http_client.post(url).json(body);
        if let Some(token) = bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;
        ensure_success(response).await
    }
}

fn retry_delay_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(5);
    RETRY_BASE_DELAY_MS.saturating_mul(1_u64 << exponent)
}

async fn ensure_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, ModelClientError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read error response body>".to_owned());
    Err(ModelClientError::HttpStatus { status, body })
}

fn normalize_text(content: String) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn normalize_optional_text(content: Option<String>) -> Option<String> {
    content.and_then(normalize_text)
}

fn extract_openai_content_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.to_owned()),
        serde_json::Value::Array(parts) => {
            let mut output = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|text| text.as_str()) {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(text);
                }
            }

            if output.trim().is_empty() {
                None
            } else {
                Some(output)
            }
        }
        _ => None,
    }
}

fn parse_tool_arguments(raw: Value, field: &str) -> Result<Value, ModelClientError> {
    match raw {
        Value::String(arguments) => serde_json::from_str::<Value>(&arguments).map_err(|error| {
            ModelClientError::ResponseFormat(format!("failed to parse {field} as JSON: {error}"))
        }),
        value => Ok(value),
    }
}

fn parse_openai_tool_calls(
    raw_calls: Vec<OpenAiToolCallResponse>,
) -> Result<Vec<ModelToolCall>, ModelClientError> {
    raw_calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let arguments = parse_tool_arguments(
                call.function.arguments,
                "choices[0].message.tool_calls[].function.arguments",
            )?;

            Ok(ModelToolCall {
                id: call
                    .id
                    .unwrap_or_else(|| format!("openai-tool-call-{}", index + 1)),
                name: call.function.name,
                arguments,
            })
        })
        .collect()
}

fn parse_ollama_tool_calls(
    raw_calls: Vec<OllamaToolCallResponse>,
) -> Result<Vec<ModelToolCall>, ModelClientError> {
    raw_calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| {
            let arguments = parse_tool_arguments(
                call.function.arguments,
                "message.tool_calls[].function.arguments",
            )?;

            Ok(ModelToolCall {
                id: call
                    .id
                    .unwrap_or_else(|| format!("ollama-tool-call-{}", index + 1)),
                name: call.function.name,
                arguments,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OpenAiRequestToolCall>,
}

impl From<&ModelMessage> for OpenAiMessage {
    fn from(message: &ModelMessage) -> Self {
        let content = if message.role == MessageRole::Assistant
            && !message.tool_calls.is_empty()
            && message.content.trim().is_empty()
        {
            None
        } else {
            Some(message.content.clone())
        };

        Self {
            role: message.role.as_str().to_owned(),
            content,
            tool_call_id: message.tool_call_id.clone(),
            tool_calls: message
                .tool_calls
                .iter()
                .map(OpenAiRequestToolCall::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiRequestToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: &'static str,
    function: OpenAiRequestToolFunction,
}

impl From<&ModelToolCall> for OpenAiRequestToolCall {
    fn from(tool_call: &ModelToolCall) -> Self {
        Self {
            id: tool_call.id.clone(),
            call_type: "function",
            function: OpenAiRequestToolFunction {
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiRequestToolFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiToolDefinition {
    #[serde(rename = "type")]
    tool_type: &'static str,
    function: OpenAiToolFunctionDefinition,
}

impl From<&ModelToolDefinition> for OpenAiToolDefinition {
    fn from(tool: &ModelToolDefinition) -> Self {
        Self {
            tool_type: "function",
            function: OpenAiToolFunctionDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiToolFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAiToolDefinition>,
}

impl OpenAiChatRequest {
    fn from_common_request(request: &ChatRequest) -> Self {
        Self {
            model: request.model.clone(),
            messages: request.messages.iter().map(OpenAiMessage::from).collect(),
            tools: request
                .tools
                .iter()
                .map(OpenAiToolDefinition::from)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiResponseMessage {
    content: Option<serde_json::Value>,
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCallResponse>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCallResponse {
    id: Option<String>,
    function: OpenAiToolCallFunctionResponse,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiToolCallFunctionResponse {
    name: String,
    arguments: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<OllamaRequestToolCall>,
}

impl From<&ModelMessage> for OllamaMessage {
    fn from(message: &ModelMessage) -> Self {
        Self {
            role: message.role.as_str().to_owned(),
            content: message.content.clone(),
            tool_name: message.tool_name.clone(),
            tool_calls: message
                .tool_calls
                .iter()
                .map(OllamaRequestToolCall::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaRequestToolCall {
    id: String,
    function: OllamaRequestToolFunction,
}

impl From<&ModelToolCall> for OllamaRequestToolCall {
    fn from(tool_call: &ModelToolCall) -> Self {
        Self {
            id: tool_call.id.clone(),
            function: OllamaRequestToolFunction {
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaRequestToolFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaToolDefinition {
    #[serde(rename = "type")]
    tool_type: &'static str,
    function: OllamaToolFunctionDefinition,
}

impl From<&ModelToolDefinition> for OllamaToolDefinition {
    fn from(tool: &ModelToolDefinition) -> Self {
        Self {
            tool_type: "function",
            function: OllamaToolFunctionDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaToolFunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct OllamaChatRequest {
    model: String,
    stream: bool,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OllamaToolDefinition>,
}

impl OllamaChatRequest {
    fn from_common_request(request: &ChatRequest) -> Self {
        Self {
            model: request.model.clone(),
            stream: false,
            messages: request.messages.iter().map(OllamaMessage::from).collect(),
            tools: request
                .tools
                .iter()
                .map(OllamaToolDefinition::from)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaResponseMessage>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCallResponse>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCallResponse {
    id: Option<String>,
    function: OllamaToolCallFunctionResponse,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCallFunctionResponse {
    name: String,
    arguments: Value,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn common_request_builds_expected_messages() {
        let request = ChatRequest::from_prompts("model-a", "system prompt", "user prompt");

        assert_eq!(request.model, "model-a");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, MessageRole::System);
        assert_eq!(request.messages[0].content, "system prompt");
        assert_eq!(request.messages[1].role, MessageRole::User);
        assert_eq!(request.messages[1].content, "user prompt");
        assert!(request.tools.is_empty());
    }

    #[test]
    fn provider_requests_include_message_and_tool_conversion() {
        let request = ChatRequest::new(
            "m".to_owned(),
            vec![
                ModelMessage::system("s"),
                ModelMessage::user("u"),
                ModelMessage::assistant_tool_calls(
                    "",
                    vec![ModelToolCall {
                        id: "call-1".to_owned(),
                        name: "search_notes".to_owned(),
                        arguments: json!({"query": "rust", "limit": 3}),
                    }],
                ),
                ModelMessage::tool_result(
                    "{\"results\":[]}",
                    Some("call-1".to_owned()),
                    Some("search_notes".to_owned()),
                ),
            ],
            vec![ModelToolDefinition {
                name: "search_notes".to_owned(),
                description: "Search notes".to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
            }],
        );

        let openai = OpenAiChatRequest::from_common_request(&request);
        let ollama = OllamaChatRequest::from_common_request(&request);

        assert_eq!(openai.model, "m");
        assert_eq!(ollama.model, "m");
        assert_eq!(openai.tools.len(), 1);
        assert_eq!(ollama.tools.len(), 1);
        assert_eq!(openai.messages.len(), 4);
        assert_eq!(ollama.messages.len(), 4);

        assert_eq!(openai.messages[2].content, None);
        assert_eq!(openai.messages[2].tool_calls.len(), 1);
        assert_eq!(
            openai.messages[2].tool_calls[0].function.arguments,
            "{\"limit\":3,\"query\":\"rust\"}"
        );
        assert_eq!(openai.messages[3].tool_call_id.as_deref(), Some("call-1"));

        assert_eq!(ollama.messages[2].tool_calls.len(), 1);
        assert_eq!(
            ollama.messages[2].tool_calls[0].function.arguments,
            json!({"query": "rust", "limit": 3})
        );
        assert_eq!(
            ollama.messages[3].tool_name.as_deref(),
            Some("search_notes")
        );
    }

    #[test]
    fn parse_tool_arguments_from_json_string() {
        let parsed = parse_tool_arguments(
            Value::String("{\"query\":\"rust\",\"limit\":5}".to_owned()),
            "field",
        )
        .expect("arguments should parse");

        assert_eq!(parsed, json!({"query": "rust", "limit": 5}));
    }

    #[test]
    fn parse_tool_arguments_rejects_invalid_json_string() {
        let error = parse_tool_arguments(Value::String("not-json".to_owned()), "field")
            .expect_err("invalid json should fail");

        let message = error.to_string();
        assert!(message.contains("failed to parse field"));
    }

    #[test]
    fn extract_openai_content_from_string() {
        let value = serde_json::Value::String("hello world".to_owned());
        let text = extract_openai_content_text(&value);
        assert_eq!(text.as_deref(), Some("hello world"));
    }

    #[test]
    fn extract_openai_content_from_array_parts() {
        let value = serde_json::json!([
            {"type": "output_text", "text": "line one"},
            {"type": "output_text", "text": "line two"}
        ]);

        let text = extract_openai_content_text(&value);
        assert_eq!(text.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn retry_delay_uses_exponential_backoff_with_cap() {
        assert_eq!(retry_delay_ms(1), 250);
        assert_eq!(retry_delay_ms(2), 500);
        assert_eq!(retry_delay_ms(6), 8_000);
        assert_eq!(retry_delay_ms(99), 8_000);
    }
}
