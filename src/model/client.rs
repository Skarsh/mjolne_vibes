use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
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
}

impl MessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMessage {
    pub role: MessageRole,
    pub content: String,
}

impl ModelMessage {
    fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
}

impl ChatRequest {
    pub fn from_prompts(model: &str, system_prompt: &str, user_prompt: &str) -> Self {
        Self {
            model: model.to_owned(),
            messages: vec![
                ModelMessage::new(MessageRole::System, system_prompt),
                ModelMessage::new(MessageRole::User, user_prompt),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatResponse {
    pub text: String,
}

impl ChatResponse {
    fn from_text(
        content: impl Into<String>,
        source_field: &'static str,
    ) -> Result<Self, ModelClientError> {
        let content = content.into();
        let content = content.trim();
        if content.is_empty() {
            return Err(ModelClientError::MissingField {
                field: source_field,
            });
        }

        Ok(Self {
            text: content.to_owned(),
        })
    }
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
        let total_attempts = self.settings.model_max_retries.saturating_add(1);
        let mut attempt: u32 = 1;

        loop {
            let result = self.chat_once(&request).await;
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

        debug!(url = %url, model = %request.model, "sending chat request to ollama");

        let response = self.post_json(&url, None, &provider_request).await?;
        let payload: OllamaChatResponse = response.json().await?;
        if let Some(error_message) = payload.error {
            return Err(ModelClientError::ResponseFormat(error_message));
        }

        let message = payload
            .message
            .ok_or(ModelClientError::MissingField { field: "message" })?;
        ChatResponse::from_text(message.content, "message.content")
    }

    async fn chat_openai(&self, request: &ChatRequest) -> Result<ChatResponse, ModelClientError> {
        let api_key = self.settings.openai_api_key.as_deref().ok_or_else(|| {
            ModelClientError::Configuration("OPENAI_API_KEY is required".to_owned())
        })?;

        let url = format!("{OPENAI_BASE_URL}/chat/completions");
        let provider_request = OpenAiChatRequest::from_common_request(request);

        debug!(url = %url, model = %request.model, "sending chat request to openai");

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

        let content = choice
            .message
            .content
            .as_ref()
            .and_then(extract_openai_content_text)
            .ok_or_else(|| {
                ModelClientError::ResponseFormat(
                    "unable to extract assistant content from OpenAI response".to_owned(),
                )
            })?;

        ChatResponse::from_text(content, "choices[0].message.content")
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ProviderMessage {
    role: String,
    content: String,
}

impl From<&ModelMessage> for ProviderMessage {
    fn from(message: &ModelMessage) -> Self {
        Self {
            role: message.role.as_str().to_owned(),
            content: message.content.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct OllamaChatRequest {
    model: String,
    stream: bool,
    messages: Vec<ProviderMessage>,
}

impl OllamaChatRequest {
    fn from_common_request(request: &ChatRequest) -> Self {
        Self {
            model: request.model.clone(),
            stream: false,
            messages: request.messages.iter().map(ProviderMessage::from).collect(),
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
    content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<ProviderMessage>,
}

impl OpenAiChatRequest {
    fn from_common_request(request: &ChatRequest) -> Self {
        Self {
            model: request.model.clone(),
            messages: request.messages.iter().map(ProviderMessage::from).collect(),
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

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn provider_requests_share_same_message_conversion() {
        let request = ChatRequest::from_prompts("m", "s", "u");
        let ollama = OllamaChatRequest::from_common_request(&request);
        let openai = OpenAiChatRequest::from_common_request(&request);

        assert_eq!(ollama.model, "m");
        assert_eq!(openai.model, "m");
        assert_eq!(
            ollama.messages,
            vec![
                ProviderMessage {
                    role: "system".to_owned(),
                    content: "s".to_owned(),
                },
                ProviderMessage {
                    role: "user".to_owned(),
                    content: "u".to_owned(),
                },
            ]
        );
        assert_eq!(openai.messages, ollama.messages);
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
