use super::*;
use crate::copilot::transport::StdioTransport;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

fn make_inner(write_tx: mpsc::UnboundedSender<String>) -> Arc<CopilotAcpClientInner> {
    Arc::new(CopilotAcpClientInner {
        transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
        active_prompt: StdMutex::new(None),
        session_id: StdMutex::new(None),
        compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
    })
}

#[test]
fn extracts_text_from_text_objects() {
    let text = extract_text(Some(&json!({
        "type": "text",
        "text": "hello",
    })));

    assert_eq!(text.as_deref(), Some("hello"));
}

#[test]
fn formats_unsupported_capability_message() {
    let message = unsupported_client_capability_message("tool_call");

    assert!(message.contains("does not implement"));
    assert!(message.contains("tool_call"));
}

#[test]
fn permission_render_denies_without_prompt() {
    let result = PermissionResponseFormat::CopilotCli
        .render(CopilotPermissionDecision::DeniedNoApprovalRule);

    assert_eq!(
        result["result"]["kind"],
        "denied-no-approval-rule-and-could-not-request-from-user"
    );
}

#[test]
fn legacy_permission_payload_selects_session_approval_option() {
    let outcome = legacy_permission_outcome(
        &[
            AcpPermissionOption {
                option_id: "allow-once".to_string(),
                kind: AcpPermissionOptionKind::AllowOnce,
            },
            AcpPermissionOption {
                option_id: "allow-always".to_string(),
                kind: AcpPermissionOptionKind::AllowAlways,
            },
        ],
        &CopilotPermissionDecision::ApprovedAlways,
    );

    assert_eq!(outcome["outcome"], "selected");
    assert_eq!(outcome["optionId"], "allow-always");
}

#[test]
fn tool_call_result_returns_failure_structure() {
    let result = build_tool_call_result(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: "failed".to_string(),
        error: "boom".to_string(),
    }));

    assert_eq!(result["result"]["resultType"], "failure");
    assert_eq!(result["result"]["error"], "boom");
}

#[test]
fn parses_shell_permission_request() {
    let request = parse_permission_request(json!({
        "kind": "shell",
        "toolCallId": "call_1",
        "fullCommandText": "git status",
        "intention": "inspect repository state",
        "commands": [{ "identifier": "git", "readOnly": true }],
        "possiblePaths": ["./"],
        "possibleUrls": [{ "url": "https://github.com" }],
        "hasWriteFileRedirection": false,
        "canOfferSessionApproval": true
    }))
    .unwrap();

    match request {
        CopilotPermissionRequest::Shell {
            full_command_text,
            possible_paths,
            possible_urls,
            can_offer_session_approval,
            ..
        } => {
            assert_eq!(full_command_text, "git status");
            assert_eq!(possible_paths, vec!["./"]);
            assert_eq!(possible_urls, vec!["https://github.com"]);
            assert!(can_offer_session_approval);
        }
        other => panic!("unexpected request: {other:?}"),
    }
}

#[test]
fn custom_tools_payload_marks_skip_permission() {
    let payload = custom_tools_payload(&[ToolDefinition::function(
        "demo_tool".to_string(),
        "Run demo".to_string(),
        json!({"type": "object"}),
    )]);

    assert_eq!(payload.len(), 1);
    assert_eq!(payload[0]["name"], "demo_tool");
    assert_eq!(payload[0]["skipPermission"], true);
}

#[test]
fn string_array_ignores_non_string_values() {
    let values = string_array(Some(&json!(["a", 1, "b"])));
    assert_eq!(values, vec!["a", "b"]);
}

#[test]
fn parses_observed_tool_call_updates() {
    let observed = parse_observed_tool_call(&json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_9",
        "title": "Reading configuration file",
        "status": "completed",
        "rawInput": { "path": "vtcode.toml" },
        "content": [
            {
                "type": "content",
                "content": {
                    "type": "text",
                    "text": "Done"
                }
            }
        ]
    }))
    .expect("observed tool call");

    assert_eq!(observed.tool_call_id, "call_9");
    assert_eq!(observed.tool_name, "Reading configuration file");
    assert_eq!(observed.status, CopilotObservedToolCallStatus::Completed);
    assert_eq!(observed.output.as_deref(), Some("Done"));
    assert_eq!(observed.terminal_id, None);
}

