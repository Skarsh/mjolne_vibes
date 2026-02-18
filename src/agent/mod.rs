use anyhow::{Context, Result};
use tracing::info;

use crate::config::AgentSettings;
use crate::model::client::ModelClient;

const SYSTEM_PROMPT: &str =
    "You are a concise, reliable Rust AI assistant. Be helpful and truthful.";

pub async fn run_chat(settings: &AgentSettings, message: &str) -> Result<()> {
    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        model_timeout_ms = settings.model_timeout_ms,
        model_max_retries = settings.model_max_retries,
        max_steps = settings.max_steps,
        "executing phase-1 chat flow"
    );

    let client = ModelClient::new(settings.clone());
    let response = client
        .chat(SYSTEM_PROMPT, message)
        .await
        .with_context(|| format!("model chat failed for provider {}", settings.model_provider))?;

    println!("{}", response.text);

    Ok(())
}
