//! Single-workspace companion execution through the shipped exec entrypoint.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vtcode_core::exec::events::{
    HarnessEventKind, PatchApplyStatus, ThreadCompletionSubtype, ThreadEvent, ThreadItemDetails,
};
use vtcode_webmcp::companion::{CompanionTasks, StartTaskInput, TaskMode, TaskSnapshot, TaskState, TaskVerification};

const MAX_PROMPT_BYTES: usize = 16 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_TASKS: usize = 64;
const MAX_CHANGES: usize = 128;
const TASK_TIMEOUT: Duration = Duration::from_secs(1800);

mod command_evidence;
#[cfg(unix)]
mod process_owner;

struct TaskRecord {
    snapshot: TaskSnapshot,
    cancellation: CancellationToken,
    worker: Option<tokio::task::JoinHandle<()>>,
}

/// Preserve operator-selected launch policy through the existing CLI/config loader.
/// Resolve the config file at server startup, before a child changes directory.
pub(super) fn launch_args(cli: &vtcode_core::cli::args::Cli, config_path: Option<PathBuf>) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(path) = config_path {
        args.extend([OsString::from("--config"), path.into_os_string()]);
    }
    for entry in &cli.config {
        if entry.contains('=') {
            args.extend([OsString::from("--config"), OsString::from(entry)]);
        }
    }
    for entry in &cli.allowed_tools {
        args.extend([OsString::from("--allowed-tools"), OsString::from(entry)]);
    }
    for entry in &cli.disallowed_tools {
        args.extend([OsString::from("--disallowed-tools"), OsString::from(entry)]);
    }
    for (flag, value) in [
        ("--provider", cli.provider.as_ref()),
        ("--model", cli.model.as_ref()),
        ("--agent", cli.agent.as_ref()),
    ] {
        if let Some(value) = value {
            args.extend([OsString::from(flag), OsString::from(value)]);
        }
    }
    for (flag, value) in [
        ("--security-level", cli.security_level.clone()),
        ("--api-key-env", cli.api_key_env.clone()),
        ("--max-tool-calls", cli.max_tool_calls.to_string()),
        ("--api-rate-limit", cli.api_rate_limit.to_string()),
        ("--max-concurrent-ops", cli.max_concurrent_ops.to_string()),
    ] {
        args.extend([OsString::from(flag), OsString::from(value)]);
    }
    for (enabled, flag) in [
        (cli.chrome, "--chrome"),
        (cli.no_chrome, "--no-chrome"),
        (cli.codex_experimental, "--codex-experimental"),
        (cli.no_codex_experimental, "--no-codex-experimental"),
    ] {
        if enabled {
            args.push(flag.into());
        }
    }
    args
}

/// Execution belongs to the binary; transport clients cannot configure the launch.
pub(super) struct CompanionService {
    workspace: PathBuf,
    executable: PathBuf,
    proxy_token_env: String,
    launch_args: Vec<OsString>,
    tasks: Arc<Mutex<BTreeMap<String, TaskRecord>>>,
    closing: AtomicBool,
}

impl CompanionService {
    pub(super) fn new(
        workspace: PathBuf,
        executable: PathBuf,
        proxy_token_env: String,
        launch_args: Vec<OsString>,
    ) -> Self {
        Self {
            workspace,
            executable,
            proxy_token_env,
            launch_args,
            tasks: Arc::new(Mutex::new(BTreeMap::new())),
            closing: AtomicBool::new(false),
        }
    }

    /// Shut down before the listener returns so owned children are reaped.
    pub(super) async fn shutdown(&self) -> Result<(), String> {
        self.closing.store(true, Ordering::Release);
        let workers: Vec<_> = {
            let mut tasks = self.tasks.lock().await;
            tasks
                .iter_mut()
                .filter_map(|(id, task)| {
                    task.cancellation.cancel();
                    task.worker.take().map(|worker| (id.clone(), worker))
                })
                .collect()
        };
        let mut failed = false;
        for (id, mut worker) in workers {
            match tokio::time::timeout(Duration::from_secs(8), &mut worker).await {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => {}
                Err(_) => {
                    worker.abort();
                    let _ = worker.await;
                }
            }
            if let Some(task) = self.tasks.lock().await.get_mut(&id) {
                task.snapshot.state = TaskState::Failed;
                task.snapshot.mark_incomplete();
                task.snapshot.progress = "task supervisor failed during shutdown".into();
            }
            failed = true;
        }
        if failed {
            Err("companion shutdown could not confirm clean task completion".into())
        } else {
            Ok(())
        }
    }

