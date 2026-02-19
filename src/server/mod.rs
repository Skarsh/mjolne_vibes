use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent::run_chat_turn;
use crate::config::AgentSettings;

#[derive(Clone)]
struct AppState {
    settings: AgentSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatRequest {
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Serialize)]
struct HealthBody {
    status: &'static str,
}

pub async fn run_http_server(settings: &AgentSettings, bind: &str) -> Result<()> {
    let state = AppState {
        settings: settings.clone(),
    };
    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/chat", post(handle_chat))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HTTP server to `{bind}`"))?;
    let local_addr = listener.local_addr().ok();

    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        requested_bind = %bind,
        bound_addr = local_addr.map(|addr| addr.to_string()),
        "starting HTTP server"
    );

    axum::serve(listener, app)
        .await
        .context("HTTP server exited with an error")
}

async fn handle_health() -> Json<HealthBody> {
    Json(HealthBody { status: "ok" })
}

async fn handle_chat(State(state): State<AppState>, Json(req): Json<ChatRequest>) -> Response {
    match run_chat_turn(&state.settings, &req.message).await {
        Ok(outcome) => (StatusCode::OK, Json(outcome)).into_response(),
        Err(error) => {
            let details = error_details(&error);
            let status = status_code_for_error(&details);
            warn!(
                status = status.as_u16(),
                error = %error,
                "HTTP chat request failed"
            );
            let body = ErrorBody { error: details };
            (status, Json(body)).into_response()
        }
    }
}

fn error_details(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

fn status_code_for_error(details: &str) -> StatusCode {
    if details.contains("input exceeds configured character limit")
        || details.contains("output exceeds configured character limit")
        || details.contains("tool call budget exceeded")
        || details.contains("max_steps")
        || details.contains("policy blocked")
        || details.contains("unknown tool")
        || details.contains("invalid tool arguments")
        || details.contains("AGENT_MAX_INPUT_CHARS")
        || details.contains("AGENT_MAX_OUTPUT_CHARS")
    {
        return StatusCode::BAD_REQUEST;
    }

    if details.contains("model chat failed") || details.contains("tool dispatch failed") {
        return StatusCode::BAD_GATEWAY;
    }

    StatusCode::INTERNAL_SERVER_ERROR
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::{error_details, status_code_for_error};

    #[test]
    fn status_code_classifies_validation_and_policy_errors() {
        let input_error = anyhow::anyhow!("input exceeds configured character limit (4000 chars)");
        let policy_error = anyhow::anyhow!("tool dispatch failed for `fetch_url`: policy blocked");
        assert_eq!(
            status_code_for_error(&error_details(&input_error)),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_code_for_error(&error_details(&policy_error)),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn status_code_classifies_upstream_errors() {
        let model_error = anyhow::anyhow!("model chat failed for provider ollama at step 1");
        assert_eq!(
            status_code_for_error(&error_details(&model_error)),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn status_code_uses_error_chain_for_wrapped_validation_errors() {
        let wrapped = anyhow::anyhow!("chat turn failed")
            .context("user input exceeded AGENT_MAX_INPUT_CHARS limit: 4101 chars (max 4000)");
        assert_eq!(
            status_code_for_error(&error_details(&wrapped)),
            StatusCode::BAD_REQUEST
        );
    }
}
