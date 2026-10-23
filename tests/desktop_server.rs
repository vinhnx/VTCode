//! End-to-end checks for the desktop service process boundary.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::process::Command;
use tokio::time::timeout;
use vtcode_desktop_protocol as protocol;

#[tokio::test]
async fn desktop_server_uses_json_only_stdout_and_creates_a_headless_session() {
    let workspace = TempDir::new().expect("workspace");
    let home = TempDir::new().expect("isolated home");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    let _configured_command = child
        .args([
            "--provider",
            "ollama",
            "--model",
            "gpt-oss:20b",
            "desktop-server",
            "--workspace",
        ])
        .arg(workspace.path())
        .current_dir(workspace.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("VT_SESSION_DIR", home.path().join("sessions"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = child.spawn().expect("spawn desktop service");
    let mut stdin = child.stdin.take().expect("service stdin");
    let requests = [
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": protocol::method::INITIALIZE,
            "params": {
                "protocol": {
                    "minimum": protocol::PROTOCOL_VERSION,
                    "maximum": protocol::PROTOCOL_VERSION
                },
                "clientName": "desktop-server-test",
                "clientVersion": "test"
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": protocol::method::SESSION_CREATE,
            "params": { "title": "Headless service test" }
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": protocol::method::SHUTDOWN,
            "params": {}
        }),
    ];
    for request in &requests {
        protocol::write_frame(&mut stdin, request)
            .await
            .expect("write protocol request");
    }
    drop(stdin);

    let output = timeout(Duration::from_secs(45), child.wait_with_output())
        .await
        .expect("service shutdown timeout")
        .expect("wait for service");
    assert!(
        output.status.success(),
        "desktop service exited with {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = std::str::from_utf8(&output.stdout)
        .expect("stdout is UTF-8 JSON")
        .lines()
        .collect::<Vec<_>>();
    assert!(!lines.is_empty(), "service should answer protocol requests");
    let messages = lines
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("every stdout line is one JSON frame"))
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message["id"] == 1 && message["result"]["protocol"]["major"] == 1)
    );
    assert!(
        messages
            .iter()
            .any(|message| { message["id"] == 2 && message["result"]["sessionId"].as_str().is_some() })
    );
    assert!(
        messages
            .iter()
            .any(|message| message["id"] == 3 && message["result"]["shutdown"] == true)
    );
}
