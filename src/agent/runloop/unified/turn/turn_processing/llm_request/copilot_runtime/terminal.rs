//! Copilot local terminal state, monitoring, and request lifecycle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anstyle::Color;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use vtcode_core::config::PtyConfig;
use vtcode_core::copilot::{
    CopilotObservedToolCall, CopilotTerminalCreateRequest, CopilotTerminalCreateResponse, CopilotTerminalExitStatus,
    CopilotTerminalOutputResponse,
};
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::style_helpers::ColorPalette;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::progress::{ProgressReporter, ProgressUpdateGuard, spawn_elapsed_time_updater};

use super::presentation::CopilotPtyStream;
use super::{CopilotRuntimeHost, emit_terminal_finished_event, emit_terminal_output_event};

impl CopilotRuntimeHost<'_> {
    pub(super) async fn handle_terminal_create(
        &mut self,
        request: CopilotTerminalCreateRequest,
    ) -> Result<CopilotTerminalCreateResponse> {
        let command_display = terminal_command_display(&request.command, &request.args);
        let response = self
            .tool_registry
            .execute_harness_command_session_terminal_run(terminal_run_args(&request))
            .await
            .context("copilot local terminal create")?;

        let terminal_id = response
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("copilot local terminal create missing session_id"))?;
        let initial_output = response.get("output").and_then(Value::as_str).map(str::to_string);
        let initial_exit_status = response
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(terminal_exit_status_from_code);
        let initial_session_completed = initial_exit_status.is_some();
        let released = Arc::new(AtomicBool::new(false));
        let exit_notify = Arc::new(tokio::sync::Notify::new());
        let state = Arc::new(Mutex::new(LocalTerminalSessionState::new(request.output_byte_limit)));
        {
            let mut session_state = lock_local_terminal_state(&state);
            if let Some(output) = initial_output.as_deref() {
                session_state.append_output(output);
            }
            session_state.exit_status = initial_exit_status.clone();
        }
        if initial_session_completed {
            exit_notify.notify_waiters();
        }

        let task = tokio::spawn(run_local_terminal_session(LocalTerminalTaskContext {
            tool_registry: self.tool_registry.clone(),
            exec_session_id: terminal_id.clone(),
            released: Arc::clone(&released),
            exit_notify: Arc::clone(&exit_notify),
            state: Arc::clone(&state),
            harness_emitter: self.harness_emitter.cloned(),
            harness_item_prefix: self.harness_item_prefix.clone(),
            handle: self.handle.clone(),
            tail_limit: resolve_stdout_tail_limit(self.vt_cfg),
            command_display,
            initial_output,
            pty_config: self.tool_registry.pty_config().clone(),
        }));

        self.local_terminal_sessions.insert(
            terminal_id.clone(),
            LocalTerminalSession {
                exec_session_id: terminal_id.clone(),
                released,
                exit_notify,
                state,
                task,
            },
        );

        Ok(CopilotTerminalCreateResponse { terminal_id })
    }

    pub(super) async fn handle_terminal_output(&self, terminal_id: &str) -> Result<CopilotTerminalOutputResponse> {
        self.local_terminal_sessions
            .get(terminal_id)
            .map(|s| s.snapshot_output())
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))
    }

    pub(super) async fn handle_terminal_release(&mut self, terminal_id: &str) -> Result<()> {
        let Some(session) = self.local_terminal_sessions.remove(terminal_id) else {
            return Ok(());
        };
        let exec_session_id = session.exec_session_id.clone();
        session.release();
        self.tool_registry.close_harness_exec_session(&exec_session_id).await?;
        Ok(())
    }

    pub(super) async fn handle_terminal_kill(&self, terminal_id: &str) -> Result<()> {
        let session = self
            .local_terminal_sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))?;
        self.tool_registry
            .terminate_harness_exec_session(&session.exec_session_id)
            .await
            .with_context(|| format!("copilot terminal kill '{}'", session.exec_session_id))
    }

    pub(super) async fn handle_terminal_wait_for_exit(&self, terminal_id: &str) -> Result<CopilotTerminalExitStatus> {
        let session = self
            .local_terminal_sessions
            .get(terminal_id)
            .ok_or_else(|| anyhow!("copilot terminal '{terminal_id}' not found"))?;
        Ok(session.wait_for_exit().await)
    }
}

#[derive(Clone)]
pub(super) struct LocalTerminalAssociation {
    pub(super) tool_call_id: String,
    pub(super) tool_name: String,
    pub(super) arguments: Value,
}

struct LocalTerminalSessionState {
    output: String,
    truncated: bool,
    output_byte_limit: Option<usize>,
    exit_status: Option<CopilotTerminalExitStatus>,
    association: Option<LocalTerminalAssociation>,
    tool_started: bool,
    tool_finished: bool,
}

impl LocalTerminalSessionState {
    fn new(output_byte_limit: Option<usize>) -> Self {
        Self {
            output: String::new(),
            truncated: false,
            output_byte_limit,
            exit_status: None,
            association: None,
            tool_started: false,
            tool_finished: false,
        }
    }

