use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdout};
use tokio::time::timeout;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{resolve_copilot_command, spawn_copilot_server_process};
use super::types::CopilotDiscoveredModel;

pub async fn list_available_models(
    config: &CopilotAuthConfig,
    workspace_root: &Path,
) -> Result<Vec<CopilotDiscoveredModel>> {
    let resolved = resolve_copilot_command(config)?;
    let mut child = spawn_copilot_server_process(&resolved, workspace_root)?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("copilot cli server stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("copilot cli server stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("copilot cli server stderr unavailable"))?;

    spawn_server_stderr(stderr);

    let result = timeout(resolved.startup_timeout, async move {
        let mut writer = stdin;
        let mut reader = BufReader::new(stdout);

        send_request(
            &mut writer,
            1,
            "ping",
            Some(json!({ "message": "vtcode model discovery" })),
        )
        .await
        .context("copilot cli ping")?;
        let ping = read_response(&mut reader, 1)
            .await
            .context("copilot cli ping")?;
        let protocol_version = ping
            .get("protocolVersion")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if protocol_version <= 0 {
            return Err(anyhow!(
                "copilot cli server did not report a protocol version"
            ));
        }

        send_request(&mut writer, 2, "models.list", None)
            .await
            .context("copilot cli models.list")?;
        let payload = read_response(&mut reader, 2)
            .await
            .context("copilot cli models.list")?;
        let models = payload
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("copilot cli models.list response missing models"))?;

        let mut discovered = Vec::new();
        for model in models {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let name = model
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let policy_enabled = model
                .get("policy")
                .and_then(Value::as_object)
                .and_then(|policy| policy.get("state"))
                .and_then(Value::as_str)
                .map(|state| state.eq_ignore_ascii_case("enabled"))
                .unwrap_or(true);
            if !policy_enabled {
                continue;
            }
            let Some(id) = id else {
                continue;
            };
            discovered.push(CopilotDiscoveredModel {
                id: id.to_string(),
                name: name.unwrap_or(id).to_string(),
            });
        }

        discovered.sort_by(|left, right| left.id.cmp(&right.id));
        discovered.dedup_by(|left, right| left.id.eq_ignore_ascii_case(&right.id));
        Ok::<Vec<CopilotDiscoveredModel>, anyhow::Error>(discovered)
    })
    .await
    .context("copilot cli model discovery timeout")??;

    let _ = child.start_kill();
    Ok(result)
}

fn spawn_server_stderr(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                tracing::debug!(target: "copilot.server.stderr", "{}", trimmed);
            }
        }
    });
}

async fn send_request<W>(writer: &mut W, id: i64, method: &str, params: Option<Value>) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = if let Some(params) = params {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
    } else {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        })
    };
    let payload = serde_json::to_vec(&message).context("copilot cli json serialization failed")?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", payload.len()).as_bytes())
        .await
        .context("copilot cli write header failed")?;
    writer
        .write_all(&payload)
        .await
        .context("copilot cli write payload failed")?;
    writer.flush().await.context("copilot cli flush failed")?;
    Ok(())
}

async fn read_response(reader: &mut BufReader<ChildStdout>, expected_id: i64) -> Result<Value> {
    loop {
        let message = read_message(reader).await?;
        let Some(object) = message.as_object() else {
            continue;
        };

        if object.get("method").is_some() {
            continue;
        }

        if let Some(error) = object.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let detail = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(anyhow!("copilot cli rpc error {code}: {detail}"));
        }

        if object.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }

        return object
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("copilot cli rpc response missing result"));
    }
}

async fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .context("copilot cli header read failed")?;
        if read == 0 {
            return Err(anyhow!("copilot cli server closed the stdio stream"));
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("invalid copilot cli content length header")?,
            );
        }
    }

    let content_length =
        content_length.ok_or_else(|| anyhow!("copilot cli response missing Content-Length"))?;
    let mut payload = vec![0_u8; content_length];
    reader
        .read_exact(&mut payload)
        .await
        .context("copilot cli payload read failed")?;
    serde_json::from_slice(&payload).context("copilot cli json decode failed")
}
