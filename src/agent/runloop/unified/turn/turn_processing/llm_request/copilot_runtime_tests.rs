use super::{
    CopilotRuntimeHost, auto_approve_builtin_permission, denied_tool_response,
    filter_copilot_tools, harness_call_item_id, map_builtin_permission_prompt_decision,
    map_copilot_finish_reason, normalize_copilot_reasoning_delta, summarize_permission_request,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use vtcode_core::copilot::{
    CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionRequest,
    CopilotTerminalCreateRequest, CopilotTerminalEnvVar, CopilotToolCallResponse,
};
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider::{FinishReason, ToolDefinition};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::transcript;
use vtcode_tui::app::{InlineCommand, InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_routing::HitlDecision;
use tempfile::TempDir;
use tokio::sync::{Notify, RwLock, mpsc::unbounded_channel};
use vtcode_core::acp::ToolPermissionCache;

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = unbounded_channel();
    let (_event_tx, event_rx) = unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
    }
}

fn collect_inline_output(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::AppendLine { segments, .. } => {
                lines.push(
                    segments
                        .into_iter()
                        .map(|segment| segment.text)
                        .collect::<String>(),
                );
            }
            InlineCommand::ReplaceLast {
                lines: replacement_lines,
                ..
            } => {
                for line in replacement_lines {
                    lines.push(
                        line.into_iter()
                            .map(|segment| segment.text)
                            .collect::<String>(),
                    );
                }
            }
            _ => {}
        }
    }
    lines.join("\n")
}

#[test]
fn filter_copilot_tools_keeps_only_allowlisted_names() {
    let tools = Arc::new(vec![
        ToolDefinition::function(
            "unified_search".to_string(),
            "Search".to_string(),
            json!({"type": "object"}),
        ),
        ToolDefinition::function(
            "apply_patch".to_string(),
            "Patch".to_string(),
            json!({"type": "object"}),
        ),
    ]);

    let filtered = filter_copilot_tools(Some(&tools), &["unified_search".to_string()]);
    let names: Vec<_> = filtered
        .iter()
        .filter_map(|tool| {
            tool.function
                .as_ref()
                .map(|function| function.name.as_str())
        })
        .collect();

    assert_eq!(names, vec!["unified_search"]);
}

#[test]
fn summarize_shell_permission_request_uses_command_cache_key() {
    let summary = summarize_permission_request(&CopilotPermissionRequest::Shell {
        tool_call_id: Some("call_1".to_string()),
        full_command_text: "git status".to_string(),
        intention: "Inspect repository status".to_string(),
        commands: Vec::new(),
        possible_paths: vec!["/workspace".to_string()],
        possible_urls: Vec::new(),
        has_write_file_redirection: false,
        can_offer_session_approval: true,
        warning: None,
    })
    .expect("summary");

    assert_eq!(summary.display_name, "GitHub Copilot shell command");
    assert!(summary.cache_key.contains("\"prefix\":\"copilot:shell\""));
    assert!(summary.cache_key.contains("git status"));
}

#[test]
fn shell_permission_cache_key_scopes_paths_and_urls() {
    let first = summarize_permission_request(&CopilotPermissionRequest::Shell {
        tool_call_id: None,
        full_command_text: "git status".to_string(),
        intention: "Inspect repository status".to_string(),
        commands: Vec::new(),
        possible_paths: vec!["/workspace/a".to_string()],
        possible_urls: Vec::new(),
        has_write_file_redirection: false,
        can_offer_session_approval: true,
        warning: None,
    })
    .expect("first summary");
    let second = summarize_permission_request(&CopilotPermissionRequest::Shell {
        tool_call_id: None,
        full_command_text: "git status".to_string(),
        intention: "Inspect repository status".to_string(),
        commands: Vec::new(),
        possible_paths: vec!["/workspace/b".to_string()],
        possible_urls: vec!["https://example.com".to_string()],
        has_write_file_redirection: false,
        can_offer_session_approval: true,
        warning: None,
    })
    .expect("second summary");

    assert_ne!(first.cache_key, second.cache_key);
}