    fn append_output(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.output.push_str(chunk);
        if let Some(limit) = self.output_byte_limit
            && self.output.len() > limit
        {
            let mut drain_until = self.output.len() - limit;
            while drain_until < self.output.len() && !self.output.is_char_boundary(drain_until) {
                drain_until += 1;
            }
            if drain_until > 0 {
                self.output.drain(..drain_until);
                self.truncated = true;
            }
        }
    }
}

pub(super) struct LocalTerminalSession {
    exec_session_id: String,
    released: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
    state: Arc<Mutex<LocalTerminalSessionState>>,
    task: tokio::task::JoinHandle<()>,
}

pub(super) struct LocalTerminalBindResult {
    pub(super) association: LocalTerminalAssociation,
    pub(super) emit_started: bool,
    pub(super) buffered_output: Option<String>,
    pub(super) finish_status: Option<ToolCallStatus>,
}

struct LocalTerminalTaskContext {
    tool_registry: ToolRegistry,
    exec_session_id: String,
    released: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
    state: Arc<Mutex<LocalTerminalSessionState>>,
    harness_emitter: Option<HarnessEventEmitter>,
    harness_item_prefix: String,
    handle: InlineHandle,
    tail_limit: usize,
    command_display: String,
    initial_output: Option<String>,
    pty_config: PtyConfig,
}

impl LocalTerminalSession {
    pub(super) fn bind_observed_tool_call(&self, update: &CopilotObservedToolCall) -> LocalTerminalBindResult {
        let mut state = lock_local_terminal_state(&self.state);
        let association = if let Some(association) = state.association.as_mut() {
            if association.tool_name == "copilot_tool" && update.tool_name != "copilot_tool" {
                association.tool_name = update.tool_name.clone();
            }
            if association.arguments.is_null()
                && let Some(arguments) = update.arguments.clone()
            {
                association.arguments = arguments;
            }
            association.clone()
        } else {
            let association = LocalTerminalAssociation {
                tool_call_id: update.tool_call_id.clone(),
                tool_name: update.tool_name.clone(),
                arguments: update.arguments.clone().unwrap_or(Value::Null),
            };
            state.association = Some(association.clone());
            association
        };

        let emit_started = if state.tool_started {
            false
        } else {
            state.tool_started = true;
            true
        };
        let buffered_output = emit_started
            .then(|| state.output.clone())
            .filter(|output| !output.trim().is_empty());
        let finish_status = if let Some(exit_status) = state.exit_status.clone() {
            if state.tool_finished {
                None
            } else {
                state.tool_finished = true;
                Some(tool_status_from_exit(&exit_status))
            }
        } else {
            None
        };

        LocalTerminalBindResult {
            association,
            emit_started,
            buffered_output,
            finish_status,
        }
    }

    fn snapshot_output(&self) -> CopilotTerminalOutputResponse {
        let state = lock_local_terminal_state(&self.state);
        CopilotTerminalOutputResponse {
            output: state.output.clone(),
            truncated: state.truncated,
            exit_status: state.exit_status.clone(),
        }
    }

    async fn wait_for_exit(&self) -> CopilotTerminalExitStatus {
        loop {
            if let Some(exit_status) = lock_local_terminal_state(&self.state).exit_status.clone() {
                return exit_status;
            }
            self.exit_notify.notified().await;
        }
    }

    fn release(self) {
        self.released.store(true, Ordering::Relaxed);
        self.exit_notify.notify_waiters();
        self.task.abort();
    }

    pub(super) fn abort(self) {
        self.release();
    }
}

async fn run_local_terminal_session(task: LocalTerminalTaskContext) {
    let LocalTerminalTaskContext {
        tool_registry,
        exec_session_id,
        released,
        exit_notify,
        state,
        harness_emitter,
        harness_item_prefix,
        handle,
        tail_limit,
        command_display,
        initial_output,
        pty_config,
    } = task;

    let TerminalStreamSetup { stream: pty_stream, elapsed_guard: _elapsed_guard } =
        setup_terminal_stream(&handle, tail_limit, &command_display, pty_config).await;
    let mut final_status = None;

    if let Some(output) = initial_output.as_deref() {
        pty_stream.push_output(output);
    }

    loop {
        if released.load(Ordering::Relaxed) {
            break;
        }

        match tool_registry.read_harness_exec_session_output(&exec_session_id, true).await {
            Ok(Some(chunk)) if !chunk.is_empty() => {
                pty_stream.push_output(&chunk);
                if let Some(TerminalOutputUpdate { tool_call_id, tool_name, output }) =
                    update_local_terminal_output(&state, &chunk)
                {
                    emit_terminal_output_event(
                        harness_emitter.as_ref(),
                        &harness_item_prefix,
                        &tool_call_id,
                        &tool_name,
                        &output,
                    );
                }
            }
            Ok(Some(_)) | Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    terminal_id = %exec_session_id,
                    error = %err,
                    "Failed to read Copilot local terminal output"
                );
                break;
            }
        }

        match tool_registry.harness_exec_session_completed(&exec_session_id).await {
            Ok(Some(code)) => {
                let exit_status = terminal_exit_status_from_code(i64::from(code));
                final_status = exit_status.as_ref().map(tool_status_from_exit);
                if let Some(TerminalCompletion { tool_call_id, tool_name, arguments, output, status }) =
                    finalize_local_terminal_exit(&state, exit_status)
                {
                    emit_terminal_finished_event(
                        harness_emitter.as_ref(),
                        &harness_item_prefix,
                        &tool_call_id,
                        &tool_name,
                        &arguments,
                        status,
                        output,
                    );
                }
                exit_notify.notify_waiters();
                break;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    terminal_id = %exec_session_id,
                    error = %err,
                    "Failed to poll Copilot local terminal exit state"
                );
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // An incomplete local terminal session is not a success. Keep the
    // header yellow when cancellation or a monitor error prevents an
    // exit code from being observed.
    let status = final_status.unwrap_or(ToolCallStatus::InProgress);
    pty_stream.finish(tool_call_status_color(&status));
}