    fn command(&self, input: &StartTaskInput, owner_socket: Option<&Path>) -> Command {
        let mut command = Command::new(&self.executable);
        #[cfg(unix)]
        if let Some(socket) = owner_socket {
            // The native helper replaces itself with this same exec entrypoint
            // only after its lifetime monitor is ready. Keep launch argv exact.
            command.args([
                std::ffi::OsStr::new(vtcode_bash_runner::process_owner::HELPER_MODE),
                self.executable.as_os_str(),
                self.executable.as_os_str(),
            ]);
            command.env(vtcode_bash_runner::process_owner::OWNER_SOCKET_ENV, socket);
        }
        #[cfg(not(unix))]
        let _ = owner_socket;
        command
            .current_dir(&self.workspace)
            .args(&self.launch_args)
            .arg("--workspace")
            .arg(&self.workspace)
            .arg("exec")
            .arg("--json");
        if input.mode == TaskMode::Plan {
            command.arg("--dry-run");
        }
        // Prompts use the existing stdin interface, never command-line parsing.
        command
            .env_remove(&self.proxy_token_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // A persisted trust record still works; no environment or dotenv value can grant trust.
        command.env("VTCODE_TRUST_WORKSPACE", "deny");
        #[cfg(unix)]
        command.process_group(0);
        command
    }
}

impl CompanionService {
    async fn start_with_timeout(&self, input: StartTaskInput, timeout: Duration) -> Result<TaskSnapshot, String> {
        if input.prompt.trim().is_empty() || input.prompt.len() > MAX_PROMPT_BYTES || input.prompt.contains('\0') {
            return Err("prompt must contain 1 to 16384 UTF-8 bytes and no NUL".into());
        }
        let mut tasks = self.tasks.lock().await;
        if self.closing.load(Ordering::Acquire) {
            return Err("companion service is shutting down".into());
        }
        if tasks.values().any(|task| task.snapshot.state == TaskState::Running) {
            return Err("a companion task is already running".into());
        }
        if tasks.len() >= MAX_TASKS {
            return Err("companion task retention limit reached; restart the service".into());
        }
        let prompt = match input.mode {
            TaskMode::Plan => {
                if input.plan_task_id.is_some() {
                    return Err("planning does not accept plan_task_id".into());
                }
                input.prompt.clone()
            }
            TaskMode::Implement => {
                let plan = input
                    .plan_task_id
                    .as_ref()
                    .and_then(|id| tasks.get(id))
                    .filter(|task| {
                        task.snapshot.mode == TaskMode::Plan
                            && task.snapshot.state == TaskState::Completed
                            && !task.snapshot.evidence_truncated
                    })
                    .and_then(|task| task.snapshot.final_response.as_deref())
                    .filter(|text| !text.trim().is_empty())
                    .ok_or("implementation requires a successfully completed planning task from this service")?;
                format!(
                    "Implement the following previously completed plan, subject to current workspace evidence and execution policy.\n\n{plan}\n\nUser implementation request:\n{}",
                    input.prompt
                )
            }
        };
        #[cfg(unix)]
        let owner = process_owner::TaskProcessOwner::new()
            .map_err(|error| format!("failed to initialize task process ownership: {error}"))?;
        #[cfg(unix)]
        let owner_socket = Some(owner.socket());
        #[cfg(not(unix))]
        let owner_socket = None;
        let mut child = self
            .command(&input, owner_socket)
            .spawn()
            .map_err(|error| format!("failed to launch VT Code exec: {error}"))?;
        let mut process_group = ProcessGroup {
            pid: child.id(),
            #[cfg(unix)]
            owner,
        };
        let mut stdin = child.stdin.take().ok_or("exec stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("exec stdout unavailable")?;
        let mut stderr = child.stderr.take().ok_or("exec stderr unavailable")?;
        let task_id = Uuid::new_v4().to_string();
        let cancellation = CancellationToken::new();
        let snapshot = TaskSnapshot {
            task_id: task_id.clone(),
            mode: input.mode,
            state: TaskState::Running,
            session_id: None,
            final_response: None,
            changes: Vec::new(),
            unified_diff: String::new(),
            evidence_truncated: false,
            command_results: Vec::new(),
            verification: TaskVerification::Unverified,
            verification_command: None,
            verification_exit_code: None,
            progress: "exec started".into(),
        };
        tasks.insert(
            task_id.clone(),
            TaskRecord {
                snapshot: snapshot.clone(),
                cancellation: cancellation.clone(),
                worker: None,
            },
        );
        let records = Arc::clone(&self.tasks);
        let worker = tokio::spawn(async move {
            let mut projection = EventProjection::default();
            let run = async {
                let events = collect_events(BufReader::new(stdout), &records, &task_id, &mut projection);
                let mut sink = tokio::io::sink();
                let drain = tokio::io::copy(&mut stderr, &mut sink);
                let write_prompt = async {
                    stdin.write_all(prompt.as_bytes()).await?;
                    stdin.shutdown().await?;
                    drop(stdin);
                    Ok::<(), std::io::Error>(())
                };
                let output = async { tokio::try_join!(events, drain, write_prompt).map(|_| ()) };
                tokio::pin!(output);
                tokio::select! {
                    result = &mut output => {
                        result?;
                        child.wait().await
                    }
                    status = child.wait() => {
                        let status = status?;
                        tokio::time::timeout(Duration::from_secs(2), &mut output).await
                            .map_err(|error| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("exec output remained open after process exit: {error}")))??;
                        Ok(status)
                    }
                }
            };
            let outcome = tokio::select! {
                biased;
                _ = cancellation.cancelled() => None,
                _ = tokio::time::sleep(timeout) => Some(Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "task deadline exceeded"))),
                result = run => Some(result),
            };
            // Also terminate descendants retaining pipes after their parent exits.
            if outcome.is_none() {
                if let Some(pid) = process_group.pid {
                    let _ = vtcode_bash_runner::process_group::kill_process_group_with_signal(
                        pid,
                        vtcode_bash_runner::KillSignal::Int,
                    );
                }
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            }
            let cleanup_ok = process_group.terminate().is_ok();
            let reap_ok = match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(Ok(_)) => true,
                _ => {
                    let _ = child.start_kill();
                    matches!(tokio::time::timeout(Duration::from_secs(2), child.wait()).await, Ok(Ok(_)))
                }
            };
            let mut tasks = records.lock().await;
            if let Some(task) = tasks.get_mut(&task_id) {
                let state = if !cleanup_ok || !reap_ok {
                    TaskState::Failed
                } else {
                    match &outcome {
                        None => TaskState::Cancelled,
                        Some(Ok(status))
                            if status.success()
                                && projection.completion == Some(ThreadCompletionSubtype::Success)
                                && !projection.failed =>
                        {
                            TaskState::Completed
                        }
                        Some(Ok(_)) if projection.completion == Some(ThreadCompletionSubtype::Cancelled) => {
                            TaskState::Cancelled
                        }
                        _ => TaskState::Failed,
                    }
                };
                task.snapshot.state = state;
                task.snapshot.progress = match state {
                    TaskState::Completed => "exec and canonical thread completed".into(),
                    TaskState::Cancelled => "task cancelled; child reaped".into(),
                    _ => public_text(
                        &projection.failure_progress(outcome.as_ref(), cleanup_ok, reap_ok),
                        2048,
                        &mut task.snapshot.evidence_truncated,
                    ),
                };
                if state != TaskState::Completed {
                    task.snapshot.mark_incomplete();
                }
            }
        });
        if let Some(task) = tasks.get_mut(&snapshot.task_id) {
            task.worker = Some(worker);
        }
        Ok(snapshot)
    }
}

