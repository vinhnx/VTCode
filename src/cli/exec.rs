mod prep;

use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::task::{ContextItem, Task, TaskResults};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::exec::events::{
    CommandExecutionStatus, ThreadEvent, ThreadItem, ThreadItemDetails,
};
use vtcode_core::utils::colors::style;
use vtcode_core::utils::file_utils::write_file_with_context;
use vtcode_core::utils::session_archive::SessionMessage;

pub use prep::ExecCommandKind;
use prep::prepare_exec_run;
pub(crate) use prep::resolve_exec_command;

const EXEC_TASK_ID: &str = "exec-task";
const EXEC_TASK_TITLE: &str = "Exec Task";
const EXEC_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive exec mode. Complete the task autonomously using the configured full-auto tool allowlist. Do not request additional user input, confirmations, or allowances—operate solely with the provided information and available tools. Provide a concise summary of the outcome when finished.";
const EXEC_TASK_INSTRUCTIONS_DRY_RUN: &str = "You are running vtcode in non-interactive exec dry-run mode. Plan and validate the approach in read-only mode without mutating files, running mutating commands, or requesting additional user input. If the task requires mutations, explain what would be changed and why.";
const REVIEW_TASK_ID: &str = "review-task";
const REVIEW_TASK_TITLE: &str = "Review Task";
const REVIEW_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive review mode. Review the requested target in read-only mode. Do not modify files, do not run mutating commands, and do not request user input. Return findings first, ordered by severity, with concrete file and line references when possible. If there are no findings, state that explicitly.";

struct ExecEventProcessor<WStdout, WEvents, WStderr> {
    json: bool,
    emit_human_output: bool,
    stdout: Option<WStdout>,
    events_writer: Option<WEvents>,
    stderr: Option<WStderr>,
    last_agent_message: Option<String>,
    last_plan_message: Option<String>,
    active_plan_item_id: Option<String>,
    active_plan_text: String,
    first_error: Option<anyhow::Error>,
}

