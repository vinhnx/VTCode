use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use vtcode_core::core::agent::task::TaskResults;
use vtcode_core::exec::events::{
    CommandExecutionStatus, ThreadEvent, ThreadItem, ThreadItemDetails,
};
use vtcode_core::utils::colors::style;

pub(super) struct ExecEventProcessor<WStdout, WEvents, WStderr> {
    json: bool,
    emit_human_output: bool,
    pub(super) stdout: Option<WStdout>,
    events_writer: Option<WEvents>,
    pub(super) stderr: Option<WStderr>,
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
    pub(super) fn new(
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

    pub(super) fn process_event(&mut self, event: &ThreadEvent) {
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

    pub(super) fn finish_output(&mut self, result: &TaskResults, dry_run: bool) {
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

    pub(super) fn final_message(&self) -> Option<&str> {
        self.last_agent_message
            .as_deref()
            .or(self.last_plan_message.as_deref())
    }

    pub(super) fn warn_empty_last_message(&mut self, path: &Path) {
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

    pub(super) fn take_error(&mut self) -> Result<()> {
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

pub(super) fn serialize_event_line(event: &ThreadEvent) -> Result<String> {
    let mut line =
        serde_json::to_string(event).context("Failed to serialize exec event to JSON")?;
    line.push('\n');
    Ok(line)
}

pub(super) fn human_event_line(event: &ThreadEvent) -> Option<String> {
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
            ThreadItemDetails::Harness(item) => {
                let label = match item.event {
                    vtcode_core::exec::events::HarnessEventKind::ContinuationStarted => {
                        style("[HARNESS]").cyan().bold()
                    }
                    vtcode_core::exec::events::HarnessEventKind::ContinuationSkipped => {
                        style("[HARNESS]").cyan().bold()
                    }
                    vtcode_core::exec::events::HarnessEventKind::VerificationStarted => {
                        style("[VERIFY]").cyan().bold()
                    }
                    vtcode_core::exec::events::HarnessEventKind::VerificationPassed => {
                        style("[VERIFY]").green().bold()
                    }
                    vtcode_core::exec::events::HarnessEventKind::VerificationFailed => {
                        style("[VERIFY FAILED]").red().bold()
                    }
                };
                let detail = item
                    .message
                    .as_deref()
                    .or(item.command.as_deref())
                    .unwrap_or("harness event");
                Some(format!("{} {}", label, detail))
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

pub(super) fn render_final_tail(result: &TaskResults, dry_run: bool) -> String {
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

pub(super) fn open_events_writer(path: &Path) -> Result<BufWriter<File>> {
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

pub(super) fn lock_or_recover<T>(mutex: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) fn exec_archive_transcript(
    messages: &[vtcode_core::llm::provider::Message],
) -> Vec<String> {
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
