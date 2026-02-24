use super::execution::{execute_tool_with_timeout, process_llm_tool_output};
use super::timeout::create_timeout_error;
use super::*;
use crate::agent::runloop::unified::state::CtrlCState;

use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::registry::ToolTimeoutCategory;
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineHandle, InlineSession, spawn_session, theme_from_styles};
use vtcode_core::utils::ansi::AnsiRenderer;

/// Helper function to create test registry with common setup
async fn create_test_registry(workspace: &std::path::Path) -> ToolRegistry {
    ToolRegistry::new(workspace.to_path_buf()).await
}

/// Helper function to create test renderer with default config
fn create_test_renderer(
    handle: &vtcode_core::ui::tui::InlineHandle,
) -> vtcode_core::utils::ansi::AnsiRenderer {
    AnsiRenderer::with_inline_ui(handle.clone(), Default::default())
}

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
    }
}

fn build_harness_state() -> crate::agent::runloop::unified::run_loop_context::HarnessTurnState {
    build_harness_state_with(4)
}

fn build_harness_state_with(
    max_tool_calls: usize,
) -> crate::agent::runloop::unified::run_loop_context::HarnessTurnState {
    crate::agent::runloop::unified::run_loop_context::HarnessTurnState::new(
        crate::agent::runloop::unified::run_loop_context::TurnRunId("test-run".to_string()),
        crate::agent::runloop::unified::run_loop_context::TurnId("test-turn".to_string()),
        max_tool_calls,
        60,
        0,
    )
}

/// Helper function to create common test context components
struct TestContext {
    registry: ToolRegistry,
    renderer: vtcode_core::utils::ansi::AnsiRenderer,
    session: vtcode_core::ui::tui::InlineSession,
    handle: vtcode_core::ui::tui::InlineHandle,
    approval_recorder: vtcode_core::tools::ApprovalRecorder,
    workspace: std::path::PathBuf,
}

impl TestContext {
    async fn new() -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();

        let registry = create_test_registry(&workspace).await;
        let active_styles = theme::active_styles();
        let theme_spec = theme_from_styles(&active_styles);
        let mut session = match spawn_session(
            theme_spec,
            None,
            vtcode_core::config::types::UiSurfacePreference::default(),
            10,
            None,
            None,
        ) {
            Ok(session) => session,
            Err(err) if err.to_string().contains("stdin is not a terminal") => {
                create_headless_session()
            }
            Err(err) => panic!("failed to spawn test session: {err:#}"),
        };
        // Skip confirmations for tests to ensure non-interactive success
        session.set_skip_confirmations(true);
        let handle = session.clone_inline_handle();
        let renderer = create_test_renderer(&handle);
        let approval_recorder = vtcode_core::tools::ApprovalRecorder::new(workspace.clone());

        Self {
            registry,
            renderer,
            session,
            handle,
            approval_recorder,
            workspace,
        }
    }
}

mod run_tool_call;

#[tokio::test]
async fn test_execute_tool_with_timeout() {
    // Setup test dependencies
    let mut registry = ToolRegistry::new(std::env::current_dir().unwrap()).await;
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    // Test a simple tool execution with unknown tool
    let result = execute_tool_with_timeout(
        &mut registry,
        "test_tool",
        json!({}),
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        0,
    )
    .await;

    // Verify the result - unknown tool should return error or failure
    match result {
        ToolExecutionStatus::Failure { .. } => {
            // Expected for unknown tool
        }
        ToolExecutionStatus::Success { ref output, .. } => {
            // Tool returns success with error in output for unknown tools
            if output.get("error").is_some() {
                // This is acceptable - tool returned an error object
            } else {
                panic!("Expected tool to return error object for unknown tool");
            }
        }
        other => panic!("Unexpected result type: {:?}", other),
    }
}

#[tokio::test]
async fn test_ask_questions_alias_resolves_to_request_user_input() {
    let tmp = tempfile::TempDir::new().unwrap();
    let registry = ToolRegistry::new(tmp.path().to_path_buf()).await;
    let tool = registry
        .get_tool(tools::ASK_QUESTIONS)
        .expect("ask_questions alias should resolve");
    assert_eq!(tool.name(), tools::REQUEST_USER_INPUT);
}

#[tokio::test]
async fn test_ask_user_question_alias_resolves_to_request_user_input() {
    let tmp = tempfile::TempDir::new().unwrap();
    let registry = ToolRegistry::new(tmp.path().to_path_buf()).await;
    let tool = registry
        .get_tool(tools::ASK_USER_QUESTION)
        .expect("ask_user_question alias should resolve");
    assert_eq!(tool.name(), tools::REQUEST_USER_INPUT);
}

