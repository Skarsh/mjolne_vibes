use std::env;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, ensure};

pub const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen2.5:3b";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1-mini";
pub const DEFAULT_MAX_STEPS: u32 = 8;
pub const DEFAULT_MAX_TOOL_CALLS: u32 = 8;
pub const DEFAULT_MAX_TOOL_CALLS_PER_STEP: u32 = 4;
pub const DEFAULT_MAX_CONSECUTIVE_TOOL_STEPS: u32 = 4;
pub const DEFAULT_MAX_INPUT_CHARS: u32 = 4_000;
pub const DEFAULT_MAX_OUTPUT_CHARS: u32 = 8_000;
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_MODEL_TIMEOUT_MS: u64 = 20_000;
pub const DEFAULT_MODEL_MAX_RETRIES: u32 = 2;
pub const DEFAULT_FETCH_URL_ALLOWED_DOMAINS: &str = "example.com";
pub const DEFAULT_NOTES_DIR: &str = "notes";
pub const DEFAULT_SAVE_NOTE_ALLOW_OVERWRITE: bool = false;

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
    pub max_tool_calls: u32,
    pub max_tool_calls_per_step: u32,
    pub max_consecutive_tool_steps: u32,
    pub max_input_chars: u32,
    pub max_output_chars: u32,
    pub tool_timeout_ms: u64,
    pub fetch_url_allowed_domains: Vec<String>,
    pub notes_dir: String,
    pub save_note_allow_overwrite: bool,
    pub model_timeout_ms: u64,
    pub model_max_retries: u32,
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

        let max_steps = parse_positive_u32_env("AGENT_MAX_STEPS", DEFAULT_MAX_STEPS)?;
        let max_tool_calls =
            parse_positive_u32_env("AGENT_MAX_TOOL_CALLS", DEFAULT_MAX_TOOL_CALLS)?;
        let max_tool_calls_per_step = parse_positive_u32_env(
            "AGENT_MAX_TOOL_CALLS_PER_STEP",
            DEFAULT_MAX_TOOL_CALLS_PER_STEP,
        )?;
        let max_consecutive_tool_steps = parse_positive_u32_env(
            "AGENT_MAX_CONSECUTIVE_TOOL_STEPS",
            DEFAULT_MAX_CONSECUTIVE_TOOL_STEPS,
        )?;
        let max_input_chars =
            parse_positive_u32_env("AGENT_MAX_INPUT_CHARS", DEFAULT_MAX_INPUT_CHARS)?;
        let max_output_chars =
            parse_positive_u32_env("AGENT_MAX_OUTPUT_CHARS", DEFAULT_MAX_OUTPUT_CHARS)?;

        let tool_timeout_ms = parse_positive_u64_env("TOOL_TIMEOUT_MS", DEFAULT_TOOL_TIMEOUT_MS)?;

        let fetch_url_allowed_domains = parse_domain_allowlist(
            "FETCH_URL_ALLOWED_DOMAINS",
            &env::var("FETCH_URL_ALLOWED_DOMAINS")
                .unwrap_or_else(|_| DEFAULT_FETCH_URL_ALLOWED_DOMAINS.to_owned()),
        )?;
        let notes_dir = env::var("NOTES_DIR").unwrap_or_else(|_| DEFAULT_NOTES_DIR.to_owned());
        ensure!(!notes_dir.trim().is_empty(), "NOTES_DIR cannot be empty");
        let save_note_allow_overwrite = parse_bool_env(
            "SAVE_NOTE_ALLOW_OVERWRITE",
            DEFAULT_SAVE_NOTE_ALLOW_OVERWRITE,
        )?;

        let model_timeout_ms =
            parse_positive_u64_env("MODEL_TIMEOUT_MS", DEFAULT_MODEL_TIMEOUT_MS)?;

        let model_max_retries = parse_u32_env("MODEL_MAX_RETRIES", DEFAULT_MODEL_MAX_RETRIES)?;

        Ok(Self {
            model_provider,
            model,
            ollama_base_url,
            openai_api_key,
            max_steps,
            max_tool_calls,
            max_tool_calls_per_step,
            max_consecutive_tool_steps,
            max_input_chars,
            max_output_chars,
            tool_timeout_ms,
            fetch_url_allowed_domains,
            notes_dir,
            save_note_allow_overwrite,
            model_timeout_ms,
            model_max_retries,
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

fn parse_positive_u32_env(name: &str, default: u32) -> Result<u32> {
    let value = parse_u32_env(name, default)?;
    ensure_positive_u32(name, value)
}

fn parse_u64_env(name: &str, default: u64) -> Result<u64> {
    match env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .with_context(|| format!("failed to parse {name} as u64")),
        Err(_) => Ok(default),
    }
}

fn parse_positive_u64_env(name: &str, default: u64) -> Result<u64> {
    let value = parse_u64_env(name, default)?;
    ensure!(value > 0, "{name} must be greater than 0");
    Ok(value)
}

fn parse_bool_env(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(raw) => parse_bool_value(name, &raw),
        Err(_) => Ok(default),
    }
}