#[test]
fn custom_tool_permission_cache_key_scopes_arguments() {
    let first = summarize_permission_request(&CopilotPermissionRequest::CustomTool {
        tool_call_id: None,
        tool_name: "demo".to_string(),
        tool_description: "Run demo".to_string(),
        args: Some(json!({"path": "/tmp/a"})),
    })
    .expect("first summary");
    let second = summarize_permission_request(&CopilotPermissionRequest::CustomTool {
        tool_call_id: None,
        tool_name: "demo".to_string(),
        tool_description: "Run demo".to_string(),
        args: Some(json!({"path": "/tmp/b"})),
    })
    .expect("second summary");

    assert_ne!(first.cache_key, second.cache_key);
}

#[test]
fn copilot_finish_reason_maps_protocol_values() {
    assert_eq!(map_copilot_finish_reason("end_turn"), FinishReason::Stop);
    assert_eq!(
        map_copilot_finish_reason("max_tokens"),
        FinishReason::Length
    );
    assert_eq!(map_copilot_finish_reason("length"), FinishReason::Length);
    assert_eq!(map_copilot_finish_reason("refusal"), FinishReason::Refusal);
    assert_eq!(
        map_copilot_finish_reason("cancelled"),
        FinishReason::Error("cancelled".to_string())
    );
}

#[test]
fn harness_call_item_id_prefers_tool_call_id() {
    let id = harness_call_item_id("turn-1-step-1", "call_7", "copilot_read");
    assert_eq!(id, "turn-1-step-1-copilot-tool-call_7");
}

#[test]
fn interrupted_builtin_permission_becomes_interactive_denial() {
    let (decision, cache_for_session) = map_builtin_permission_prompt_decision(
        HitlDecision::Interrupt,
        Some("Run cargo check".to_string()),
    );

    assert!(!cache_for_session);
    assert_eq!(
        decision,
        vtcode_core::copilot::CopilotPermissionDecision::DeniedInteractivelyByUser {
            feedback: Some("Run cargo check".to_string()),
        }
    );
}

#[test]
fn interrupted_tool_permission_returns_failure_response() {
    let response = denied_tool_response("unified_exec", "permission request interrupted");

    assert_eq!(
        response,
        CopilotToolCallResponse::Failure(vtcode_core::copilot::CopilotToolCallFailure {
            text_result_for_llm: "VT Code denied the tool `unified_exec`.".to_string(),
            error: "tool 'unified_exec' permission request interrupted".to_string(),
        })
    );
}

#[test]
fn custom_tool_permissions_are_auto_approved_for_session() {
    let approval = auto_approve_builtin_permission(&CopilotPermissionRequest::CustomTool {
        tool_call_id: Some("call-1".to_string()),
        tool_name: "Read last 100 lines of CHANGELOG".to_string(),
        tool_description: "Read the changelog tail".to_string(),
        args: Some(json!({"path": "CHANGELOG.md"})),
    });

    assert_eq!(
        approval,
        Some((
            vtcode_core::copilot::CopilotPermissionDecision::ApprovedAlways,
            true
        ))
    );
}

#[test]
fn shell_permissions_are_not_auto_approved() {
    let approval = auto_approve_builtin_permission(&CopilotPermissionRequest::Shell {
        tool_call_id: Some("call-2".to_string()),
        full_command_text: "cargo check".to_string(),
        intention: "Verify the workspace builds".to_string(),
        commands: vec![],
        possible_paths: vec!["./".to_string()],
        possible_urls: vec![],
        has_write_file_redirection: false,
        can_offer_session_approval: true,
        warning: None,
    });

    assert_eq!(approval, None);
}

#[test]
fn copilot_reasoning_delta_normalizes_chunk_boundaries() {
    assert_eq!(
        normalize_copilot_reasoning_delta(
            "The user wants me to run `cargo check` and report what I see.",
            "Running cargo check".to_string()
        ),
        " Running cargo check"
    );
    assert_eq!(
        normalize_copilot_reasoning_delta("prefix\n", "next".to_string()),
        "next"
    );
}