impl<WStdout, WEvents, WStderr> ExecEventProcessor<WStdout, WEvents, WStderr>
where
    WStdout: Write,
    WEvents: Write,
    WStderr: Write,
{
    fn new(
        json: bool,
        emit_human_output: bool,
        stdout: Option<WStdout>,
        events_writer: Option<WEvents>,
        stderr: Option<WStderr>,
    ) -> Self {
        Self {
            json,
            emit_human_output,
            stdout,
            events_writer,
            stderr,
            last_agent_message: None,
            last_plan_message: None,
            active_plan_item_id: None,
            active_plan_text: String::new(),
            first_error: None,
        }
    }

    fn process_event(&mut self, event: &ThreadEvent) {
        self.track_output_text(event);
        if self.first_error.is_some() {
            return;
        }
        if let Err(err) = self.process_event_impl(event) {
            self.capture_error(err);
        }
    }

    fn process_event_impl(&mut self, event: &ThreadEvent) -> Result<()> {
        let serialized = if self.json || self.events_writer.is_some() {
            Some(serialize_event_line(event)?)
        } else {
            None
        };

        if self.json
            && let Some(line) = serialized.as_deref()
        {
            self.write_stdout(line)?;
        }

        if let Some(writer) = self.events_writer.as_mut()
            && let Some(line) = serialized.as_deref()
        {
            writer
                .write_all(line.as_bytes())
                .context("Failed to write exec event to events file")?;
        }

        if self.emit_human_output
            && let Some(line) = human_event_line(event)
        {
            self.write_stderr_line(&line)?;
        }

        Ok(())
    }

    fn finish_output(&mut self, result: &TaskResults, dry_run: bool) {
        if self.emit_human_output {
            let tail = render_final_tail(result, dry_run);
            if let Err(err) = self.write_stderr(&tail) {
                self.capture_error(err);
            }
        }

        if let Some(writer) = self.stdout.as_mut()
            && let Err(err) = writer.flush().context("Failed to flush exec JSON output")
        {
            self.capture_error(err);
        }

        if let Some(writer) = self.events_writer.as_mut()
            && let Err(err) = writer.flush().context("Failed to flush exec events file")
        {
            self.capture_error(err);
        }

        if let Some(writer) = self.stderr.as_mut()
            && let Err(err) = writer.flush().context("Failed to flush exec human output")
        {
            self.capture_error(err);
        }
    }

    fn final_message(&self) -> Option<&str> {
        self.last_agent_message
            .as_deref()
            .or(self.last_plan_message.as_deref())
    }

    fn warn_empty_last_message(&mut self, path: &Path) {
        if !self.emit_human_output {
            return;
        }
        let warning = format!(
            "Warning: no last agent message; wrote empty content to {}",
            path.display()
        );
        if let Err(err) = self.write_stderr_line(&warning) {
            self.capture_error(err);
        }
    }

    fn take_error(&mut self) -> Result<()> {
        if let Some(err) = self.first_error.take() {
            Err(err)
        } else {
            Ok(())
        }
    }

    fn track_output_text(&mut self, event: &ThreadEvent) {
        match event {
            ThreadEvent::ItemStarted(started) => self.track_item(&started.item),
            ThreadEvent::ItemUpdated(updated) => self.track_item(&updated.item),
            ThreadEvent::ItemCompleted(completed) => self.track_item(&completed.item),
            ThreadEvent::PlanDelta(delta) => self.track_plan_delta(&delta.item_id, &delta.delta),
            _ => {}
        }
    }

    fn track_item(&mut self, item: &ThreadItem) {
        match &item.details {
            ThreadItemDetails::AgentMessage(message) => {
                if !message.text.trim().is_empty() {
                    self.last_agent_message = Some(message.text.clone());
                }
            }
            ThreadItemDetails::Plan(plan) => {
                self.active_plan_item_id = Some(item.id.clone());
                self.active_plan_text = plan.text.clone();
                if !plan.text.trim().is_empty() {
                    self.last_plan_message = Some(plan.text.clone());
                }
            }
            _ => {}
        }
    }

    fn track_plan_delta(&mut self, item_id: &str, delta: &str) {
        if delta.trim().is_empty() {
            return;
        }

        if self.active_plan_item_id.as_deref() != Some(item_id) {
            self.active_plan_item_id = Some(item_id.to_string());
            self.active_plan_text.clear();
        }

        self.active_plan_text.push_str(delta);
        if !self.active_plan_text.trim().is_empty() {
            self.last_plan_message = Some(self.active_plan_text.clone());
        }
    }

    fn write_stdout(&mut self, text: &str) -> Result<()> {
        if let Some(writer) = self.stdout.as_mut() {
            writer
                .write_all(text.as_bytes())
                .context("Failed to write exec JSON output")?;
        }
        Ok(())
    }

    fn write_stderr(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if let Some(writer) = self.stderr.as_mut() {
            writer
                .write_all(text.as_bytes())
                .context("Failed to write exec human output")?;
        }
        Ok(())
    }

    fn write_stderr_line(&mut self, line: &str) -> Result<()> {
        if line.is_empty() {
            return Ok(());
        }

        self.write_stderr(line)?;
        if !line.ends_with('\n') {
            self.write_stderr("\n")?;
        }
        Ok(())
    }

    fn capture_error(&mut self, err: anyhow::Error) {
        if self.first_error.is_none() {
            self.first_error = Some(err);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecCommandOptions {
    pub json: bool,
    pub dry_run: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
    pub command: ExecCommandKind,
}

fn task_instructions(dry_run: bool) -> &'static str {
    if dry_run {
        EXEC_TASK_INSTRUCTIONS_DRY_RUN
    } else {
        EXEC_TASK_INSTRUCTIONS
    }
}

struct TaskSpec {
    id: &'static str,
    title: &'static str,
    instructions: &'static str,
}

fn task_spec(command: &ExecCommandKind, dry_run: bool) -> TaskSpec {
    match command {
        ExecCommandKind::Review { .. } => TaskSpec {
            id: REVIEW_TASK_ID,
            title: REVIEW_TASK_TITLE,
            instructions: REVIEW_TASK_INSTRUCTIONS,
        },
        _ => TaskSpec {
            id: EXEC_TASK_ID,
            title: EXEC_TASK_TITLE,
            instructions: task_instructions(dry_run),
        },
    }
}

fn serialize_event_line(event: &ThreadEvent) -> Result<String> {
    let mut line =
        serde_json::to_string(event).context("Failed to serialize exec event to JSON")?;
    line.push('\n');
    Ok(line)
}

fn human_event_line(event: &ThreadEvent) -> Option<String> {
    match event {
        ThreadEvent::ItemStarted(started) => match &started.item.details {
            ThreadItemDetails::CommandExecution(details) => Some(format!(
                "{} {}",
                style("[COMMAND]").cyan().bold(),
                details.command
            )),
            _ => None,
        },
        ThreadEvent::ItemCompleted(completed) => match &completed.item.details {
            ThreadItemDetails::CommandExecution(details)
                if matches!(details.status, CommandExecutionStatus::Failed) =>
            {
                let exit_suffix = details
                    .exit_code
                    .map(|code| format!(" (exit {code})"))
                    .unwrap_or_default();
                Some(format!(
                    "{} {}{}",
                    style("[COMMAND FAILED]").red().bold(),
                    details.command,
                    exit_suffix
                ))
            }
            ThreadItemDetails::Error(item) => Some(format!(
                "{} {}",
                style("[WARNING]").red().bold(),
                item.message
            )),
            _ => None,
        },
        ThreadEvent::TurnFailed(failed) => Some(format!(
            "{} {}",
            style("[ERROR]").red().bold(),
            failed.message
        )),
        ThreadEvent::Error(error) => Some(format!(
            "{} {}",
            style("[ERROR]").red().bold(),
            error.message
        )),
        _ => None,
    }
}

fn render_final_tail(result: &TaskResults, dry_run: bool) -> String {
    let mut output = String::new();
    output.push('\n');

    if !result.summary.trim().is_empty() {
        let _ = writeln!(
            output,
            "{} {}\n",
            style("[SUMMARY]").green().bold(),
            result.summary.trim()
        );
    }

    let avg_display = result
        .average_turn_duration_ms
        .map(|avg| format!("{avg:.1}"))
        .unwrap_or_else(|| "-".to_string());
    let max_display = result
        .max_turn_duration_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());

    let _ = writeln!(output, "{}", style("[OUTCOME]").magenta().bold());
    let _ = writeln!(output, "  {:16} {}", "outcome", result.outcome);
    let _ = writeln!(output, "  {:16} {}", "dry_run", dry_run);
    let _ = writeln!(output, "  {:16} {}", "turns", result.turns_executed);
    let _ = writeln!(
        output,
        "  {:16} {}",
        "duration_ms", result.total_duration_ms
    );
    let _ = writeln!(output, "  {:16} {}", "avg_turn_ms", avg_display);
    let _ = writeln!(output, "  {:16} {}", "max_turn_ms", max_display);
    let _ = writeln!(output, "  {:16} {}\n", "warnings", result.warnings.len());

    if !result.modified_files.is_empty() {
        let _ = writeln!(output, "{}", style("[FILES]").cyan().bold());
        for (idx, file) in result.modified_files.iter().enumerate() {
            let _ = writeln!(output, "  {:>2}. {}", idx + 1, file);
        }
        output.push('\n');
    }

    if !result.executed_commands.is_empty() {
        let _ = writeln!(output, "{}", style("[COMMANDS]").cyan().bold());
        for (idx, cmd) in result.executed_commands.iter().enumerate() {
            let _ = writeln!(output, "  {:>2}. {}", idx + 1, cmd);
        }
        output.push('\n');
    }

    if !result.warnings.is_empty() {
        let _ = writeln!(output, "{}", style("[WARNINGS]").red().bold());
        for (idx, warning) in result.warnings.iter().enumerate() {
            let _ = writeln!(output, "  {:>2}. {}", idx + 1, warning);
        }
        output.push('\n');
    }

    output
}

fn open_events_writer(path: &Path) -> Result<BufWriter<File>> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let file = File::create(path)
        .with_context(|| format!("Failed to write exec events: {}", path.display()))?;
    Ok(BufWriter::new(file))
}