fn parse_bool_value(name: &str, raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow!(
            "failed to parse {name} as bool; expected one of true/false/1/0/yes/no/on/off"
        )),
    }
}

fn ensure_positive_u32(name: &str, value: u32) -> Result<u32> {
    ensure!(value > 0, "{name} must be greater than 0");
    Ok(value)
}

fn parse_domain_allowlist(name: &str, raw: &str) -> Result<Vec<String>> {
    let mut domains = raw
        .split(',')
        .filter_map(|domain| {
            let normalized = domain.trim().trim_matches('.').to_ascii_lowercase();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<Vec<_>>();

    ensure!(
        !domains.is_empty(),
        "{name} must contain at least one domain"
    );

    for domain in &domains {
        ensure!(
            domain
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-'),
            "{name} contains invalid domain `{domain}`"
        );
    }

    domains.sort();
    domains.dedup();
    Ok(domains)
}

#[cfg(test)]
mod tests {
    use super::{ensure_positive_u32, parse_bool_value, parse_domain_allowlist};

    #[test]
    fn ensure_positive_u32_accepts_positive_values() {
        let value = ensure_positive_u32("AGENT_MAX_STEPS", 3).expect("positive values should pass");
        assert_eq!(value, 3);
    }

    #[test]
    fn ensure_positive_u32_rejects_zero() {
        let error = ensure_positive_u32("AGENT_MAX_STEPS", 0).expect_err("zero values should fail");
        assert!(error.to_string().contains("AGENT_MAX_STEPS"));
        assert!(error.to_string().contains("greater than 0"));
    }

    #[test]
    fn parse_bool_value_accepts_truthy_and_falsy_values() {
        assert!(parse_bool_value("SAVE_NOTE_ALLOW_OVERWRITE", "true").expect("true should parse"));
        assert!(parse_bool_value("SAVE_NOTE_ALLOW_OVERWRITE", "1").expect("1 should parse"));
        assert!(
            !parse_bool_value("SAVE_NOTE_ALLOW_OVERWRITE", "false").expect("false should parse")
        );
        assert!(!parse_bool_value("SAVE_NOTE_ALLOW_OVERWRITE", "0").expect("0 should parse"));
    }

    #[test]
    fn parse_bool_value_rejects_invalid_values() {
        let error = parse_bool_value("SAVE_NOTE_ALLOW_OVERWRITE", "maybe")
            .expect_err("invalid bool should fail");
        assert!(error.to_string().contains("SAVE_NOTE_ALLOW_OVERWRITE"));
    }

    #[test]
    fn parse_domain_allowlist_normalizes_and_deduplicates() {
        let domains = parse_domain_allowlist(
            "FETCH_URL_ALLOWED_DOMAINS",
            "Example.com, api.example.com, example.com, .docs.rs.",
        )
        .expect("allowlist should parse");

        assert_eq!(
            domains,
            vec![
                "api.example.com".to_owned(),
                "docs.rs".to_owned(),
                "example.com".to_owned()
            ]
        );
    }

    #[test]
    fn parse_domain_allowlist_rejects_empty_input() {
        let error = parse_domain_allowlist("FETCH_URL_ALLOWED_DOMAINS", " , ,, ")
            .expect_err("empty values should fail");
        assert!(
            error
                .to_string()
                .contains("FETCH_URL_ALLOWED_DOMAINS must contain at least one domain")
        );
    }

    #[test]
    fn parse_domain_allowlist_rejects_invalid_characters() {
        let error = parse_domain_allowlist("FETCH_URL_ALLOWED_DOMAINS", "exa*mple.com")
            .expect_err("invalid domain should fail");
        assert!(
            error
                .to_string()
                .contains("FETCH_URL_ALLOWED_DOMAINS contains invalid domain")
        );
    }
}
