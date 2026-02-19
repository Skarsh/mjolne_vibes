use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent::{ChatTurnError, ChatTurnErrorKind, run_chat_turn};
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
            let status = status_code_for_error_kind(error.kind());
            warn!(
                status = status.as_u16(),
                error = %details,
                "HTTP chat request failed"
            );
            let body = ErrorBody { error: details };
            (status, Json(body)).into_response()
        }
    }
}

fn error_details(error: &ChatTurnError) -> String {
    error.details()
}

fn status_code_for_error_kind(kind: ChatTurnErrorKind) -> StatusCode {
    match kind {
        ChatTurnErrorKind::BadRequest => StatusCode::BAD_REQUEST,
        ChatTurnErrorKind::Upstream => StatusCode::BAD_GATEWAY,
        ChatTurnErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::status_code_for_error_kind;
    use crate::agent::ChatTurnErrorKind;

    #[test]
    fn status_code_classifies_bad_request_kind() {
        assert_eq!(
            status_code_for_error_kind(ChatTurnErrorKind::BadRequest),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn status_code_classifies_upstream_kind() {
        assert_eq!(
            status_code_for_error_kind(ChatTurnErrorKind::Upstream),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn status_code_classifies_internal_kind() {
        assert_eq!(
            status_code_for_error_kind(ChatTurnErrorKind::Internal),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
