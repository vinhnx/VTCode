use super::*;

pub(super) fn handle_terminal_create_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        parse_terminal_create_request,
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalCreate(PendingTerminalCreateRequest { request, response_tx })
        },
        |response| RpcReply::result(build_terminal_create_result(response)),
        RpcReply::runtime_error("VT Code could not create the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not create the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_create.respond",
    )
}

pub(super) fn handle_terminal_output_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| CopilotTerminalOutputRequest {
                session_id,
                terminal_id,
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalOutput(PendingTerminalOutputRequest { request, response_tx })
        },
        |response| RpcReply::result(build_terminal_output_result(response)),
        RpcReply::runtime_error("VT Code could not read the requested terminal output."),
        RpcReply::runtime_error(
            "VT Code could not read the requested terminal output because the Copilot runtime is unavailable.",
        ),
        "terminal_output.respond",
    )
}

pub(super) fn handle_terminal_release_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| CopilotTerminalReleaseRequest {
                session_id,
                terminal_id,
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalRelease(PendingTerminalReleaseRequest { request, response_tx })
        },
        |_| RpcReply::result(json!({})),
        RpcReply::runtime_error("VT Code could not release the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not release the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_release.respond",
    )
}

pub(super) fn handle_terminal_kill_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| CopilotTerminalKillRequest {
                session_id,
                terminal_id,
            })
        },
        |request, response_tx| CopilotRuntimeRequest::TerminalKill(PendingTerminalKillRequest { request, response_tx }),
        |_| RpcReply::result(json!({})),
        RpcReply::runtime_error("VT Code could not kill the requested terminal command."),
        RpcReply::runtime_error(
            "VT Code could not kill the requested terminal command because the Copilot runtime is unavailable.",
        ),
        "terminal_kill.respond",
    )
}

pub(super) fn handle_terminal_wait_for_exit_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| CopilotTerminalWaitForExitRequest {
                session_id,
                terminal_id,
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalWaitForExit(PendingTerminalWaitForExitRequest { request, response_tx })
        },
        |response| RpcReply::result(build_terminal_wait_for_exit_result(response)),
        RpcReply::runtime_error("VT Code could not wait for the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not wait for the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_wait_for_exit.respond",
    )
}

fn parse_terminal_create_request(params: &Value) -> Result<CopilotTerminalCreateRequest> {
    let session_id = optional_session_id(params);
    let command = required_non_empty_string(params, "command", "copilot terminal/create missing command")?;
    let args = parse_string_array(
        params.get("args"),
        MAX_TERMINAL_ARG_COUNT,
        "copilot terminal args must be strings",
        "copilot terminal/create has too many args",
    )?;
    let env = parse_terminal_env_vars(params.get("env"))?;
    let cwd = params.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let output_byte_limit = params.get("outputByteLimit").and_then(Value::as_u64).map(|value| {
        usize::try_from(value)
            .unwrap_or(MAX_TERMINAL_OUTPUT_BYTE_LIMIT)
            .min(MAX_TERMINAL_OUTPUT_BYTE_LIMIT)
    });

    Ok(CopilotTerminalCreateRequest {
        session_id,
        command,
        args,
        env,
        cwd,
        output_byte_limit,
    })
}

fn parse_terminal_request<T>(params: &Value, build: impl FnOnce(String, String) -> T) -> Result<T> {
    let session_id = optional_session_id(params);
    let terminal_id = required_non_empty_string(params, "terminalId", "copilot terminal request missing terminalId")?;
    Ok(build(session_id, terminal_id))
}

fn optional_session_id(params: &Value) -> String {
    params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn required_non_empty_string(params: &Value, key: &str, error_message: &'static str) -> Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!(error_message))
}

fn parse_string_array(
    value: Option<&Value>,
    max_items: usize,
    item_error: &'static str,
    limit_error: &'static str,
) -> Result<Vec<String>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    if values.len() > max_items {
        anyhow::bail!(limit_error);
    }

    values
        .iter()
        .map(|value| value.as_str().map(str::to_string).ok_or_else(|| anyhow!(item_error)))
        .collect()
}

fn parse_terminal_env_vars(value: Option<&Value>) -> Result<Vec<CopilotTerminalEnvVar>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    if values.len() > MAX_TERMINAL_ENV_VAR_COUNT {
        anyhow::bail!("copilot terminal/create has too many env entries");
    }

    values
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .ok_or_else(|| anyhow!("copilot terminal env entries must be objects"))?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| anyhow!("copilot terminal env entries require a name"))?;
            let value = object
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| anyhow!("copilot terminal env entries require a value"))?;
            Ok(CopilotTerminalEnvVar { name, value })
        })
        .collect()
}

fn build_terminal_create_result(response: CopilotTerminalCreateResponse) -> Value {
    json!({
        "terminalId": response.terminal_id,
    })
}

fn build_terminal_output_result(response: CopilotTerminalOutputResponse) -> Value {
    let exit_status = response.exit_status.map(build_terminal_exit_status_json);
    let mut result = serde_json::Map::from_iter([
        ("output".to_string(), Value::String(response.output)),
        ("truncated".to_string(), Value::Bool(response.truncated)),
    ]);
    if let Some(exit_status) = exit_status {
        result.insert("exitStatus".to_string(), exit_status);
    }
    Value::Object(result)
}

fn build_terminal_wait_for_exit_result(response: CopilotTerminalExitStatus) -> Value {
    build_terminal_exit_status_json(response)
}

fn build_terminal_exit_status_json(status: CopilotTerminalExitStatus) -> Value {
    let mut result = serde_json::Map::new();
    result.insert("exitCode".to_string(), status.exit_code.map_or(Value::Null, |value| Value::from(u64::from(value))));
    result.insert("signal".to_string(), status.signal.map_or(Value::Null, Value::String));
    Value::Object(result)
}