struct TerminalStreamSetup {
    stream: CopilotPtyStream,
    elapsed_guard: ProgressUpdateGuard,
}

async fn setup_terminal_stream(
    handle: &InlineHandle,
    tail_limit: usize,
    command_display: &str,
    pty_config: PtyConfig,
) -> TerminalStreamSetup {
    let progress_reporter = ProgressReporter::new();
    progress_reporter.set_total(100).await;
    progress_reporter.set_progress(40).await;
    progress_reporter
        .set_message(format!("Running command: {command_display}"))
        .await;

    let elapsed_guard = ProgressUpdateGuard::new(spawn_elapsed_time_updater(
        progress_reporter.clone(),
        format!("command: {command_display}"),
        500,
    ));

    TerminalStreamSetup {
        stream: CopilotPtyStream::start(handle, progress_reporter, tail_limit, command_display.to_string(), pty_config),
        elapsed_guard,
    }
}

fn terminal_run_args(request: &CopilotTerminalCreateRequest) -> Value {
    json!({
        "action": "run",
        "command": request.command,
        "args": request.args,
        "cwd": request.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "tty": true,
        "yield_time_ms": 100,
        "env": request.env.iter().map(|e| json!({"name": e.name, "value": e.value})).collect::<Vec<_>>(),
    })
}

fn terminal_command_display(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        let parts: Vec<&str> = std::iter::once(command).chain(args.iter().map(|s| s.as_str())).collect();
        shell_words::join(&parts)
    }
}

fn terminal_exit_status_from_code(code: i64) -> Option<CopilotTerminalExitStatus> {
    u32::try_from(code)
        .ok()
        .map(|exit_code| CopilotTerminalExitStatus { exit_code: Some(exit_code), signal: None })
}

fn tool_status_from_exit(exit_status: &CopilotTerminalExitStatus) -> ToolCallStatus {
    match exit_status.exit_code {
        Some(0) => ToolCallStatus::Completed,
        Some(_) | None => ToolCallStatus::Failed,
    }
}

fn tool_call_status_color(status: &ToolCallStatus) -> Color {
    let palette = ColorPalette::default();
    match status {
        ToolCallStatus::Completed => palette.success,
        ToolCallStatus::Failed => palette.error,
        ToolCallStatus::InProgress => palette.warning,
    }
}

struct TerminalOutputUpdate {
    tool_call_id: String,
    tool_name: String,
    output: String,
}

struct TerminalCompletion {
    tool_call_id: String,
    tool_name: String,
    arguments: Value,
    output: String,
    status: ToolCallStatus,
}

fn update_local_terminal_output(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
    chunk: &str,
) -> Option<TerminalOutputUpdate> {
    let mut state = lock_local_terminal_state(state);
    state.append_output(chunk);
    let association = state.association.clone()?;
    if !state.tool_started || chunk.trim().is_empty() {
        return None;
    }
    Some(TerminalOutputUpdate {
        tool_call_id: association.tool_call_id,
        tool_name: association.tool_name,
        output: state.output.clone(),
    })
}

fn finalize_local_terminal_exit(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
    exit_status: Option<CopilotTerminalExitStatus>,
) -> Option<TerminalCompletion> {
    let mut state = lock_local_terminal_state(state);
    state.exit_status = exit_status.clone();
    let association = state.association.clone()?;
    if state.tool_finished {
        return None;
    }
    let exit_status = state.exit_status.clone()?;
    state.tool_finished = true;
    Some(TerminalCompletion {
        tool_call_id: association.tool_call_id,
        tool_name: association.tool_name,
        arguments: association.arguments,
        output: state.output.clone(),
        status: tool_status_from_exit(&exit_status),
    })
}

fn lock_local_terminal_state(
    state: &Arc<Mutex<LocalTerminalSessionState>>,
) -> MutexGuard<'_, LocalTerminalSessionState> {
    state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