#[test]
fn copilot_reasoning_delta_collapses_single_newlines_inside_chunk() {
    assert_eq!(
        normalize_copilot_reasoning_delta(
            "Run",
            " cargo fmt and report the\n results\n.\nRunning cargo fmt".to_string()
        ),
        " cargo fmt and report the results. Running cargo fmt"
    );
}

#[tokio::test]
async fn observed_tool_calls_emit_incremental_output_updates() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path().to_path_buf();
    let harness_path = workspace.join("harness.jsonl");

    let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(
        vtcode_core::config::PermissionsConfig::default(),
    ));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state = HarnessTurnState::new(
        TurnRunId("run-test".to_string()),
        TurnId("turn-test".to_string()),
        8,
        60,
        0,
    );
    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        true,
        Some(&emitter),
        "turn-test-step-1".to_string(),
    );

    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_1".to_string(),
        tool_name: "Run cargo check on the workspace".to_string(),
        status: CopilotObservedToolCallStatus::InProgress,
        arguments: Some(json!({"command": "cargo check"})),
        output: Some("Compiling vtcode-core".to_string()),
        terminal_id: None,
    });
    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_1".to_string(),
        tool_name: "Run cargo check on the workspace".to_string(),
        status: CopilotObservedToolCallStatus::Completed,
        arguments: Some(json!({"command": "cargo check"})),
        output: Some("Compiling vtcode-core\nFinished `dev` profile".to_string()),
        terminal_id: None,
    });

    let payload = std::fs::read_to_string(harness_path).expect("read harness log");
    let events: Vec<serde_json::Value> = payload
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse harness event"))
        .collect();

    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.updated"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["output"] == "Compiling vtcode-core"
            && entry["event"]["item"]["status"] == "in_progress"
    }));
    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.completed"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["status"] == "completed"
            && entry["event"]["item"]["output"]
                .as_str()
                .is_some_and(|output| output.contains("Finished `dev` profile"))
    }));
}

#[tokio::test]
async fn observed_shell_tool_calls_stream_into_inline_pty_ui() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path().to_path_buf();

    let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let (command_tx, mut command_rx) = unbounded_channel();
    let (_event_tx, event_rx) = unbounded_channel();
    let mut session = InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
    };
    let handle = session.clone_inline_handle();
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(
        vtcode_core::config::PermissionsConfig::default(),
    ));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state = HarnessTurnState::new(
        TurnRunId("run-test".to_string()),
        TurnId("turn-test".to_string()),
        8,
        60,
        0,
    );

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        true,
        None,
        "turn-test-step-1".to_string(),
    );

    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_shell".to_string(),
        tool_name: "Run cargo check on workspace".to_string(),
        status: CopilotObservedToolCallStatus::InProgress,
        arguments: Some(json!({
            "command": "cd /tmp && cargo check 2>&1",
            "description": "Run cargo check on workspace",
            "mode": "sync",
        })),
        output: Some("Checking vtcode-core\n".to_string()),
        terminal_id: None,
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_shell".to_string(),
        tool_name: "Run cargo check on workspace".to_string(),
        status: CopilotObservedToolCallStatus::Completed,
        arguments: Some(json!({
            "command": "cd /tmp && cargo check 2>&1",
            "description": "Run cargo check on workspace",
            "mode": "sync",
        })),
        output: Some("Checking vtcode-core\nFinished `dev` profile\n".to_string()),
        terminal_id: None,
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let inline_output = collect_inline_output(&mut command_rx);
    assert!(inline_output.contains("• Ran cd /tmp && cargo check 2>&1"));
    assert!(inline_output.contains("Checking vtcode-core"));
    assert!(inline_output.contains("Finished `dev` profile"));
}

