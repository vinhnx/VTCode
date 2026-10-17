use super::*;
use futures::{FutureExt, poll};
use std::sync::atomic::AtomicUsize;
use vtcode_core::copilot::{CopilotObservedToolCallStatus, CopilotTerminalEnvVar};

fn session(output_byte_limit: Option<usize>) -> LocalTerminalSession {
    LocalTerminalSession {
        exec_session_id: "terminal-test".to_string(),
        released: Arc::new(AtomicBool::new(false)),
        exit_notify: Arc::new(tokio::sync::Notify::new()),
        state: Arc::new(Mutex::new(LocalTerminalSessionState::new(output_byte_limit))),
        task: tokio::spawn(async {}),
    }
}

fn observed(tool_name: &str, arguments: Option<Value>) -> CopilotObservedToolCall {
    CopilotObservedToolCall {
        tool_call_id: "call-original".to_string(),
        tool_name: tool_name.to_string(),
        status: CopilotObservedToolCallStatus::InProgress,
        arguments,
        output: None,
        terminal_id: Some("terminal-test".to_string()),
    }
}

#[test]
fn output_byte_limits_keep_utf8_suffixes_at_both_sides_of_boundaries() {
    struct LimitCase {
        limit: Option<usize>,
        expected: &'static str,
        truncated: bool,
    }
    for case in [
        LimitCase { limit: None, expected: "A界B", truncated: false },
        LimitCase {
            limit: Some(5), expected: "A界B", truncated: false
        },
        LimitCase { limit: Some(4), expected: "界B", truncated: true },
        LimitCase { limit: Some(3), expected: "B", truncated: true },
        LimitCase { limit: Some(1), expected: "B", truncated: true },
        LimitCase { limit: Some(0), expected: "", truncated: true },
    ] {
        let mut state = LocalTerminalSessionState::new(case.limit);
        state.append_output("A界B");
        assert_eq!(state.output, case.expected);
        assert_eq!(state.truncated, case.truncated);
        state.append_output("");
        assert_eq!(state.output, case.expected);
        assert_eq!(state.truncated, case.truncated);
    }
    let mut state = LocalTerminalSessionState::new(Some(4));
    state.append_output("A界");
    assert!(!state.truncated);
    state.append_output("BC");
    assert_eq!(state.output, "BC");
    assert!(state.truncated);
}

#[tokio::test]
async fn late_binding_replays_buffer_and_completion_only_once() {
    let session = session(Some(1024));
    assert!(update_local_terminal_output(&session.state, "before binding").is_none());
    assert!(finalize_local_terminal_exit(&session.state, terminal_exit_status_from_code(7)).is_none());
    let first = session.bind_observed_tool_call(&observed("copilot_tool", None));
    assert!(first.emit_started);
    assert_eq!(first.buffered_output.as_deref(), Some("before binding"));
    assert_eq!(first.finish_status, Some(ToolCallStatus::Failed));
    assert_eq!(first.association.tool_call_id, "call-original");
    let mut second_update = observed("resolved tool", Some(json!({"command": "printf evidence"})));
    second_update.tool_call_id = "call-unrelated".to_string();
    let second = session.bind_observed_tool_call(&second_update);
    assert!(!second.emit_started);
    assert!(second.buffered_output.is_none());
    assert!(second.finish_status.is_none());
    assert_eq!(second.association.tool_call_id, "call-original");
    assert_eq!(second.association.tool_name, "resolved tool");
    assert_eq!(second.association.arguments, json!({"command": "printf evidence"}));
    let third = session.bind_observed_tool_call(&observed("different tool", Some(json!({"command": "wrong"}))));
    assert_eq!(third.association.tool_name, "resolved tool");
    assert_eq!(third.association.arguments, second.association.arguments);
    assert!(finalize_local_terminal_exit(&session.state, terminal_exit_status_from_code(7)).is_none());
    session.release();
}

#[tokio::test]
async fn bound_output_and_completion_preserve_identity_and_accumulated_order() {
    let session = session(None);
    let first = session.bind_observed_tool_call(&observed("shell tool", Some(json!({"command": "printf output"}))));
    assert!(first.emit_started);
    assert!(first.buffered_output.is_none());
    assert!(first.finish_status.is_none());
    let update = update_local_terminal_output(&session.state, "first").unwrap();
    assert_eq!(update.tool_call_id, "call-original");
    assert_eq!(update.tool_name, "shell tool");
    assert_eq!(update.output, "first");
    assert!(update_local_terminal_output(&session.state, "\n").is_none());
    let update = update_local_terminal_output(&session.state, "second").unwrap();
    assert_eq!(update.output, "first\nsecond");
    assert!(finalize_local_terminal_exit(&session.state, None).is_none());
    let completion = finalize_local_terminal_exit(&session.state, terminal_exit_status_from_code(0)).unwrap();
    assert_eq!(completion.tool_call_id, "call-original");
    assert_eq!(completion.tool_name, "shell tool");
    assert_eq!(completion.arguments, json!({"command": "printf output"}));
    assert_eq!(completion.output, "first\nsecond");
    assert_eq!(completion.status, ToolCallStatus::Completed);
    assert!(finalize_local_terminal_exit(&session.state, terminal_exit_status_from_code(0)).is_none());
    let snapshot = session.snapshot_output();
    assert_eq!(snapshot.output, completion.output);
    assert!(!snapshot.truncated);
    assert_eq!(snapshot.exit_status.unwrap().exit_code, Some(0));
    session.release();
}