#[test]
fn parses_observed_tool_call_terminal_content() {
    let observed = parse_observed_tool_call(&json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_terminal",
        "title": "Run cargo check",
        "status": "in_progress",
        "content": [
            {
                "type": "content",
                "content": {
                    "type": "terminal",
                    "terminalId": "run-123"
                }
            }
        ]
    }))
    .expect("observed terminal tool call");

    assert_eq!(observed.terminal_id.as_deref(), Some("run-123"));
}

#[test]
fn parses_observed_tool_call_raw_output_text_payload() {
    let observed = parse_observed_tool_call(&json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_output",
        "title": "Run cargo check",
        "status": "completed",
        "rawOutput": {
            "content": "Checking vtcode\nFinished `dev` profile\n<exited with exit code 0>",
            "detailedContent": "ignored fallback"
        }
    }))
    .expect("observed raw output");

    assert_eq!(
        observed.output.as_deref(),
        Some("Checking vtcode\nFinished `dev` profile\n<exited with exit code 0>")
    );
}

#[test]
fn parses_observed_tool_call_raw_output_detailed_content_fallback() {
    let observed = parse_observed_tool_call(&json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_output_fallback",
        "title": "Run cargo fmt",
        "status": "completed",
        "rawOutput": {
            "content": "",
            "detailedContent": "\n<exited with exit code 0>"
        }
    }))
    .expect("observed raw output fallback");

    assert_eq!(
        observed.output.as_deref(),
        Some("\n<exited with exit code 0>")
    );
}

#[tokio::test]
async fn handle_terminal_create_request_dispatches_runtime_request() {
    let (write_tx, mut write_rx) = mpsc::unbounded_channel();
    let (updates, _updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
    let (runtime_requests, mut runtime_requests_rx) =
        mpsc::unbounded_channel::<CopilotRuntimeRequest>();

    let inner = Arc::new(CopilotAcpClientInner {
        transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
        active_prompt: StdMutex::new(Some(ActivePrompt {
            updates,
            runtime_requests,
        })),
        session_id: StdMutex::new(None),
        compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
    });

    handle_terminal_create_request(
        &inner,
        &json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "terminal/create",
            "params": {
                "sessionId": "session_1",
                "command": "cargo",
                "args": ["check"],
                "env": [
                    {
                        "name": "RUST_LOG",
                        "value": "debug"
                    }
                ],
                "cwd": "/tmp/acp"
            }
        }),
    )
    .expect("dispatch terminal/create");

    let runtime_request = runtime_requests_rx
        .recv()
        .await
        .expect("runtime request available");
    let CopilotRuntimeRequest::TerminalCreate(request) = runtime_request else {
        panic!("expected terminal/create runtime request");
    };
    assert_eq!(request.request.session_id, "session_1");
    assert_eq!(request.request.command, "cargo");
    assert_eq!(request.request.args, vec!["check"]);
    assert_eq!(
        request.request.env,
        vec![CopilotTerminalEnvVar {
            name: "RUST_LOG".to_string(),
            value: "debug".to_string(),
        }]
    );
    request
        .respond(CopilotTerminalCreateResponse {
            terminal_id: "run-123".to_string(),
        })
        .expect("respond terminal/create");

    let payload = timeout(Duration::from_secs(1), write_rx.recv())
        .await
        .expect("terminal/create response timeout")
        .expect("terminal/create response payload");
    let payload: Value = serde_json::from_str(&payload).expect("valid json payload");
    assert_eq!(payload["id"], 12);
    assert_eq!(payload["result"]["terminalId"], "run-123");
}

