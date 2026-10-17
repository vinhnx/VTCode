use super::{
    CopilotRuntimeHost, append_session_limit_grant_guidance, auto_approve_builtin_permission, bounded_output_evidence,
    copilot_failure_response_with_diagnosis, copilot_tool_result_text, denied_tool_response, filter_copilot_tools,
    harness_call_item_id, map_builtin_permission_prompt_decision, summarize_permission_request,
};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use vtcode_core::config::CommandsConfig;
use vtcode_core::copilot::{
    CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionRequest, CopilotTerminalCreateRequest,
    CopilotTerminalEnvVar, CopilotToolCallResponse,
};
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::transcript;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_routing::HitlDecision;
use crate::agent::runloop::unified::turn::tool_outcomes::ToolFailureDiagnosis;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{LoopTracker, mutation_blocked_until_verification};
use tempfile::TempDir;
use tokio::sync::{Notify, RwLock, mpsc::unbounded_channel};
use vtcode_core::acp::ToolPermissionCache;

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = unbounded_channel();
    let (_event_tx, event_rx) = unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
        worker: None,
    }
}

fn collect_inline_output(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>) -> String {
    let mut lines: Vec<String> = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::AppendLine { segments, .. } => {
                lines.push(segments.into_iter().map(|segment| segment.text).collect::<String>());
            }
            InlineCommand::ReplaceLast { lines: replacement_lines, .. } => {
                for line in replacement_lines {
                    lines.push(line.into_iter().map(|segment| segment.text).collect::<String>());
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
        ToolDefinition::function("code_search".to_string(), "Search".to_string(), json!({"type": "object"})),
        ToolDefinition::function("apply_patch".to_string(), "Patch".to_string(), json!({"type": "object"})),
    ]);

    let filtered = filter_copilot_tools(Some(&tools), &["code_search".to_string()]);
    let names: Vec<_> = filtered
        .iter()
        .filter_map(|tool| tool.function.as_ref().map(|function| function.name.as_str()))
        .collect();

    assert_eq!(names, vec!["code_search"]);
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
fn harness_call_item_id_prefers_tool_call_id() {
    let id = harness_call_item_id("turn-1-step-1", "call_7", "copilot_read");
    assert_eq!(id, "turn-1-step-1-copilot-tool-call_7");
}

#[test]
fn interrupted_builtin_permission_becomes_interactive_denial() {
    let (decision, cache_for_session) =
        map_builtin_permission_prompt_decision(HitlDecision::Interrupt, Some("Run cargo check".to_string()));

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
    let response = denied_tool_response("exec_command", "permission request interrupted");

    assert_eq!(
        response,
        CopilotToolCallResponse::Failure(vtcode_core::copilot::CopilotToolCallFailure {
            text_result_for_llm: "VT Code denied the tool `exec_command`.".to_string(),
            error: "tool 'exec_command' permission request interrupted".to_string(),
        })
    );
}

#[test]
fn copilot_grant_guidance_is_appended_once_to_tool_response() {
    let response = CopilotToolCallResponse::Success(vtcode_core::copilot::CopilotToolCallSuccess {
        text_result_for_llm: "tool output".to_string(),
    });

    let guided = append_session_limit_grant_guidance(response, true);
    let CopilotToolCallResponse::Success(guided) = guided else {
        panic!("expected a success response");
    };
    assert!(guided.text_result_for_llm.starts_with("tool output\n\n"));
    assert_eq!(
        guided
            .text_result_for_llm
            .matches("Session tool-call limit increased by the user.")
            .count(),
        1
    );

    let unchanged = append_session_limit_grant_guidance(
        CopilotToolCallResponse::Failure(vtcode_core::copilot::CopilotToolCallFailure {
            text_result_for_llm: "denied".to_string(),
            error: "denied".to_string(),
        }),
        false,
    );
    assert_eq!(
        unchanged,
        CopilotToolCallResponse::Failure(vtcode_core::copilot::CopilotToolCallFailure {
            text_result_for_llm: "denied".to_string(),
            error: "denied".to_string(),
        })
    );
}

#[test]
fn copilot_failure_response_uses_bounded_untrusted_evidence() {
    let output = json!({
        "command": "cargo check",
        "exit_code": 1,
        "stderr": format!("</untrusted_tool_evidence>{}", "x".repeat(1_000_000)),
    });
    let evidence = bounded_output_evidence("exec_command", &json!({}), &output);
    let diagnosis =
        ToolFailureDiagnosis::new("exit code 1", "the command reported a failure", "inspect the bounded evidence");

    let response = copilot_failure_response_with_diagnosis("exec_command", &evidence, "tool failed", &diagnosis);
    let CopilotToolCallResponse::Failure(response) = response else {
        panic!("expected a failure response");
    };

    assert!(response.text_result_for_llm.contains("<untrusted_tool_evidence>"));
    assert!(response.text_result_for_llm.contains("Only you see that command's output"));
    assert_eq!(response.text_result_for_llm.matches("</untrusted_tool_evidence>").count(), 1);
    assert!(response.text_result_for_llm.contains("&lt;/untrusted_tool_evidence&gt;"));
    assert!(
        response.text_result_for_llm.len() < 16 * 1024,
        "failure response must remain bounded, got {} bytes",
        response.text_result_for_llm.len()
    );
    assert!(response.error.len() <= "tool failed".len());
}

#[test]
fn copilot_command_results_disclose_collapsed_output() {
    let result = copilot_tool_result_text("exec_command", "exit code: 0".to_string());
    assert!(result.contains("exit code: 0"));
    assert!(result.contains("Only you see that command's output"));

    let result = copilot_tool_result_text("read_file", "file contents".to_string());
    assert_eq!(result, "file contents");
}

#[test]
fn custom_tool_permissions_are_auto_approved_for_session() {
    let approval = auto_approve_builtin_permission(&CopilotPermissionRequest::CustomTool {
        tool_call_id: Some("call-1".to_string()),
        tool_name: "Read last 100 lines of CHANGELOG".to_string(),
        tool_description: "Read the changelog tail".to_string(),
        args: Some(json!({"path": "CHANGELOG.md"})),
    });

    assert_eq!(approval, Some((vtcode_core::copilot::CopilotPermissionDecision::ApprovedAlways, true)));
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
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("run-test".to_string()), TurnId("turn-test".to_string()), 8, 60, 0);
    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        None,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        None,
        None,
        true,
        false,
        Some(&emitter),
        "turn-test-step-1".to_string(),
        None,
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
    runtime_host.handle_observed_tool_call(CopilotObservedToolCall {
        tool_call_id: "call_2".to_string(),
        tool_name: "Read the missing file".to_string(),
        status: CopilotObservedToolCallStatus::Failed,
        arguments: Some(json!({"path": "missing.txt"})),
        output: Some("file not found".to_string()),
        terminal_id: None,
    });

    let payload = std::fs::read_to_string(harness_path).expect("read harness log");
    let events: Vec<serde_json::Value> = payload
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .expect("harness event lines should be valid JSON");

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
    drop(runtime_host);
    assert_eq!(harness_state.snapshot_turn_diagnostics(Default::default(), 0).failed_tool_calls, 1);
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
        worker: None,
    };
    let handle = session.clone_inline_handle();
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("run-test".to_string()), TurnId("turn-test".to_string()), 8, 60, 0);

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        None,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        None,
        None,
        true,
        false,
        None,
        "turn-test-step-1".to_string(),
        None,
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
    let mut commands_config = CommandsConfig::default();
    commands_config.allow_list.push("/usr/bin/printenv".to_string());
    tool_registry.apply_commands_config(&commands_config);
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let traj = TrajectoryLogger::new(&workspace);
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel_state = McpPanelState::default();
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("run-test".to_string()), TurnId("turn-test".to_string()), 8, 60, 0);
    let emitter = HarnessEventEmitter::new(harness_path.clone()).expect("harness emitter");

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        None,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        None,
        None,
        true,
        false,
        Some(&emitter),
        "turn-test-step-1".to_string(),
        None,
    );
    let response = runtime_host
        .handle_terminal_create(CopilotTerminalCreateRequest {
            session_id: "session-terminal".to_string(),
            command: "/usr/bin/printenv".to_string(),
            args: vec!["ACP_TEST_TOKEN".to_string()],
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
    assert_eq!(output.output.trim(), "vtcode-terminal");
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
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .expect("harness event lines should be valid JSON");

    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.started"
            && entry["event"]["item"]["type"] == "tool_invocation"
            && entry["event"]["item"]["tool_name"] == "Run cargo check on the workspace"
    }));
    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.updated"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["output"]
                .as_str()
                .is_some_and(|output| output.trim() == "vtcode-terminal")
    }));
    assert!(events.iter().any(|entry| {
        entry["event"]["type"] == "item.completed"
            && entry["event"]["item"]["type"] == "tool_output"
            && entry["event"]["item"]["status"] == "completed"
            && entry["event"]["item"]["output"]
                .as_str()
                .is_some_and(|output| output.trim() == "vtcode-terminal")
    }));

    runtime_host
        .handle_terminal_release(&response.terminal_id)
        .await
        .expect("release local terminal");
    runtime_host.handle_terminal_output(&response.terminal_id).await.unwrap_err();
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
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel_state = McpPanelState::default();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    safety_validator.start_turn();
    let traj = TrajectoryLogger::new(&workspace);
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("run-test".to_string()), TurnId("turn-test".to_string()), 8, 60, 0);
    let tools = Arc::new(vec![ToolDefinition::function(
        "exec_command".to_string(),
        "Run a VT Code command".to_string(),
        json!({"type": "object"}),
    )]);

    transcript::clear();
    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        None,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        None,
        None,
        Some(&tools),
        true,
        false,
        None,
        "turn-test-step-1".to_string(),
        None,
    );
    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "call_1".to_string(),
                tool_name: "exec_command".to_string(),
                arguments: json!({
                    "action": "run",
                    "command": "cat sample.txt"
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
        !stripped_text.contains("hello from acp"),
        "successful command output should be compacted in the live transcript: {stripped_text:?}"
    );
    assert!(
        stripped_text.contains("Ran cat") || stripped_text.contains("Run command"),
        "expected command preview in transcript, got: {stripped_text}"
    );
}

