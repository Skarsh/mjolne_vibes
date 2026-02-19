use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::sync::OnceLock;
use tracing_subscriber::fmt;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

use mjolne_vibes::agent::{run_chat, run_chat_json, run_repl};
use mjolne_vibes::config::AgentSettings;
use mjolne_vibes::eval::{DEFAULT_EVAL_CASES_PATH, run_eval_command};
use mjolne_vibes::server::run_http_server;
use mjolne_vibes::studio::run_studio;

static FILE_LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

#[derive(Debug, Parser)]
#[command(name = "mjolne_vibes", about = "CLI-first Rust AI agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Send a message to the agent.
    Chat {
        message: String,
        /// Emit a machine-readable JSON payload with final text, trace, and tool calls.
        #[arg(long)]
        json: bool,
    },
    /// Start an interactive multi-turn REPL session.
    Repl {
        /// Print info/debug logs to terminal during interactive use.
        #[arg(long)]
        verbose: bool,
    },
    /// Run evaluation cases from YAML.
    Eval {
        /// Path to eval cases YAML file.
        #[arg(long, default_value = DEFAULT_EVAL_CASES_PATH)]
        cases: String,
    },
    /// Start an HTTP server exposing the same one-turn chat loop.
    Serve {
        /// Socket address to bind, for example 127.0.0.1:8080.
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: String,
    },
    /// Start native studio UI with chat and canvas panes.
    Studio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogMode {
    Standard,
    ReplQuiet,
    ReplVerbose,
}

impl LogMode {
    fn from_command(command: &Commands) -> Self {
        match command {
            Commands::Repl { verbose: true } => Self::ReplVerbose,
            Commands::Repl { verbose: false } => Self::ReplQuiet,
            Commands::Chat { .. }
            | Commands::Eval { .. }
            | Commands::Serve { .. }
            | Commands::Studio => Self::Standard,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(LogMode::from_command(&cli.command))?;
    let settings = AgentSettings::from_env().context("failed to load configuration")?;

    match cli.command {
        Commands::Chat {
            message,
            json: false,
        } => run_chat(&settings, &message).await?,
        Commands::Chat {
            message,
            json: true,
        } => run_chat_json(&settings, &message).await?,
        Commands::Repl { .. } => run_repl(&settings).await?,
        Commands::Eval { cases } => {
            run_eval_command(&settings, std::path::Path::new(&cases)).await?
        }
        Commands::Serve { bind } => run_http_server(&settings, &bind).await?,
        Commands::Studio => run_studio(&settings)?,
    }

    Ok(())
}

fn init_tracing(mode: LogMode) -> Result<()> {
    let default_console_filter = match mode {
        LogMode::ReplQuiet => "warn",
        LogMode::ReplVerbose => "info,mjolne_vibes=debug",
        LogMode::Standard => "info,mjolne_vibes=info",
    };
    let console_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_console_filter));

    let file_filter = match std::env::var("MJOLNE_FILE_LOG") {
        Ok(value) => value
            .parse::<EnvFilter>()
            .with_context(|| format!("failed to parse MJOLNE_FILE_LOG `{value}`"))?,
        Err(_) => EnvFilter::new("info,mjolne_vibes=debug"),
    };

    let log_dir = std::env::var("MJOLNE_LOG_DIR").unwrap_or_else(|_| "logs".to_owned());
    let file_appender = tracing_appender::rolling::daily(log_dir, "mjolne_vibes.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = FILE_LOG_GUARD.set(guard);

    let console_layer = fmt::layer()
        .compact()
        .with_target(false)
        .with_filter(console_filter);

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(file_writer)
        .with_filter(file_filter);

    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing subscriber: {error}"))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Commands, LogMode};

    #[test]
    fn repl_defaults_to_quiet_mode() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "repl"]).expect("parse should succeed");
        match cli.command {
            Commands::Repl { verbose } => assert!(!verbose),
            _ => panic!("expected repl command"),
        }
        assert_eq!(
            LogMode::from_command(&Commands::Repl { verbose: false }),
            LogMode::ReplQuiet
        );
    }

    #[test]
    fn repl_verbose_flag_enables_verbose_mode() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "repl", "--verbose"])
            .expect("parse should succeed");
        match cli.command {
            Commands::Repl { verbose } => assert!(verbose),
            _ => panic!("expected repl command"),
        }
        assert_eq!(
            LogMode::from_command(&Commands::Repl { verbose: true }),
            LogMode::ReplVerbose
        );
    }

    #[test]
    fn eval_command_uses_default_cases_path() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "eval"]).expect("parse should succeed");
        match cli.command {
            Commands::Eval { cases } => assert_eq!(cases, super::DEFAULT_EVAL_CASES_PATH),
            _ => panic!("expected eval command"),
        }
    }

    #[test]
    fn chat_command_supports_json_flag() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "chat", "hello", "--json"])
            .expect("parse should succeed");
        match cli.command {
            Commands::Chat { message, json } => {
                assert_eq!(message, "hello");
                assert!(json);
            }
            _ => panic!("expected chat command"),
        }
    }

    #[test]
    fn serve_command_uses_default_bind_address() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "serve"]).expect("parse should succeed");
        match cli.command {
            Commands::Serve { bind } => assert_eq!(bind, "127.0.0.1:8080"),
            _ => panic!("expected serve command"),
        }
    }

    #[test]
    fn studio_command_is_available() {
        let cli = Cli::try_parse_from(["mjolne_vibes", "studio"]).expect("parse should succeed");
        match cli.command {
            Commands::Studio => {}
            _ => panic!("expected studio command"),
        }
    }
}
