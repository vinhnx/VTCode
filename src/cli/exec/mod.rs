mod event_output;
mod prep;
mod run;

use anyhow::{Result, bail};
use std::path::PathBuf;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

pub use prep::ExecCommandKind;
pub(crate) use prep::resolve_exec_command;

#[derive(Debug, Clone)]
pub struct ExecCommandOptions {
    pub json: bool,
    pub dry_run: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
    pub command: ExecCommandKind,
}

pub async fn handle_exec_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    tokio::select! {
        res = run::handle_exec_command_impl(config, vt_cfg, options) => res,
        _ = vtcode_core::shutdown::shutdown_signal() => {
            eprintln!("{}", vtcode_core::utils::colors::style("\nCancelled by user.").red());
            bail!("Operation cancelled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExecCommandKind;
    use super::event_output::{
        ExecEventProcessor, human_event_line, render_final_tail, serialize_event_line,
    };
    use super::run::{REVIEW_TASK_ID, resolve_exec_event_log_path, task_instructions, task_spec};
    use tempfile::TempDir;
    use vtcode_core::core::agent::task::{TaskOutcome, TaskResults};
    use vtcode_core::exec::events::{
        AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ErrorItem,
        HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ItemStartedEvent, PlanDeltaEvent,
        ThreadErrorEvent, ThreadEvent, ThreadItem, ThreadItemDetails, ThreadStartedEvent,
        TurnCompletedEvent, Usage,
    };

    type TestProcessor = ExecEventProcessor<Vec<u8>, Vec<u8>, Vec<u8>>;

    #[test]
    fn dry_run_instructions_are_read_only_focused() {
        let instructions = task_instructions(true);
        assert!(instructions.contains("read-only"));
        assert!(instructions.contains("without mutating files"));
    }

    #[test]
    fn normal_exec_instructions_do_not_use_dry_run_wording() {
        let instructions = task_instructions(false);
        assert!(!instructions.contains("dry-run mode"));
    }

    #[test]
    fn review_commands_use_review_instructions() {
        let spec = vtcode_core::review::build_review_spec(false, None, Vec::new(), None)
            .expect("review spec");
        let task = task_spec(&ExecCommandKind::Review { spec }, false);

        assert_eq!(task.id, REVIEW_TASK_ID);
        assert!(task.instructions.contains("read-only mode"));
    }

    #[test]
    fn json_mode_serializes_raw_event_lines() {
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: "thread-1".to_string(),
        });
        let mut processor = TestProcessor::new(true, false, Some(Vec::new()), None, None);

        processor.process_event(&event);

        let output = String::from_utf8(processor.stdout.take().expect("stdout buffer"))
            .expect("stdout should be utf8");
        assert_eq!(
            output,
            serialize_event_line(&event).expect("event should serialize")
        );
        assert!(processor.stderr.is_none());
    }

    #[test]
    fn tracked_last_message_prefers_agent_over_plan() {
        let mut processor = TestProcessor::new(false, false, None, None, None);
        processor.process_event(&ThreadEvent::PlanDelta(PlanDeltaEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "plan-1".to_string(),
            delta: "First plan".to_string(),
        }));
        assert_eq!(processor.final_message(), Some("First plan"));

        processor.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "msg-1".to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: "Final summary".to_string(),
                }),
            },
        }));
        assert_eq!(processor.final_message(), Some("Final summary"));
    }

    #[test]
    fn human_mode_uses_stderr_and_preserves_tail_sections() {
        let mut processor =
            TestProcessor::new(false, true, Some(Vec::new()), None, Some(Vec::new()));
        processor.process_event(&ThreadEvent::ItemStarted(ItemStartedEvent {
            item: ThreadItem {
                id: "cmd-1".to_string(),
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "git status".to_string(),
                    arguments: None,
                    aggregated_output: String::new(),
                    exit_code: None,
                    status: CommandExecutionStatus::InProgress,
                })),
            },
        }));
        processor.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "warn-1".to_string(),
                details: ThreadItemDetails::Error(ErrorItem {
                    message: "watch out".to_string(),
                }),
            },
        }));

        let result = TaskResults {
            created_contexts: Vec::new(),
            modified_files: vec!["src/main.rs".to_string()],
            executed_commands: vec!["git status".to_string()],
            summary: "done".to_string(),
            warnings: vec!["watch out".to_string()],
            thread_events: Vec::new(),
            outcome: TaskOutcome::Success,
            turns_executed: 1,
            total_duration_ms: 123,
            average_turn_duration_ms: Some(123.0),
            max_turn_duration_ms: Some(123),
            turn_durations_ms: vec![123],
        };
        processor.finish_output(&result, false);
        processor.take_error().expect("processor should succeed");

        let stdout = String::from_utf8(processor.stdout.take().expect("stdout buffer"))
            .expect("stdout should be utf8");
        let stderr = String::from_utf8(processor.stderr.take().expect("stderr buffer"))
            .expect("stderr should be utf8");

        assert!(stdout.is_empty());
        assert!(stderr.contains("[COMMAND]"));
        assert!(stderr.contains("[WARNING]"));
        assert!(stderr.contains("[SUMMARY]"));
        assert!(stderr.contains("[OUTCOME]"));
        assert!(stderr.contains("[FILES]"));
        assert!(stderr.contains("[COMMANDS]"));
        assert!(stderr.contains("[WARNINGS]"));
    }

    #[test]
    fn human_event_line_formats_failures() {
        let line = human_event_line(&ThreadEvent::Error(ThreadErrorEvent {
            message: "boom".to_string(),
        }))
        .expect("error event should render");
        assert!(line.contains("[ERROR]"));
        assert!(line.contains("boom"));
    }

    #[test]
    fn human_event_line_formats_harness_verification_events() {
        let line = human_event_line(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "verify-1".to_string(),
                details: ThreadItemDetails::Harness(HarnessEventItem {
                    event: HarnessEventKind::VerificationFailed,
                    message: Some("cargo check failed".to_string()),
                    command: Some("cargo check".to_string()),
                    exit_code: Some(101),
                }),
            },
        }))
        .expect("harness event should render");

        assert!(line.contains("[VERIFY FAILED]"));
        assert!(line.contains("cargo check failed"));
    }

    #[test]
    fn resolve_exec_event_log_path_appends_jsonl_when_given_directory() {
        let temp = TempDir::new().expect("tempdir");
        let resolved =
            resolve_exec_event_log_path(temp.path().to_str().expect("tempdir path"), "session-123");

        assert_eq!(resolved.parent(), Some(temp.path()));
        let file_name = resolved
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name");
        assert!(file_name.starts_with("harness-session-123-"));
        assert!(file_name.ends_with(".jsonl"));
    }

    #[test]
    fn final_tail_includes_summary_and_metrics() {
        let result = TaskResults {
            created_contexts: Vec::new(),
            modified_files: Vec::new(),
            executed_commands: Vec::new(),
            summary: "Completed work".to_string(),
            warnings: Vec::new(),
            thread_events: vec![ThreadEvent::TurnCompleted(TurnCompletedEvent {
                usage: Usage::default(),
            })],
            outcome: TaskOutcome::Success,
            turns_executed: 2,
            total_duration_ms: 250,
            average_turn_duration_ms: Some(125.0),
            max_turn_duration_ms: Some(200),
            turn_durations_ms: vec![50, 200],
        };

        let rendered = render_final_tail(&result, true);
        assert!(rendered.contains("[SUMMARY]"));
        assert!(rendered.contains("Completed work"));
        assert!(rendered.contains("dry_run"));
        assert!(rendered.contains("true"));
    }
}