#[async_trait]
impl CompanionTasks for CompanionService {
    async fn start(&self, input: StartTaskInput) -> Result<TaskSnapshot, String> {
        self.start_with_timeout(input, TASK_TIMEOUT).await
    }

    async fn get(&self, task_id: &str) -> Result<TaskSnapshot, String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks.get_mut(task_id).ok_or("unknown companion task")?;
        if task.snapshot.state == TaskState::Running && task.worker.as_ref().is_some_and(|worker| worker.is_finished())
        {
            task.snapshot.state = TaskState::Failed;
            task.snapshot.mark_incomplete();
            task.snapshot.progress = "task supervisor stopped without a terminal result".into();
        }
        Ok(task.snapshot.clone())
    }

    async fn cancel(&self, task_id: &str) -> Result<bool, String> {
        let tasks = self.tasks.lock().await;
        let task = tasks.get(task_id).ok_or("unknown companion task")?;
        if task.snapshot.state != TaskState::Running {
            return Ok(false);
        }
        task.cancellation.cancel();
        Ok(true)
    }
}

// Drops also clean up if the supervisor future is aborted during runtime teardown.
struct ProcessGroup {
    pid: Option<u32>,
    #[cfg(unix)]
    owner: process_owner::TaskProcessOwner,
}

impl ProcessGroup {
    fn terminate(&mut self) -> std::io::Result<()> {
        let group_result = match self.pid.take() {
            Some(pid) => vtcode_bash_runner::process_group::kill_process_group(pid),
            None => Ok(()),
        };
        #[cfg(unix)]
        {
            // Keep the root watchdog pinning its group ID until after the
            // numeric group signal. The same ordering covers explicit cleanup
            // and supervisor-future drop, without relying on capture order.
            let owner_result = self.owner.shutdown();
            match (group_result, owner_result) {
                (Err(group), Err(owner)) => Err(std::io::Error::other(format!(
                    "group cleanup failed: {group}; task owner cleanup failed: {owner}"
                ))),
                (Err(error), _) | (_, Err(error)) => Err(error),
                (Ok(()), Ok(())) => Ok(()),
            }
        }
        #[cfg(not(unix))]
        group_result
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Err(error) = self.terminate() {
            tracing::warn!(%error, "companion process group cleanup failed");
        }
    }
}

