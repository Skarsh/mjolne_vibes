use std::env;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, ensure};

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen2.5:3b";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1-mini";
pub const DEFAULT_MAX_STEPS: u32 = 8;
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    Ollama,
    OpenAi,
}

impl ModelProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAi => "openai",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Ollama => DEFAULT_OLLAMA_MODEL,
            Self::OpenAi => DEFAULT_OPENAI_MODEL,
        }
    }
}

impl Display for ModelProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ModelProvider {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ollama" => Ok(Self::Ollama),
            "openai" => Ok(Self::OpenAi),
            other => Err(anyhow!(
                "invalid MODEL_PROVIDER `{other}`; expected `ollama` or `openai`"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSettings {
    pub model_provider: ModelProvider,
    pub model: String,
    pub ollama_base_url: String,
    pub openai_api_key: Option<String>,
    pub max_steps: u32,
    pub tool_timeout_ms: u64,
}

impl AgentSettings {
    pub fn from_env() -> Result<Self> {
        // Load .env if present, but do not fail if file does not exist.
        let _ = dotenvy::dotenv();

        let model_provider = env::var("MODEL_PROVIDER")
            .unwrap_or_else(|_| ModelProvider::Ollama.as_str().to_owned())
            .parse::<ModelProvider>()
            .context("failed to parse MODEL_PROVIDER")?;

        let model = env::var("MODEL").unwrap_or_else(|_| model_provider.default_model().to_owned());
        ensure!(!model.trim().is_empty(), "MODEL cannot be empty");

        let ollama_base_url =
            env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_OLLAMA_BASE_URL.to_owned());
        ensure!(
            !ollama_base_url.trim().is_empty(),
            "OLLAMA_BASE_URL cannot be empty"
        );

        let openai_api_key = read_optional_env("OPENAI_API_KEY");
        if model_provider == ModelProvider::OpenAi {
            let has_key = openai_api_key
                .as_deref()
                .map(|key| !key.trim().is_empty())
                .unwrap_or(false);
            ensure!(
                has_key,
                "OPENAI_API_KEY must be set when MODEL_PROVIDER is `openai`"
            );
        }

        let max_steps = parse_u32_env("AGENT_MAX_STEPS", DEFAULT_MAX_STEPS)?;
        ensure!(max_steps > 0, "AGENT_MAX_STEPS must be greater than 0");

        let tool_timeout_ms = parse_u64_env("TOOL_TIMEOUT_MS", DEFAULT_TOOL_TIMEOUT_MS)?;
        ensure!(
            tool_timeout_ms > 0,
            "TOOL_TIMEOUT_MS must be greater than 0"
        );

        Ok(Self {
            model_provider,
            model,
            ollama_base_url,
            openai_api_key,
            max_steps,
            tool_timeout_ms,
        })
    }
}

fn read_optional_env(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn parse_u32_env(name: &str, default: u32) -> Result<u32> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<u32>()
            .with_context(|| format!("failed to parse {name} as u32")),
        Err(_) => Ok(default),
    }
}

fn parse_u64_env(name: &str, default: u64) -> Result<u64> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .with_context(|| format!("failed to parse {name} as u64")),
        Err(_) => Ok(default),
    }
}
