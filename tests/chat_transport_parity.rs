use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

use mjolne_vibes::test_support::{apply_ollama_test_env, remove_dir_if_exists, temp_path};
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
        remove_dir_if_exists(&self.notes_dir);
        remove_dir_if_exists(&self.log_dir);
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
    let notes_dir = temp_path("integration-notes");
    let log_dir = temp_path("integration-logs");
    fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
    fs::create_dir_all(&log_dir).expect("log dir should be creatable");

    let mut command = Command::new(bin_path());
    command
        .args(["serve", "--bind", &bind_addr])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    apply_ollama_test_env(
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
    let notes_dir = temp_path("integration-cli-notes");
    let log_dir = temp_path("integration-cli-logs");
    fs::create_dir_all(&notes_dir).expect("notes dir should be creatable");
    fs::create_dir_all(&log_dir).expect("log dir should be creatable");

    let mut command = Command::new(bin_path());
    command.args(["chat", message, "--json"]);
    apply_ollama_test_env(
        &mut command,
        &notes_dir,
        &log_dir,
        max_input_chars,
        "http://127.0.0.1:9",
    );

    let output = command.output().expect("CLI command should execute");

    remove_dir_if_exists(&notes_dir);
    remove_dir_if_exists(&log_dir);
    output
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

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_mjolne_vibes")
}
