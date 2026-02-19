use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use serde_json::json;
use tokio::time::sleep;

struct RunningServer {
    child: Child,
    bind_addr: String,
    notes_dir: PathBuf,
    log_dir: PathBuf,
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.notes_dir);
        let _ = fs::remove_dir_all(&self.log_dir);
    }
}

#[tokio::test]
async fn cli_and_http_share_oversized_input_guardrail() {
    let Some(server) = start_server(4).await else {
        eprintln!("skipping: local TCP bind is not permitted in this environment");
        return;
    };
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{}/chat", server.bind_addr))
        .json(&json!({ "message": "hello" }))
        .send()
        .await
        .expect("HTTP request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("HTTP error body should be valid JSON");
    let error = body
        .get("error")
        .and_then(|value| value.as_str())
        .expect("error field should be a string");
    assert!(
        error.contains("AGENT_MAX_INPUT_CHARS"),
        "expected AGENT_MAX_INPUT_CHARS in error, got: {error}"
    );

    let output = run_cli_chat_json("hello", 4);
    assert!(
        !output.status.success(),
        "CLI should fail for oversized input"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AGENT_MAX_INPUT_CHARS"),
        "expected AGENT_MAX_INPUT_CHARS in CLI stderr, got: {stderr}"
    );
}

#[tokio::test]
async fn http_returns_bad_gateway_for_unreachable_model() {
    let Some(server) = start_server(4000).await else {
        eprintln!("skipping: local TCP bind is not permitted in this environment");
        return;
    };
    let client = reqwest::Client::new();

    let response = client
        .post(format!("http://{}/chat", server.bind_addr))
        .json(&json!({ "message": "hi" }))
        .send()
        .await
        .expect("HTTP request should complete");

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body: serde_json::Value = response
        .json()
        .await
        .expect("HTTP error body should be valid JSON");
    let error = body
        .get("error")
        .and_then(|value| value.as_str())
        .expect("error field should be a string");
    assert!(
        error.contains("model chat failed"),
        "expected upstream model failure in error, got: {error}"
    );
}

async fn start_server(max_input_chars: u32) -> Option<RunningServer> {
    let port = find_available_port()?;
    let bind_addr = format!("127.0.0.1:{port}");
    let notes_dir = unique_temp_path("integration-notes");
    let log_dir = unique_temp_path("integration-logs");
    fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
    fs::create_dir_all(&log_dir).expect("log dir should be creatable");

    let mut command = Command::new(bin_path());
    command
        .args(["serve", "--bind", &bind_addr])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_test_env(
        &mut command,
        &notes_dir,
        &log_dir,
        max_input_chars,
        "http://127.0.0.1:9",
    );

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
        Err(error) => panic!("server should start: {error}"),
    };

    let health_url = format!("http://{bind_addr}/health");
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Some(status) = child.try_wait().expect("failed to poll server process") {
            panic!("server exited before becoming healthy: {status}");
        }

        if let Ok(response) = client.get(&health_url).send().await
            && response.status().is_success()
        {
            return Some(RunningServer {
                child,
                bind_addr,
                notes_dir,
                log_dir,
            });
        }

        sleep(Duration::from_millis(50)).await;
    }

    let _ = child.kill();
    let _ = child.wait();
    panic!("server did not become healthy at {health_url}");
}

fn run_cli_chat_json(message: &str, max_input_chars: u32) -> Output {
    let notes_dir = unique_temp_path("integration-cli-notes");
    let log_dir = unique_temp_path("integration-cli-logs");
    fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
    fs::create_dir_all(&log_dir).expect("log dir should be creatable");

    let mut command = Command::new(bin_path());
    command.args(["chat", message, "--json"]);
    apply_test_env(
        &mut command,
        &notes_dir,
        &log_dir,
        max_input_chars,
        "http://127.0.0.1:9",
    );

    let output = command.output().expect("CLI command should execute");

    let _ = fs::remove_dir_all(notes_dir);
    let _ = fs::remove_dir_all(log_dir);
    output
}

fn apply_test_env(
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

fn find_available_port() -> Option<u16> {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
        Err(error) => panic!("ephemeral port should be available for bind: {error}"),
    };
    let port = listener
        .local_addr()
        .expect("ephemeral listener should have local address")
        .port();
    drop(listener);
    Some(port)
}

fn unique_temp_path(prefix: &str) -> PathBuf {
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

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mjolne_vibes")
}
