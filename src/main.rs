use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod agent;
mod config;
mod model;

use crate::agent::run_chat;
use crate::config::AgentSettings;

#[derive(Debug, Parser)]
#[command(name = "mjolne_vibes", about = "CLI-first Rust AI agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Send a message to the agent.
    Chat { message: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing()?;

    let cli = Cli::parse();
    let settings = AgentSettings::from_env().context("failed to load configuration")?;

    match cli.command {
        Commands::Chat { message } => run_chat(&settings, &message).await?,
    }

    Ok(())
}

fn init_tracing() -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,mjolne_vibes=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing subscriber: {error}"))
}
