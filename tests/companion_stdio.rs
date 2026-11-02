//! Local MCP process boundary: protocol-only output, scoped reads, trust, and EOF cleanup.

use anyhow::Context;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

fn command(workspace: &TempDir, home: &TempDir) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    let _configured = command
        .arg("--workspace")
        .arg(workspace.path())
        .args(["--quiet", "--provider", "openai", "--model", "gpt-5", "webmcp", "stdio"])
        .current_dir(workspace.path())
        .env("HOME", home.path())
        .env("VTCODE_CONFIG", home.path())
        .env_remove("VTCODE_CONFIG_PATH")
        .env("VTCODE_TRUST_WORKSPACE", "deny")
        .env("OPENAI_API_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn request(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    value: Value,
) -> anyhow::Result<Value> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await?;
    let expected_id = value.get("id").context("MCP request requires an ID")?;
    loop {
        let mut line = String::new();
        let count = tokio::time::timeout(Duration::from_secs(10), stdout.read_line(&mut line))
            .await
            .context("MCP response deadline")??;
        anyhow::ensure!(count != 0, "unexpected MCP EOF");
        let response: Value = serde_json::from_str(&line).context("stdout must contain only JSON-RPC")?;
        if response.get("id") == Some(expected_id) {
            return Ok(response);
        }
    }
}

#[tokio::test]
async fn companion_stdio_reads_only_bound_files_and_exits_on_eof() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    std::fs::write(workspace.path().join("README.md"), "local fixture 17\n").unwrap();
    std::fs::write(workspace.path().join(".env"), "PRIVATE_MARKER=hidden").unwrap();
    let mut child = command(&workspace, &home).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let initialized = request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
                "protocolVersion":"2025-03-26", "capabilities":{},
                "clientInfo":{"name":"companion-stdio-test","version":"0.1.0"}
            }
        }),
    )
    .await
    .unwrap();
    assert!(initialized["result"]["capabilities"]["tools"].is_object());
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let listing = request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc":"2.0", "id":2, "method":"tools/list"
        }),
    )
    .await
    .unwrap();
    let mut names: Vec<_> = listing["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["fetch", "search"]);
    for (id, path, allowed) in [(3, "README.md", true), (4, ".env", false), (5, "../README.md", false)] {
        let response = request(
            &mut stdin,
            &mut stdout,
            json!({
                "jsonrpc":"2.0", "id":id, "method":"tools/call",
                "params":{"name":"fetch","arguments":{"id":path}}
            }),
        )
        .await
        .unwrap();
        if allowed {
            assert_eq!(response["result"]["structuredContent"]["text"], "local fixture 17\n");
        } else {
            assert!(response.get("error").is_some() || response["result"]["isError"] == true);
            assert!(!response.to_string().contains("PRIVATE_MARKER"));
        }
    }
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("EOF must stop the local service")
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read_to_string(workspace.path().join("README.md")).unwrap(), "local fixture 17\n");
}

#[tokio::test]
async fn companion_stdio_cannot_grant_workspace_trust() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let mut command = command(&workspace, &home);
    let _configured = command
        .args(["--companion", "--config", "automation.full_auto.enabled=true"])
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("persisted full-auto workspace trust"));
}
