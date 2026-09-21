//! The Streamable HTTP transport, and the one guard in front of it.
//!
//! An HTTP MCP endpoint holding a team's API key is worth protecting, so the
//! binary refuses to bind off loopback without a token, and answers 401 without
//! one. Both are checked here against the real process.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use std::{
    net::TcpListener,
    process::Stdio,
    sync::atomic::{AtomicU16, Ordering},
    time::Duration,
};

use common::{Fake, env};
use serde_json::json;
use tokio::process::{Child, Command};

/// Ports handed out so far, so two tests in this process never pick the same
/// one. Asking the operating system for port 0 twice in a row can answer the
/// same number, and then one server wins the bind and the other test talks to
/// it — a flake that looks like an authorisation bug.
static HANDED_OUT: AtomicU16 = AtomicU16::new(0);

/// A port this process has not used, and nothing else is listening on.
fn free_port() -> u16 {
    for _ in 0..500 {
        let candidate = 19_000 + HANDED_OUT.fetch_add(1, Ordering::Relaxed);
        if TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return candidate;
        }
    }
    panic!("no free port in the test range");
}

/// Spawn the binary with the HTTP transport and wait for it to answer.
async fn serve(vars: &[(String, String)], bind: &str) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"));
    command
        .arg("--http")
        .arg(bind)
        .env_clear()
        .env("GRYZZLY_LOG", "warn")
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (key, value) in vars {
        command.env(key, value);
    }
    let child = command.spawn().expect("the binary runs");

    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(response) = client
            .get(format!("http://{bind}/healthz"))
            .timeout(Duration::from_millis(200))
            .send()
            .await
            && response.status().is_success()
        {
            return child;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server never answered on {bind}");
}

#[tokio::test]
async fn the_health_probe_answers_without_a_token() {
    let fake = Fake::start().await;
    let bind = format!("127.0.0.1:{}", free_port());
    let mut child = serve(&env(&fake, &[("GRYZZLY_HTTP_TOKEN", "letmein")]), &bind).await;

    let body: serde_json::Value = reqwest::get(format!("http://{bind}/healthz"))
        .await
        .expect("healthz")
        .json()
        .await
        .expect("json");
    assert_eq!(body["status"], "ok");
    let _ = child.kill().await;
}

#[tokio::test]
async fn the_endpoint_refuses_a_request_without_the_bearer_token() {
    let fake = Fake::start().await;
    let bind = format!("127.0.0.1:{}", free_port());
    let mut child = serve(&env(&fake, &[("GRYZZLY_HTTP_TOKEN", "letmein")]), &bind).await;

    let response = reqwest::Client::new()
        .post(format!("http://{bind}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(initialize())
        .send()
        .await
        .expect("a response");
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(
        response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("Bearer")),
        "the 401 must say how to authenticate"
    );
    let _ = child.kill().await;
}

#[tokio::test]
async fn the_endpoint_serves_the_handshake_with_the_bearer_token() {
    let fake = Fake::start().await;
    let bind = format!("127.0.0.1:{}", free_port());
    let mut child = serve(&env(&fake, &[("GRYZZLY_HTTP_TOKEN", "letmein")]), &bind).await;

    let response = reqwest::Client::new()
        .post(format!("http://{bind}/mcp"))
        .header("authorization", "Bearer letmein")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(initialize())
        .send()
        .await
        .expect("a response");
    assert!(response.status().is_success(), "{:?}", response.status());
    let body = response.text().await.expect("a body");
    assert!(body.contains("protocolVersion"), "{body}");
    assert!(body.contains("gryzzly"), "{body}");
    let _ = child.kill().await;
}

#[tokio::test]
async fn binding_off_loopback_without_a_token_refuses_to_start() {
    let fake = Fake::start().await;
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"))
        .arg("--http")
        .arg("0.0.0.0:65001")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GRYZZLY_API_KEY", "test-key")
        .env("GRYZZLY_BASE_URL", fake.base_url())
        .output()
        .await
        .expect("the binary runs");
    assert!(!output.status.success(), "it must refuse to serve");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("GRYZZLY_HTTP_TOKEN"), "{stderr}");
}

#[tokio::test]
async fn check_reports_the_configuration_it_resolved() {
    let fake = Fake::start().await;
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"))
        .arg("--check")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GRYZZLY_API_KEY", "test-key")
        .env("GRYZZLY_BASE_URL", fake.base_url())
        .env("GRYZZLY_USER_ID", common::USER_ID)
        .output()
        .await
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("acting as    Edouard Claude"), "{stdout}");
    assert!(
        stdout.contains("53"),
        "the tool count is reported: {stdout}"
    );
    assert!(stdout.contains("ready."), "{stdout}");
}

#[tokio::test]
async fn check_fails_loudly_when_the_user_cannot_be_resolved() {
    let fake = Fake::start().await;
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"))
        .arg("--check")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GRYZZLY_API_KEY", "test-key")
        .env("GRYZZLY_BASE_URL", fake.base_url())
        .env("GRYZZLY_USER_EMAIL", "nobody@example.test")
        .output()
        .await
        .expect("the binary runs");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("unknown") || stderr.contains("GRYZZLY_USER_EMAIL"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
}

#[tokio::test]
async fn list_tools_prints_the_surface_without_touching_the_api() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-gryzzly"))
        .arg("--list-tools")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GRYZZLY_API_KEY", "test-key")
        .output()
        .await
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.starts_with("53 tools"), "{stdout}");
    assert!(stdout.contains("log_time"), "{stdout}");
    assert!(stdout.contains("DEL   delete_time_entry"), "{stdout}");
}

fn initialize() -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "mcp-gryzzly-tests", "version": "0" }
        }
    })
    .to_string()
}