fn lock_or_recover<T>(mutex: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn exec_archive_transcript(messages: &[vtcode_core::llm::provider::Message]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|message| {
            let text = message.content.as_text();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(format!("{}: {}", message.role, trimmed.replace('\n', " ")))
            }
        })
        .collect()
}

pub async fn handle_exec_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    tokio::select! {
        res = handle_exec_command_impl(config, vt_cfg, options) => res,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("{}", style("\nCancelled by user.").red());
            bail!("Operation cancelled");
        }
    }
}

async fn handle_exec_command_impl(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
) -> Result<()> {
    let prepared = prepare_exec_run(config, vt_cfg, &options).await?;
    let prep::ExecPreparedRun {
        config: run_config,
        vt_cfg: run_vt_cfg,
        model_id,
        prompt,
        session_id,
        archive,
        thread_bootstrap,
    } = prepared;
    let task_spec = task_spec(&options.command, options.dry_run);

    let automation_cfg = &run_vt_cfg.automation.full_auto;

    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        model_id,
        run_config.api_key.clone(),
        run_config.workspace.clone(),
        session_id,
        RunnerSettings {
            reasoning_effort: Some(run_config.reasoning_effort),
            verbosity: None,
        },
        None,
        thread_bootstrap,
        run_vt_cfg.clone(),
    )
    .await?;

    let allowed_tools = match &options.command {
        ExecCommandKind::Review { .. } => {
            runner
                .review_tool_allowlist(&automation_cfg.allowed_tools)
                .await
        }
        _ => automation_cfg.allowed_tools.clone(),
    };
    runner.enable_full_auto(&allowed_tools).await;
    if options.dry_run {
        runner.enable_plan_mode();
    }
    runner.set_quiet(true);

    let processor = Arc::new(Mutex::new(ExecEventProcessor::<
        io::Stdout,
        BufWriter<File>,
        io::Stderr,
    >::new(
        options.json,
        !options.json && !run_config.quiet,
        options.json.then(io::stdout),
        options
            .events_path
            .as_ref()
            .map(|path| open_events_writer(path.as_path()))
            .transpose()?,
        (!options.json && !run_config.quiet).then(io::stderr),
    )));
    let event_processor = Arc::clone(&processor);
    runner.set_event_handler(move |event| {
        let mut processor = lock_or_recover(&event_processor);
        processor.process_event(event);
    });

    // OPTIMIZATION: Avoid unnecessary allocations for static strings
    let task = Task {
        id: task_spec.id.into(),
        title: task_spec.title.into(),
        description: prompt.trim().to_string(),
        instructions: Some(task_spec.instructions.into()),
    };

    let max_retries = run_vt_cfg.agent.max_task_retries;
    let result = runner
        .execute_task_with_retry(&task, &[] as &[ContextItem], max_retries)
        .await
        .context("Failed to execute autonomous task after retries")?;

    let last_message = {
        let mut processor = lock_or_recover(&processor);
        let message = processor.final_message().unwrap_or_default().to_string();
        processor.finish_output(&result, options.dry_run);
        message
    };

    if let Some(path) = &options.last_message_file {
        let message = last_message.as_str();
        write_file_with_context(path, message, "last message file").await?;
        if message.is_empty() {
            let mut processor = lock_or_recover(&processor);
            processor.warn_empty_last_message(path);
        }
    }

    let session_messages = runner.session_messages();
    let session_archive_messages: Vec<SessionMessage> =
        session_messages.iter().map(SessionMessage::from).collect();
    archive
        .finalize(
            exec_archive_transcript(&session_messages),
            session_archive_messages.len(),
            result.executed_commands.clone(),
            session_archive_messages,
        )
        .context("Failed to save exec session archive")?;

    let mut processor = lock_or_recover(&processor);
    processor
        .take_error()
        .context("Failed to process exec event output")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ExecCommandKind, ExecEventProcessor, REVIEW_TASK_ID, human_event_line, render_final_tail,
        serialize_event_line, task_instructions, task_spec,
    };
    use vtcode_core::core::agent::task::{TaskOutcome, TaskResults};
    use vtcode_core::exec::events::{
        AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ErrorItem,
        ItemCompletedEvent, ItemStartedEvent, PlanDeltaEvent, ThreadErrorEvent, ThreadEvent,
        ThreadItem, ThreadItemDetails, ThreadStartedEvent, TurnCompletedEvent, Usage,
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
