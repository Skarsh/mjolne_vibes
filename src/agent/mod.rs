use anyhow::Result;
use tracing::info;

use crate::config::AgentSettings;

pub async fn run_chat_placeholder(settings: &AgentSettings, message: &str) -> Result<()> {
    info!(
        provider = %settings.model_provider,
        model = %settings.model,
        max_steps = settings.max_steps,
        tool_timeout_ms = settings.tool_timeout_ms,
        "executing placeholder chat flow"
    );

    println!("chat placeholder response");
    println!("provider: {}", settings.model_provider);
    println!("model: {}", settings.model);
    println!("max_steps: {}", settings.max_steps);
    println!("tool_timeout_ms: {}", settings.tool_timeout_ms);
    println!("message: {message}");

    Ok(())
}