#[derive(Default)]
struct EventProjection {
    commands: command_evidence::CommandEvidence,
    completion: Option<ThreadCompletionSubtype>,
    failed: bool,
    failure_reason: Option<String>,
}

impl EventProjection {
    fn record_failure(&mut self, snapshot: &mut TaskSnapshot, message: &str) {
        self.failed = true;
        if self.failure_reason.is_none() && !message.trim().is_empty() {
            self.failure_reason = Some(public_text(message, 1024, &mut snapshot.evidence_truncated));
        }
    }

    fn failure_progress(
        &self,
        outcome: Option<&std::io::Result<std::process::ExitStatus>>,
        cleanup_ok: bool,
        reap_ok: bool,
    ) -> String {
        let mut reasons = Vec::new();
        if let Some(reason) = &self.failure_reason {
            reasons.push(format!("runtime failure: {reason}"));
        }
        match outcome {
            Some(Err(error)) => reasons.push(format!("exec supervision failed: {error}")),
            Some(Ok(status)) if !status.success() => reasons.push(format!("exec exited with {status}")),
            _ => {}
        }
        if self.completion.is_none() {
            reasons.push("missing canonical thread completion".into());
        } else if self.completion != Some(ThreadCompletionSubtype::Success) && reasons.is_empty() {
            reasons.push("canonical thread did not complete successfully".into());
        } else if self.failed && reasons.is_empty() {
            reasons.push("runtime reported a failure".into());
        }
        if !cleanup_ok {
            reasons.push("owned process cleanup failed".into());
        }
        if !reap_ok {
            reasons.push("child reap could not be confirmed".into());
        }
        reasons.join("; ")
    }

