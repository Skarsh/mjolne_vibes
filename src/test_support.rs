use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn temp_path(prefix: &str) -> PathBuf {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "mjolne_vibes_{prefix}_{}_{}",
        std::process::id(),
        now_ns
    ))
}

pub fn remove_dir_if_exists(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

pub fn apply_ollama_test_env(
    command: &mut Command,
    notes_dir: &Path,
    log_dir: &Path,
    max_input_chars: u32,
    ollama_base_url: &str,
) {
    command.env("MODEL_PROVIDER", "ollama");
    command.env("MODEL", "qwen2.5:3b");
    command.env("OLLAMA_BASE_URL", ollama_base_url);
    command.env("AGENT_MAX_STEPS", "4");
    command.env("AGENT_MAX_TOOL_CALLS", "4");
    command.env("AGENT_MAX_TOOL_CALLS_PER_STEP", "2");
    command.env("AGENT_MAX_CONSECUTIVE_TOOL_STEPS", "2");
    command.env("AGENT_MAX_INPUT_CHARS", max_input_chars.to_string());
    command.env("AGENT_MAX_OUTPUT_CHARS", "2000");
    command.env("TOOL_TIMEOUT_MS", "100");
    command.env("FETCH_URL_MAX_BYTES", "4096");
    command.env("FETCH_URL_ALLOWED_DOMAINS", "example.com");
    command.env("NOTES_DIR", notes_dir.as_os_str());
    command.env("SAVE_NOTE_ALLOW_OVERWRITE", "false");
    command.env("MODEL_TIMEOUT_MS", "100");
    command.env("MODEL_MAX_RETRIES", "0");
    command.env("RUST_LOG", "error");
    command.env("MJOLNE_FILE_LOG", "error");
    command.env("MJOLNE_LOG_DIR", log_dir.as_os_str());
}
