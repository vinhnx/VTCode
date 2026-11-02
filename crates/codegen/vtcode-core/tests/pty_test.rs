#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use serde_json::json;
use std::fs;
use tempfile::TempDir;
use vtcode_config::CommandsConfig;
use vtcode_core::tools::ToolRegistry;

async fn temp_registry() -> (TempDir, ToolRegistry) {
    temp_registry_with_config(None).await
}

async fn temp_registry_with_config(vtcode_toml: Option<&str>) -> (TempDir, ToolRegistry) {
    let temp = TempDir::new().expect("temp workspace");
    fs::write(temp.path().join("Cargo.toml"), "[package]\nname = \"pty-test\"\n").expect("write fixture Cargo.toml");
    if let Some(config) = vtcode_toml {
        fs::write(temp.path().join("vtcode.toml"), config).expect("write fixture vtcode.toml");
    }
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let _allow_result = registry.allow_all_tools().await.ok();
    (temp, registry)
}

#[cfg(unix)]
fn exec_session_id(response: &serde_json::Value) -> String {
    response
        .get("process_id")
        .and_then(|value| value.as_str())
        .or_else(|| response.get("session_id").and_then(|value| value.as_str()))
        .expect("session id should be present")
        .to_string()
}

#[cfg(unix)]
async fn read_session_until_exit(
    registry: &ToolRegistry,
    session_id: &str,
    attempts: usize,
    yield_time_ms: u64,
) -> (String, serde_json::Value) {
    let mut output = String::new();
    let mut last = json!({});

    for attempt in 0..attempts {
        let read = registry
            .execute_tool(
                "read_pty_session",
                json!({
                    "session_id": session_id,
                    "yield_time_ms": yield_time_ms + attempt as u64,
                }),
            )
            .await
            .expect("read pty session");

        output.push_str(read.get("output").and_then(|value| value.as_str()).unwrap_or_default());
        if read.get("is_exited").and_then(|value| value.as_bool()).unwrap_or(false) || read.get("error").is_some() {
            return (output, read);
        }
        last = read;
    }

    (output, last)
}

#[path = "pty_test/basics.rs"]
mod basics;
#[path = "pty_test/session_read.rs"]
mod session_read;
#[path = "pty_test/spool.rs"]
mod spool;
#[path = "pty_test/write_stdin.rs"]
mod write_stdin;