#[test]
fn enqueue_runtime_request_clears_stale_active_prompt_when_receiver_is_gone() {
    let (write_tx, _write_rx) = mpsc::unbounded_channel();
    let (updates, updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
    let (runtime_requests, runtime_requests_rx) =
        mpsc::unbounded_channel::<CopilotRuntimeRequest>();
    drop(updates_rx);
    drop(runtime_requests_rx);

    let inner = Arc::new(CopilotAcpClientInner {
        transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
        active_prompt: StdMutex::new(Some(ActivePrompt {
            updates,
            runtime_requests,
        })),
        session_id: StdMutex::new(None),
        compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
    });

    let err = enqueue_runtime_request(
        &inner,
        CopilotRuntimeRequest::CompatibilityNotice(CopilotCompatibilityNotice {
            state: CopilotAcpCompatibilityState::PromptOnly,
            message: "prompt-only degraded mode".to_string(),
        }),
    )
    .expect_err("closed runtime receiver should fail");

    assert!(
        err.to_string()
            .contains("copilot runtime request channel closed")
    );
    assert!(
        inner
            .active_prompt
            .lock()
            .expect("active_prompt lock")
            .is_none()
    );
}

#[test]
fn handle_permission_request_falls_back_when_runtime_receiver_is_gone() {
    let (write_tx, mut write_rx) = mpsc::unbounded_channel();
    let (updates, _updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
    let (runtime_requests, runtime_requests_rx) =
        mpsc::unbounded_channel::<CopilotRuntimeRequest>();
    drop(runtime_requests_rx);

    let inner = Arc::new(CopilotAcpClientInner {
        transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
        active_prompt: StdMutex::new(Some(ActivePrompt {
            updates,
            runtime_requests,
        })),
        session_id: StdMutex::new(None),
        compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
    });

    handle_permission_request(
        &inner,
        &json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "permission.request",
            "params": {
                "permissionRequest": {
                    "kind": "shell",
                    "fullCommandText": "git status",
                    "intention": "inspect repository state"
                }
            }
        }),
    )
    .expect("stale runtime receiver should fall back cleanly");

    let payload = write_rx.try_recv().expect("fallback response payload");
    let payload: Value = serde_json::from_str(&payload).expect("valid json payload");
    assert_eq!(payload["jsonrpc"], "2.0");
    assert_eq!(payload["id"], 9);
    assert_eq!(
        payload["result"]["result"]["kind"],
        "denied-no-approval-rule-and-could-not-request-from-user"
    );
}

#[tokio::test]
async fn prompt_session_cancel_handle_cancels_active_prompt_and_aborts_completion() {
    let (write_tx, mut write_rx) = mpsc::unbounded_channel();
    let (updates, _updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
    let (runtime_requests, _runtime_requests_rx) =
        mpsc::unbounded_channel::<CopilotRuntimeRequest>();

    let client = CopilotAcpClient {
        inner: Arc::new(CopilotAcpClientInner {
            transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
            active_prompt: StdMutex::new(Some(ActivePrompt {
                updates,
                runtime_requests,
            })),
            session_id: StdMutex::new(Some("session_123".to_string())),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
        }),
    };

    let completion = tokio::spawn(async {
        std::future::pending::<()>().await;
        Ok::<PromptCompletion, anyhow::Error>(PromptCompletion {
            stop_reason: "cancelled".to_string(),
        })
    });
    let abort_handle = completion.abort_handle();

    let cancel_handle = PromptSessionCancelHandle {
        client: client.clone(),
        completion_abort: abort_handle,
    };

    cancel_handle.cancel();

    let payload = write_rx.recv().await.expect("session cancel payload");
    let payload: Value = serde_json::from_str(&payload).expect("valid cancel payload");
    assert_eq!(payload["method"], "session/cancel");
    assert_eq!(payload["params"]["sessionId"], "session_123");
    assert!(
        client
            .inner
            .active_prompt
            .lock()
            .expect("active_prompt lock")
            .is_none()
    );

    let err = completion.await.expect_err("completion should be aborted");
    assert!(err.is_cancelled(), "expected cancelled task, got {err}");
}

#[test]
fn make_inner_helper_creates_valid_inner() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let inner = make_inner(tx);
    assert!(inner.active_prompt.lock().unwrap().is_none());
    assert!(inner.session_id.lock().unwrap().is_none());
    assert_eq!(
        *inner.compatibility_state.lock().unwrap(),
        CopilotAcpCompatibilityState::FullTools
    );
}