#[tokio::test]
async fn copilot_terminal_sessions_bind_local_pty_output_and_release_cleanly() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path().to_path_buf();
    let harness_path = workspace.join("harness-terminal.jsonl");

    let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(
        vtcode_core::config::PermissionsConfig::default(),
    ));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state = HarnessTurnState::new(
        TurnRunId("run-test".to_string()),
        TurnId("turn-test".to_string()),
        8,
        60,
        0,
    );
    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        true,
        Some(&emitter),
        "turn-test-step-1".to_string(),
    );

    let response = runtime_host
        .handle_terminal_create(CopilotTerminalCreateRequest {
            session_id: "session-terminal".to_string(),
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "printf \"$ACP_TEST_TOKEN\"".to_string()],
            env: vec![CopilotTerminalEnvVar {
                name: "ACP_TEST_TOKEN".to_string(),
                value: "vtcode-terminal".to_string(),
            }],
            cwd: None,
            output_byte_limit: Some(1024),
        })
        .await
        .expect("create local terminal");

    let exit_status = runtime_host
        .handle_terminal_wait_for_exit(&response.terminal_id)
        .await
        .expect("wait for local terminal exit");
    assert_eq!(exit_status.exit_code, Some(0));

    let output = runtime_host
        .handle_terminal_output(&response.terminal_id)
        .await
        .expect("read local terminal output");
    assert_eq!(output.output, "vtcode-terminal");
    assert!(!output.truncated);
    assert_eq!(output.exit_status, Some(exit_status.clone()));

    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_terminal".to_string(),
        tool_name: "Run cargo check on the workspace".to_string(),
        status: CopilotObservedToolCallStatus::InProgress,
        arguments: Some(json!({"command": "cargo check"})),
        output: Some("ignored remote output".to_string()),
        terminal_id: Some(response.terminal_id.clone()),
    });

    let payload = std::fs::read_to_string(&harness_path).expect("read harness log");
    let events: Vec<serde_json::Value> = payload
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse harness event"))
        .collect();

    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.started"
            && entry["event"]["item"]["type"] == "tool_invocation"
            && entry["event"]["item"]["tool_name"] == "Run cargo check on the workspace"
    }));
    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.updated"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["output"] == "vtcode-terminal"
    }));
    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.completed"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["status"] == "completed"
            && entry["event"]["item"]["output"] == "vtcode-terminal"
    }));

    runtime_host
        .handle_terminal_release(&response.terminal_id)
        .await
        .expect("release local terminal");
    assert!(
        runtime_host
            .handle_terminal_output(&response.terminal_id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn vtcode_tool_calls_render_transcript_output_via_shared_pipeline() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path().to_path_buf();
    let sample_file = workspace.join("sample.txt");
    std::fs::write(&sample_file, "hello from acp\n").expect("write sample file");

    let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut session_stats = SessionStats::default();
    let mut mcp_panel_state = McpPanelState::default();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(
        vtcode_core::config::PermissionsConfig::default(),
    ));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    safety_validator.start_turn();
    let traj = TrajectoryLogger::new(&workspace);
    let mut harness_state = HarnessTurnState::new(
        TurnRunId("run-test".to_string()),
        TurnId("turn-test".to_string()),
        8,
        60,
        0,
    );
    let tools = Arc::new(vec![ToolDefinition::function(
        "unified_exec".to_string(),
        "Run a VT Code command".to_string(),
        json!({"type": "object"}),
    )]);

    transcript::clear();
    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        Some(&tools),
        true,
        None,
        "turn-test-step-1".to_string(),
    );

    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "call_1".to_string(),
                tool_name: "unified_exec".to_string(),
                arguments: json!({
                    "action": "run",
                    "command": "printf 'hello from acp\\n'"
                }),
            },
        )
        .await
        .expect("copilot VT Code tool call should succeed");

    match response {
        CopilotToolCallResponse::Success(success) => {
            assert!(success.text_result_for_llm.contains("hello from acp"));
        }
        other => panic!("unexpected tool response: {other:?}"),
    }

    let transcript_text = transcript::snapshot().join("\n");
    let stripped_text = vtcode_core::utils::ansi_parser::strip_ansi(&transcript_text);
    assert!(runtime_host.harness_state.tool_calls >= 1);
    assert!(
        stripped_text.contains("hello from acp"),
        "STRIPPED TEXT: {:?}",
        stripped_text
    );
    assert!(
        stripped_text.contains("Ran printf") || stripped_text.contains("Run command"),
        "expected command preview in transcript, got: {stripped_text}"
    );
}