#[tokio::test]
async fn wait_returns_preexisting_exit_and_pending_exit_notification() {
    let session = session(None);
    {
        let mut wait = Box::pin(session.wait_for_exit());
        assert!(poll!(&mut wait).is_pending());
        assert!(finalize_local_terminal_exit(&session.state, terminal_exit_status_from_code(23)).is_none());
        session.exit_notify.notify_waiters();
        let status = tokio::time::timeout(Duration::from_secs(1), wait).await.unwrap();
        assert_eq!(status.exit_code, Some(23));
    }
    assert_eq!(session.wait_for_exit().now_or_never().unwrap().exit_code, Some(23));
    session.release();
}

struct TaskDrop(Arc<AtomicUsize>);

impl Drop for TaskDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn release_and_abort_wake_waiters_and_cancel_the_monitor_task() {
    for abort in [false, true] {
        let entered = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(tokio::sync::Notify::new());
        let task_entered = Arc::clone(&entered);
        let task_dropped = Arc::clone(&dropped);
        let task_done = Arc::clone(&done);
        struct NotifyOnDrop(Arc<tokio::sync::Notify>);
        impl Drop for NotifyOnDrop {
            fn drop(&mut self) {
                self.0.notify_one();
            }
        }
        let task = tokio::spawn(async move {
            let _done = NotifyOnDrop(task_done);
            let _drop = TaskDrop(task_dropped);
            task_entered.notify_one();
            std::future::pending::<()>().await;
        });
        let reached = tokio::time::timeout(Duration::from_secs(1), entered.notified()).await;
        if reached.is_err() {
            task.abort();
            let _ = task.await;
            panic!("monitor fixture did not start");
        }
        let released = Arc::new(AtomicBool::new(false));
        let exit_notify = Arc::new(tokio::sync::Notify::new());
        let mut notified = Box::pin(exit_notify.notified());
        assert!(poll!(&mut notified).is_pending());
        let session = LocalTerminalSession {
            exec_session_id: "pending-terminal".to_string(),
            released: Arc::clone(&released),
            exit_notify: Arc::clone(&exit_notify),
            state: Arc::new(Mutex::new(LocalTerminalSessionState::new(None))),
            task,
        };
        if abort {
            session.abort();
        } else {
            session.release();
        }
        assert!(released.load(Ordering::Relaxed));
        tokio::time::timeout(Duration::from_secs(1), &mut notified).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), done.notified()).await.unwrap();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn exit_conversion_rejects_out_of_range_codes_and_classifies_failure() {
    assert!(terminal_exit_status_from_code(-1).is_none());
    assert!(terminal_exit_status_from_code(i64::from(u32::MAX) + 1).is_none());
    assert_eq!(terminal_exit_status_from_code(i64::from(u32::MAX)).unwrap().exit_code, Some(u32::MAX));
    assert_eq!(tool_status_from_exit(&terminal_exit_status_from_code(0).unwrap()), ToolCallStatus::Completed);
    assert_eq!(tool_status_from_exit(&terminal_exit_status_from_code(1).unwrap()), ToolCallStatus::Failed);
    assert_eq!(
        tool_status_from_exit(&CopilotTerminalExitStatus { exit_code: None, signal: None }),
        ToolCallStatus::Failed
    );
}

#[test]
fn terminal_arguments_keep_shell_metacharacters_as_separate_argv_values() {
    let request = CopilotTerminalCreateRequest {
        session_id: "terminal-session".to_string(),
        command: "/usr/bin/printf".to_string(),
        args: vec!["%s".to_string(), "literal; $(touch never)".to_string()],
        env: vec![CopilotTerminalEnvVar {
            name: "MARKER".to_string(),
            value: "quoted value".to_string(),
        }],
        cwd: Some(std::path::PathBuf::from("workspace with spaces")),
        output_byte_limit: Some(128),
    };
    let args = terminal_run_args(&request);
    assert_eq!(args["command"], "/usr/bin/printf");
    assert_eq!(args["args"], json!(["%s", "literal; $(touch never)"]));
    assert_eq!(args["cwd"], "workspace with spaces");
    assert_eq!(args["env"], json!([{"name": "MARKER", "value": "quoted value"}]));
    assert_eq!(args["tty"], true);
    assert_eq!(args["yield_time_ms"], 100);
    let display = terminal_command_display(&request.command, &request.args);
    assert_eq!(shell_words::split(&display).unwrap(), ["/usr/bin/printf", "%s", "literal; $(touch never)"]);
}