    fn observe(&mut self, snapshot: &mut TaskSnapshot, event: ThreadEvent) {
        self.commands.observe(snapshot, &event);
        snapshot.progress = match &event {
            ThreadEvent::ThreadStarted(_) => "runtime session started",
            ThreadEvent::TurnStarted(_) => "agent turn running",
            ThreadEvent::ItemStarted(_) => "processing runtime activity",
            ThreadEvent::ItemCompleted(_) => "received runtime evidence",
            ThreadEvent::ThreadCompleted(_) => "thread ended; awaiting process cleanup",
            ThreadEvent::TurnFailed(_) | ThreadEvent::TurnBlocked(_) | ThreadEvent::Error(_) => {
                "runtime reported a failure"
            }
            _ => "runtime event received",
        }
        .into();
        match event {
            ThreadEvent::ThreadStarted(event) => {
                snapshot.session_id = Some(public_text(&event.thread_id, 256, &mut snapshot.evidence_truncated))
            }
            ThreadEvent::ThreadCompleted(event) => {
                self.completion = Some(event.subtype);
                if event.subtype != ThreadCompletionSubtype::Success {
                    self.record_failure(snapshot, event.stop_reason.as_deref().unwrap_or(&event.outcome_code));
                }
                snapshot.session_id = Some(public_text(&event.session_id, 256, &mut snapshot.evidence_truncated));
                if let Some(result) = event.result {
                    snapshot.final_response =
                        Some(public_text(&result, MAX_TEXT_BYTES, &mut snapshot.evidence_truncated));
                }
            }
            ThreadEvent::Error(event) => self.record_failure(snapshot, &event.message),
            ThreadEvent::TurnFailed(event) => self.record_failure(snapshot, &event.message),
            ThreadEvent::TurnBlocked(event) => self.record_failure(snapshot, &event.message),
            ThreadEvent::ItemStarted(event) => {
                if matches!(
                    event.item.details,
                    ThreadItemDetails::FileChange(_)
                        | ThreadItemDetails::CommandExecution(_)
                        | ThreadItemDetails::ToolInvocation(_)
                        | ThreadItemDetails::McpToolCall(_)
                ) {
                    snapshot.invalidate_verification();
                }
            }
            ThreadEvent::ItemCompleted(event) => match event.item.details {
                ThreadItemDetails::AgentMessage(message) => {
                    snapshot.final_response =
                        Some(public_text(&message.text, MAX_TEXT_BYTES, &mut snapshot.evidence_truncated))
                }
                ThreadItemDetails::FileChange(change) => {
                    snapshot.invalidate_verification();
                    if change.status == PatchApplyStatus::Completed {
                        if change.changes.is_empty()
                            || change
                                .changes
                                .iter()
                                .any(|path| !vtcode_webmcp::filesystem::is_public_evidence_path(&path.path))
                        {
                            snapshot.evidence_truncated = true;
                            return;
                        }
                        for path in change.changes {
                            if snapshot.changes.len() < MAX_CHANGES {
                                snapshot
                                    .changes
                                    .push(public_text(&path.path, 1024, &mut snapshot.evidence_truncated));
                            } else {
                                snapshot.evidence_truncated = true;
                            }
                        }
                        if let Some(diff) = change.unified_diff {
                            let remaining = MAX_TEXT_BYTES.saturating_sub(snapshot.unified_diff.len());
                            snapshot.unified_diff.push_str(&public_text(
                                &diff,
                                remaining,
                                &mut snapshot.evidence_truncated,
                            ));
                        }
                        snapshot.evidence_truncated |= change.diff_incomplete.unwrap_or(false);
                    }
                }
                ThreadItemDetails::Harness(event) => match event.event {
                    HarnessEventKind::VerificationStarted
                    | HarnessEventKind::VerificationPassed
                    | HarnessEventKind::VerificationFailed => {
                        snapshot.verification = match event.event {
                            HarnessEventKind::VerificationPassed
                                if event.exit_code == Some(0) && event.command.is_some() =>
                            {
                                TaskVerification::Verified
                            }
                            HarnessEventKind::VerificationFailed => TaskVerification::Failed,
                            _ => TaskVerification::Unverified,
                        };
                        snapshot.verification_command = event
                            .command
                            .map(|text| public_text(&text, 2048, &mut snapshot.evidence_truncated));
                        snapshot.verification_exit_code = event.exit_code;
                    }
                    _ => {}
                },
                ThreadItemDetails::CommandExecution(_)
                | ThreadItemDetails::ToolInvocation(_)
                | ThreadItemDetails::McpToolCall(_) => {
                    snapshot.invalidate_verification();
                }
                _ => {}
            },
            _ => {}
        }
        if let Some(reason) = &self.failure_reason {
            snapshot.progress = format!("runtime failure: {reason}");
        }
    }
}

fn public_text(text: &str, limit: usize, truncated: &mut bool) -> String {
    let clean = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
    let redacted = vtcode_commons::sanitizer::redact_secrets(clean);
    let end = vtcode_commons::formatting::truncate_utf8_prefix(&redacted, limit);
    *truncated |= end.len() < redacted.len();
    end.to_owned()
}

async fn collect_events(
    mut reader: impl AsyncBufRead + Unpin,
    records: &Mutex<BTreeMap<String, TaskRecord>>,
    task_id: &str,
    projection: &mut EventProjection,
) -> std::io::Result<()> {
    while let Some(line) = read_event_line(&mut reader).await? {
        let event = serde_json::from_slice::<ThreadEvent>(&line).map_err(|error| {
            std::io::Error::other(format!("invalid exec event JSON at line {} column {}", error.line(), error.column()))
        })?;
        let mut records = records.lock().await;
        if let Some(task) = records.get_mut(task_id) {
            projection.observe(&mut task.snapshot, event);
        }
    }
    Ok(())
}

async fn read_event_line(reader: &mut (impl AsyncBufRead + Unpin)) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::other("incomplete exec event"))
            };
        }
        let count = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        if count > MAX_EVENT_BYTES.saturating_sub(line.len()) {
            return Err(std::io::Error::other("exec event exceeds byte limit"));
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if line.last() == Some(&b'\n') {
            return Ok(Some(line));
        }
    }
}

#[cfg(test)]
mod tests;