#[tokio::test]
async fn copilot_blocks_mutating_tool_calls_while_verification_is_pending() {
    let temp = TempDir::new().expect("temp workspace");
    let workspace = temp.path().to_path_buf();
    let target = workspace.join("blocked.rs");
    let blocked_target = workspace.join("blocked-after-checkpoint.rs");
    std::fs::write(workspace.join("invalid.rs"), "pub fn value() -> u8 { \"bad\" }\n").unwrap();
    std::fs::write(workspace.join("context.txt"), "retained evidence\n").unwrap();

    let mut tool_registry = ToolRegistry::new(workspace).await;
    let mut commands = tool_registry.commands_config();
    commands.allow_list.push("sed".to_owned());
    tool_registry.apply_commands_config(&commands);
    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel_state = McpPanelState::default();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let approval_recorder = ApprovalRecorder::new(temp.path().to_path_buf());
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));
    let safety_validator = Arc::new(ToolCallSafetyValidator::new());
    safety_validator.start_turn();
    let traj = TrajectoryLogger::new(temp.path());
    let mut harness_state =
        HarnessTurnState::new(TurnRunId("run-test".to_string()), TurnId("turn-test".to_string()), 10, 60, 0);
    harness_state.record_assistant_text_response();
    harness_state.record_assistant_text_response();
    let tools = Arc::new(vec![
        ToolDefinition::function(
            vtcode_core::config::constants::tools::APPLY_PATCH.to_string(),
            "Apply a patch".to_string(),
            json!({"type": "object"}),
        ),
        ToolDefinition::function(
            vtcode_core::config::constants::tools::EXEC_COMMAND.to_string(),
            "Run a command".to_string(),
            json!({"type": "object"}),
        ),
    ]);
    let mut loop_tracker = LoopTracker::new();
    loop_tracker.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD - 1;
    assert!(!mutation_blocked_until_verification(
        &loop_tracker,
        vtcode_core::config::constants::tools::APPLY_PATCH,
        &json!({"patch": "*** Begin Patch\n*** End Patch\n"}),
    ));
    let suppress_output_signal = Arc::new(AtomicBool::new(false));
    let suppress_output_signal_for_assertion = suppress_output_signal.clone();

    let mut runtime_host = CopilotRuntimeHost::new(
        &mut tool_registry,
        &tool_result_cache,
        &mut session,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel_state,
        &handle,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        &approval_recorder,
        &decision_ledger,
        &tool_permission_cache,
        &permissions_state,
        None,
        &safety_validator,
        None,
        None,
        &traj,
        &mut harness_state,
        Some(&mut loop_tracker),
        Some(suppress_output_signal),
        Some(&tools),
        true,
        false,
        None,
        "turn-test-step-1".to_string(),
        None,
    );
    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "mutation-call".to_string(),
                tool_name: vtcode_core::config::constants::tools::APPLY_PATCH.to_string(),
                arguments: json!({
                    "patch": "*** Begin Patch\n*** Add File: blocked.rs\n+mutation must trigger verification\n*** End Patch\n"
                }),
            },
        )
        .await
        .expect("Copilot mutation should return a tool response");

    match response {
        CopilotToolCallResponse::Success(_) => {}
        other => panic!("unexpected mutation response: {other:?}"),
    }
    assert!(target.exists(), "the first mutation should reach the tool pipeline");
    assert!(
        runtime_host
            .loop_tracker
            .as_deref()
            .is_some_and(|tracker| tracker.verification_is_pending())
    );
    assert_eq!(runtime_host.harness_state.consecutive_assistant_text_responses, 0);
    assert!(runtime_host.harness_state.has_out_of_band_tool_progress());
    assert!(suppress_output_signal_for_assertion.load(Ordering::Acquire));

    runtime_host.harness_state.record_assistant_text_response();
    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "blocked-call".to_string(),
                tool_name: vtcode_core::config::constants::tools::APPLY_PATCH.to_string(),
                arguments: json!({
                    "patch": "*** Begin Patch\n*** Add File: blocked-after-checkpoint.rs\n+must stay absent\n*** End Patch\n"
                }),
            },
        )
        .await
        .expect("blocked Copilot tool call should return a denial response");

    match response {
        CopilotToolCallResponse::Failure(failure) => {
            assert!(failure.error.contains("verification"), "unexpected denial: {}", failure.error);
        }
        other => panic!("unexpected Copilot response: {other:?}"),
    }
    assert!(!blocked_target.exists(), "blocked mutation must not reach the tool pipeline");
    assert_eq!(
        runtime_host.harness_state.consecutive_assistant_text_responses, 1,
        "a denied inline mutation is not a productive response-streak boundary"
    );

    assert!(runtime_host.session_stats.verification_snapshot().0);
    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "failed-verification-call".to_string(),
                tool_name: vtcode_core::config::constants::tools::EXEC_COMMAND.to_string(),
                arguments: json!({"cmd":"rustc --crate-type lib invalid.rs -o invalid.rlib"}),
            },
        )
        .await
        .unwrap();
    let CopilotToolCallResponse::Failure(failure) = response else {
        panic!("invalid source must fail verification");
    };
    assert!(
        failure
            .text_result_for_llm
            .contains("The verifier reported a terminal non-zero exit")
    );
    assert!(failure.text_result_for_llm.contains("revise the fix hypothesis"));
    assert!(!temp.path().join("invalid.rlib").exists());
    assert!(runtime_host.session_stats.verification_snapshot().0);
    assert!(!runtime_host.loop_tracker.as_deref_mut().unwrap().take_verifier_failed_notice());

    for (index, command) in [
        "sed -n '1,2p' context.txt",
        "sed -n '1,1p' context.txt",
        "sed -n '1,3p' context.txt",
    ]
    .into_iter()
    .enumerate()
    {
        let response = runtime_host
            .handle_vtcode_tool_call(
                &mut renderer,
                vtcode_core::copilot::CopilotToolCallRequest {
                    tool_call_id: format!("retained-evidence-{index}"),
                    tool_name: vtcode_core::config::constants::tools::EXEC_COMMAND.to_string(),
                    arguments: json!({"cmd":command}),
                },
            )
            .await
            .unwrap();
        let CopilotToolCallResponse::Success(success) = response else {
            panic!("inspection must succeed");
        };
        assert_eq!(
            success
                .text_result_for_llm
                .contains("revise the hypothesis and change scope or approach"),
            index == 1,
        );
        assert!(
            !success
                .text_result_for_llm
                .contains("The verifier reported a terminal non-zero exit")
        );
    }

    let response = runtime_host
        .handle_vtcode_tool_call(
            &mut renderer,
            vtcode_core::copilot::CopilotToolCallRequest {
                tool_call_id: "verification-call".to_string(),
                tool_name: vtcode_core::config::constants::tools::EXEC_COMMAND.to_string(),
                arguments: json!({
                    "action": "run",
                    "command": "rustc --version"
                }),
            },
        )
        .await
        .expect("successful Copilot verifier should return a tool response");

    match response {
        CopilotToolCallResponse::Success(_) => {}
        other => panic!("unexpected verification response: {other:?}"),
    }
    assert!(
        runtime_host
            .loop_tracker
            .as_deref()
            .is_some_and(|tracker| !tracker.verification_is_pending())
    );
    assert!(!runtime_host.session_stats.verification_snapshot().0);
    assert_eq!(
        runtime_host.harness_state.consecutive_assistant_text_responses, 0,
        "successful inline verification must remain visible in authoritative turn state"
    );

    let resumed_tracker = LoopTracker::with_verification_snapshot(runtime_host.session_stats.verification_snapshot());
    assert!(!mutation_blocked_until_verification(
        &resumed_tracker,
        vtcode_core::config::constants::tools::APPLY_PATCH,
        &json!({
            "patch": "*** Begin Patch\n*** Add File: resumed.txt\n+mutation is unblocked\n*** End Patch\n"
        }),
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use vtcode_core::config::constants::tools;
        let script = temp.path().join("verify.sh");
        let marker = temp.path().join("custom-check-ran");
        runtime_host.tool_registry.allow_all_tools().await.unwrap();
        let mut commands = runtime_host.tool_registry.commands_config();
        commands.allow_list.push("./verify.sh".into());
        runtime_host.tool_registry.apply_commands_config(&commands);
        let tracker = runtime_host
            .tool_registry
            .execute_tool(
                tools::TASK_TRACKER,
                json!({
                    "action":"create", "title":"Native custom check", "items":[{
                        "description":"Verify", "status":"completed", "verify":["./verify.sh"]
                    }]
                }),
            )
            .await
            .unwrap();
        assert!(tracker.get("error").is_none(), "{tracker}");
        runtime_host.tool_registry.restore_completion_verification_debt();
        runtime_host.loop_tracker.as_deref_mut().unwrap().mark_verification_pending();
        for exit_code in [7, 0] {
            std::fs::write(&script, format!("#!/bin/sh\ntouch custom-check-ran\nexit {exit_code}\n")).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            let response = runtime_host
                .handle_vtcode_tool_call(
                    &mut renderer,
                    vtcode_core::copilot::CopilotToolCallRequest {
                        tool_call_id: format!("custom-verifier-{exit_code}"),
                        tool_name: tools::EXEC_COMMAND.into(),
                        arguments: json!({"cmd":"./verify.sh"}),
                    },
                )
                .await
                .unwrap();
            assert!(marker.exists(), "{response:?}");
            std::fs::remove_file(&marker).unwrap();
            assert_eq!(matches!(response, CopilotToolCallResponse::Success(_)), exit_code == 0);
            let tracker = runtime_host.loop_tracker.as_deref().unwrap();
            assert_eq!(tracker.verification_is_pending(), exit_code != 0);
            assert_eq!(
                tracker.fix_edits_remaining,
                if exit_code == 0 {
                    0
                } else {
                    crate::agent::runloop::unified::turn::tool_outcomes::helpers::FAILED_VERIFICATION_FIX_ALLOWANCE
                }
            );
            assert_eq!(runtime_host.tool_registry.completion_verification_pending(), exit_code != 0);
        }
    }
}