#[test]
fn test_process_tool_output() {
    // Test successful output
    let output = json!({
        "exit_code": 0,
        "stdout": "test output",
        "modified_files": ["file1.txt", "file2.txt"],
        "has_more": false
    });

    let status = process_llm_tool_output(output);
    if let ToolExecutionStatus::Success {
        output: _,
        stdout,
        modified_files,
        command_success,
        has_more,
    } = status
    {
        assert_eq!(stdout, Some("test output".to_string()));
        assert_eq!(modified_files, vec!["file1.txt", "file2.txt"]);
        assert!(command_success);
        assert!(!has_more);
    } else {
        panic!("Expected Success variant");
    }
}

#[test]
fn test_process_tool_output_loop_detection() {
    // Test loop detection output - should return Failure with clear message
    let output = json!({
        "error": {
            "tool_name": "read_file",
            "error_type": "PolicyViolation",
            "message": "Tool 'read_file' blocked after 5 identical invocations in recent history (limit: 5)",
            "is_recoverable": false,
            "recovery_suggestions": [],
            "original_error": null
        },
        "loop_detected": true,
        "repeat_count": 5,
        "tool": "read_file"
    });

    let status = process_llm_tool_output(output);
    if let ToolExecutionStatus::Failure { error } = status {
        let error_msg = error.to_string();
        assert!(error_msg.contains("LOOP DETECTION"));
        assert!(error_msg.contains("read_file"));
        assert!(error_msg.contains("5"));
        assert!(error_msg.contains("DO NOT retry"));
        assert!(error_msg.contains("ACTION REQUIRED"));
    } else {
        panic!(
            "Expected Failure variant for loop detection, got: {:?}",
            status
        );
    }
}

/*
#[tokio::test]
async fn test_run_tool_call_read_file_success() {
...
}
*/

#[test]
fn test_create_timeout_error() {
    let status = create_timeout_error(
        "test_tool",
        ToolTimeoutCategory::Default,
        Some(Duration::from_secs(42)),
    );
    if let ToolExecutionStatus::Timeout { error } = status {
        assert!(error.message.contains("test_tool"));
        assert!(error.message.contains("timeout ceiling"));
        assert!(error.message.contains("42"));
    } else {
        panic!("Expected Timeout variant");
    }
}

// Note: This test requires tokio's test-util feature (start_paused, advance)
// which is not enabled in the standard build. The test is commented out
// to avoid compilation errors. To run it, enable tokio/test-util in Cargo.toml.
//
// #[tokio::test(start_paused = true)]
// async fn emits_warning_before_timeout_ceiling() {
//     let warnings = Arc::new(Mutex::new(Vec::new()));
//     let writer_buffer = warnings.clone();
//
//     let subscriber = fmt()
//         .with_writer(move || CaptureWriter::new(writer_buffer.clone()))
//         .with_max_level(Level::WARN)
//         .without_time()
//         .finish();
//
//     let _guard = tracing::subscriber::set_default(subscriber);
//
//     let temp_dir = TempDir::new().expect(\"create temp dir\");
//     let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
//     registry
//         .register_tool(ToolRegistration::new(
//             \"__test_slow_tool__\",
//             CapabilityLevel::Basic,
//             false,
//             slow_tool_executor,
//         ))
//         .expect(\"register slow tool\");
//
//     let ctrl_c_state = Arc::new(CtrlCState::new());
//     let ctrl_c_notify = Arc::new(Notify::new());
//
//     let mut registry_task = registry;
//     let ctrl_c_state_clone = ctrl_c_state.clone();
//     let ctrl_c_notify_clone = ctrl_c_notify.clone();
//
//     let execution = tokio::spawn(async move {
//         execute_tool_with_timeout(
//             &mut registry_task,
//             \"__test_slow_tool__\",
//             Value::Null,
//             &ctrl_c_state_clone,
//             &ctrl_c_notify_clone,
//             None,
//         )
//         .await
//     });
//
//     let default_timeout = Duration::from_secs(300);
//     let warning_delay = default_timeout
//         .checked_sub(TOOL_TIMEOUT_WARNING_HEADROOM)
//         .expect(\"warning delay\");
//     advance(warning_delay).await;
//     yield_now().await;
//
//     let captured = warnings.lock().unwrap();
//     let combined = captured.join(\"\");
//     assert!(
//         combined.contains(\"has run\"),
//         \"expected warning log to include 'has run', captured logs: {}\",
//         combined
//     );
//     drop(captured);
//
//     advance(TOOL_TIMEOUT_WARNING_HEADROOM + Duration::from_secs(1)).await;
//     let status = execution.await.expect(\"join execution\");
//     assert!(matches!(status, ToolExecutionStatus::Timeout { .. }));
// }
