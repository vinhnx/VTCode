use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time;

use crate::config::{HookCommandConfig, HooksConfig};
use crate::exec::events::{CompactionMode, CompactionTrigger};
use crate::permissions::PermissionRequest;
use crate::utils::dot_config::load_lifecycle_hook_approval;

use crate::hooks::lifecycle::compiled::CompiledLifecycleHooks;
use crate::hooks::lifecycle::interpret::{
    HookCommandResult, interpret_permission_request, interpret_post_tool, interpret_pre_tool, interpret_session_end,
    interpret_session_start, interpret_stop, interpret_user_prompt,
};
use crate::hooks::lifecycle::types::{
    HookMessage, NotificationHookType, PermissionRequestHookOutcome, PostToolHookOutcome, PreCompactHookOutcome,
    PreToolHookDecision, PreToolHookOutcome, SessionEndReason, SessionStartHookOutcome, SessionStartTrigger,
    StopHookOutcome, UserPromptHookOutcome,
};
#[cfg(test)]
use crate::hooks::lifecycle::utils::generate_session_id;
use crate::hooks::lifecycle::utils::path_to_string;

const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Upper bound on the post-kill stdout/stderr drain for a timed-out hook.
/// Mirrors `crate::exec::async_command`: a descendant that escaped the process
/// group via `setsid()` can keep a pipe write end open forever, so the drain
/// must be bounded or `run_session_end` can hang (and leak the hook task).
const HOOK_POST_KILL_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Best-effort cleanup for a hook's process group on abort.
///
/// `kill_on_drop(true)` only kills the direct `sh` child, which leaves shell
/// grandchildren alive and holding the captured pipes open. When the owning
/// future is cancelled, this guard kills the whole group; it is disarmed once
/// the hook finishes normally so a deliberately detached descendant is not
/// retroactively killed. Mirrors `crate::exec::async_command`.
#[cfg(unix)]
struct HookProcessGroupGuard {
    pid: Option<u32>,
    armed: bool,
}

#[cfg(unix)]
impl HookProcessGroupGuard {
    fn new(pid: Option<u32>) -> Self {
        Self { pid, armed: true }
    }

    /// Kill at most once: once the child has been reaped its PID can be
    /// recycled, so a second best-effort kill could hit an unrelated group.
    fn kill(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        if let Some(pid) = self.pid {
            let _ = vtcode_bash_runner::process_group::kill_process_group_by_pid(pid);
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for HookProcessGroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

mod payloads;

#[derive(Clone)]
pub struct LifecycleHookEngine {
    inner: Arc<LifecycleHookInner>,
}

impl LifecycleHookEngine {
    /// Test-only convenience constructor. Production session paths must use
    /// [`Self::new_with_session_gated`] so workspace-controlled hook content
    /// stays fail-closed; this un-gated entry point exists only so test
    /// configurations can construct engines directly.
    #[cfg(test)]
    pub fn new(workspace: PathBuf, config: &HooksConfig, trigger: SessionStartTrigger) -> Result<Option<Self>> {
        Self::new_with_session(workspace, config, trigger, generate_session_id())
    }

    /// Test-only convenience constructor; see [`Self::new`].
    #[cfg(test)]
    pub fn new_with_session(
        workspace: PathBuf,
        config: &HooksConfig,
        trigger: SessionStartTrigger,
        session_id: impl Into<String>,
    ) -> Result<Option<Self>> {
        Self::new_with_session_gated(workspace, config, trigger, session_id, false)
    }

    /// Construct the engine with the workspace approval gate.
    ///
    /// When `workspace_gated` is true, the workspace configuration (a
    /// workspace-root `vtcode.toml`, the workspace `.vtcode/` fallback, a
    /// project profile, or a workspace-sourced agent spec with hooks) contains
    /// lifecycle hook commands. No lifecycle command then executes until the
    /// user explicitly approves the exact command set this engine will run,
    /// bound to its digest. Callers must pass `true` whenever
    /// `VTCodeConfig.workspace_lifecycle_hooks` is non-empty or the active
    /// primary agent contributes workspace-controlled hooks.
    pub fn new_with_session_gated(
        workspace: PathBuf,
        config: &HooksConfig,
        trigger: SessionStartTrigger,
        session_id: impl Into<String>,
        workspace_gated: bool,
    ) -> Result<Option<Self>> {
        if config.lifecycle.is_empty() {
            return Ok(None);
        }

        let compiled = CompiledLifecycleHooks::from_config(&config.lifecycle)?;
        if compiled.is_empty() {
            return Ok(None);
        }

        let command_digest = commands_digest(&compiled);

        Ok(Some(Self {
            inner: Arc::new(LifecycleHookInner {
                workspace,
                session_id: session_id.into(),
                trigger,
                hooks: compiled,
                workspace_gated,
                command_digest,
                state: Mutex::new(LifecycleHookState {
                    transcript_path: None,
                    approved_workspace_digest: None,
                }),
            }),
        }))
    }

    pub async fn run_session_start(&self) -> Result<SessionStartHookOutcome> {
        let mut messages = Vec::new();
        let mut additional_context = Vec::new();

        if self.inner.hooks.session_start.is_empty() {
            return Ok(SessionStartHookOutcome { messages, additional_context });
        }

        let trigger_value = self.inner.trigger.as_str().to_owned();
        let payload = self.build_session_start_payload().await?;

        for group in &self.inner.hooks.session_start {
            if !group.matcher.matches(&trigger_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("SessionStart", command, &payload).await {
                    Ok(result) => interpret_session_start(
                        command,
                        &result,
                        &mut messages,
                        &mut additional_context,
                        self.inner.hooks.quiet_success_output,
                    ),
                    Err(err) => messages
                        .push(HookMessage::error(format!("SessionStart hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(SessionStartHookOutcome { messages, additional_context })
    }

    pub async fn run_session_end(&self, turn_id: &str, reason: SessionEndReason) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.session_end.is_empty() {
            return Ok(messages);
        }

        let payload = self.build_session_end_payload(turn_id, reason).await?;
        let reason_value = reason.as_str().to_owned();

        for group in &self.inner.hooks.session_end {
            if !group.matcher.matches(&reason_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("SessionEnd", command, &payload).await {
                    Ok(result) => {
                        interpret_session_end(command, &result, &mut messages, self.inner.hooks.quiet_success_output)
                    }
                    Err(err) => messages
                        .push(HookMessage::error(format!("SessionEnd hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(messages)
    }

    pub async fn run_subagent_start(
        &self,
        parent_session_id: &str,
        child_thread_id: &str,
        agent_name: &str,
        display_label: &str,
        background: bool,
        status: &str,
        transcript_path: Option<&std::path::Path>,
    ) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.subagent_start.is_empty() {
            return Ok(messages);
        }

        let payload = self
            .build_subagent_start_payload(
                parent_session_id,
                child_thread_id,
                agent_name,
                display_label,
                background,
                status,
                transcript_path,
            )
            .await?;
        let matcher_value = agent_name.to_owned();

        for group in &self.inner.hooks.subagent_start {
            if !group.matcher.matches(&matcher_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("SubagentStart", command, &payload).await {
                    Ok(result) => {
                        interpret_session_end(command, &result, &mut messages, self.inner.hooks.quiet_success_output)
                    }
                    Err(err) => messages
                        .push(HookMessage::error(format!("SubagentStart hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(messages)
    }

    pub async fn run_subagent_stop(
        &self,
        parent_session_id: &str,
        child_thread_id: &str,
        agent_name: &str,
        display_label: &str,
        background: bool,
        status: &str,
        transcript_path: Option<&std::path::Path>,
    ) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.subagent_stop.is_empty() {
            return Ok(messages);
        }

        let payload = self
            .build_subagent_stop_payload(
                parent_session_id,
                child_thread_id,
                agent_name,
                display_label,
                background,
                status,
                transcript_path,
            )
            .await?;
        let matcher_value = agent_name.to_owned();

        for group in &self.inner.hooks.subagent_stop {
            if !group.matcher.matches(&matcher_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("SubagentStop", command, &payload).await {
                    Ok(result) => {
                        interpret_session_end(command, &result, &mut messages, self.inner.hooks.quiet_success_output)
                    }
                    Err(err) => messages
                        .push(HookMessage::error(format!("SubagentStop hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(messages)
    }

    /// Execute all `UserPromptSubmit` hooks that match the prompt content.
    pub async fn run_user_prompt_submit(&self, turn_id: &str, prompt: &str) -> Result<UserPromptHookOutcome> {
        let mut outcome = UserPromptHookOutcome::default();

        if self.inner.hooks.user_prompt_submit.is_empty() {
            return Ok(outcome);
        }

        let payload = self.build_user_prompt_payload(turn_id, prompt).await?;

        for group in &self.inner.hooks.user_prompt_submit {
            if !group.matcher.matches(prompt) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("UserPromptSubmit", command, &payload).await {
                    Ok(result) => {
                        interpret_user_prompt(command, &result, &mut outcome, self.inner.hooks.quiet_success_output);
                        if !outcome.allow_prompt {
                            return Ok(outcome);
                        }
                    }
                    Err(err) => outcome
                        .messages
                        .push(HookMessage::error(format!("UserPromptSubmit hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(outcome)
    }

    /// Execute all `PermissionRequest` hooks that match the tool name.
    pub async fn run_permission_request(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        permission_request: &PermissionRequest,
        permission_suggestions: &[Value],
    ) -> Result<PermissionRequestHookOutcome> {
        let mut outcome = PermissionRequestHookOutcome::default();

        if self.inner.hooks.permission_request.is_empty() {
            return Ok(outcome);
        }

        let payload = self
            .build_permission_request_payload(tool_name, tool_input, permission_request, permission_suggestions)
            .await?;

        for group in &self.inner.hooks.permission_request {
            if !group.matcher.matches(tool_name) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PermissionRequest", command, &payload).await {
                    Ok(result) => {
                        interpret_permission_request(
                            command,
                            &result,
                            &mut outcome,
                            self.inner.hooks.quiet_success_output,
                        );
                        if outcome.decision.is_some() {
                            return Ok(outcome);
                        }
                    }
                    Err(err) => outcome.messages.push(HookMessage::error(format!(
                        "PermissionRequest hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(outcome)
    }

    /// Execute all `PreToolUse` hooks that match the tool name.
    ///
    /// Hooks run in configuration order. When a hook returns
    /// `hookSpecificOutput.updatedInput`, later hooks receive the rewritten
    /// tool input in their payload, so policy hooks placed after rewrite
    /// hooks observe the final command. The first Allow/Deny decision still
    /// short-circuits the remaining hooks.
    pub async fn run_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_call_id: Option<&str>,
    ) -> Result<PreToolHookOutcome> {
        let mut outcome = PreToolHookOutcome::default();

        if self.inner.hooks.pre_tool_use.is_empty() {
            return Ok(outcome);
        }

        let mut payload = self.build_pre_tool_payload(tool_name, tool_input, tool_call_id).await?;

        for group in &self.inner.hooks.pre_tool_use {
            if !group.matcher.matches(tool_name) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PreToolUse", command, &payload).await {
                    Ok(result) => {
                        interpret_pre_tool(command, &result, &mut outcome, self.inner.hooks.quiet_success_output);
                        if let Some(updated) = outcome.updated_input.take() {
                            outcome.updated_input = Some(updated.clone());
                            if let Some(payload_obj) = payload.as_object_mut() {
                                payload_obj.insert("tool_input".to_string(), updated);
                            }
                        }
                        match outcome.decision {
                            PreToolHookDecision::Allow | PreToolHookDecision::Deny => {
                                return Ok(outcome);
                            }
                            _ => {}
                        }
                    }
                    Err(err) => outcome
                        .messages
                        .push(HookMessage::error(format!("PreToolUse hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(outcome)
    }

    /// Execute all `PostToolUse` hooks that match the tool name.
    pub async fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_output: &Value,
        tool_call_id: Option<&str>,
    ) -> Result<PostToolHookOutcome> {
        let mut outcome = PostToolHookOutcome::default();

        if self.inner.hooks.post_tool_use.is_empty() {
            return Ok(outcome);
        }

        let payload = self
            .build_post_tool_payload(tool_name, tool_input, tool_output, tool_call_id)
            .await?;

        for group in &self.inner.hooks.post_tool_use {
            if !group.matcher.matches(tool_name) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PostToolUse", command, &payload).await {
                    Ok(result) => {
                        interpret_post_tool(command, &result, &mut outcome, self.inner.hooks.quiet_success_output)
                    }
                    Err(err) => outcome
                        .messages
                        .push(HookMessage::error(format!("PostToolUse hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(outcome)
    }

    /// Execute all `PreCompact` hooks that match the compaction trigger.
    pub async fn run_pre_compact(
        &self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
    ) -> Result<PreCompactHookOutcome> {
        let mut outcome = PreCompactHookOutcome::default();

        if self.inner.hooks.pre_compact.is_empty() {
            return Ok(outcome);
        }

        let payload = self
            .build_pre_compact_payload(
                trigger,
                mode,
                original_message_count,
                compacted_message_count,
                history_artifact_path,
            )
            .await?;
        let trigger_value = trigger.as_str().to_owned();

        for group in &self.inner.hooks.pre_compact {
            if !group.matcher.matches(&trigger_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PreCompact", command, &payload).await {
                    Ok(result) => {
                        interpret_session_end(
                            command,
                            &result,
                            &mut outcome.messages,
                            self.inner.hooks.quiet_success_output,
                        );
                    }
                    Err(err) => outcome
                        .messages
                        .push(HookMessage::error(format!("PreCompact hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(outcome)
    }

    /// Execute all `Notification` hooks that match the notification type.
    pub async fn run_notification(
        &self,
        notification_type: NotificationHookType,
        title: &str,
        message: &str,
    ) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.notification.is_empty() {
            return Ok(messages);
        }

        let payload = self.build_notification_payload(notification_type, title, message).await?;
        let matcher_value = notification_type.as_str().to_owned();

        for group in &self.inner.hooks.notification {
            if !group.matcher.matches(&matcher_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("Notification", command, &payload).await {
                    Ok(result) => {
                        interpret_session_end(command, &result, &mut messages, self.inner.hooks.quiet_success_output)
                    }
                    Err(err) => messages
                        .push(HookMessage::error(format!("Notification hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(messages)
    }

    /// Execute all `Stop` hooks.
    pub async fn run_stop(&self, last_assistant_message: &str, stop_hook_active: bool) -> Result<StopHookOutcome> {
        let mut outcome = StopHookOutcome::default();

        if self.inner.hooks.stop.is_empty() {
            return Ok(outcome);
        }

        let payload = self.build_stop_payload(last_assistant_message, stop_hook_active).await?;

        for group in &self.inner.hooks.stop {
            if !group.matcher.matches("stop") {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("Stop", command, &payload).await {
                    Ok(result) => {
                        interpret_stop(command, &result, &mut outcome, self.inner.hooks.quiet_success_output);
                        if outcome.block_reason.is_some() {
                            return Ok(outcome);
                        }
                    }
                    Err(err) => outcome
                        .messages
                        .push(HookMessage::error(format!("Stop hook `{}` failed: {err}", command.command))),
                }
            }
        }

        Ok(outcome)
    }

    /// Update the transcript path that hooks can access via the `VT_TRANSCRIPT_PATH` env var.
    pub async fn update_transcript_path(&self, path: Option<PathBuf>) {
        let mut state = self.inner.state.lock().await;
        state.transcript_path = path;
    }

    /// Return the current transcript path, if set.
    pub async fn transcript_path(&self) -> Option<PathBuf> {
        let state = self.inner.state.lock().await;
        state.transcript_path.clone()
    }

    /// Whether the workspace approval gate is active for this engine.
    pub fn workspace_gated(&self) -> bool {
        self.inner.workspace_gated
    }

    /// Digest of the exact lifecycle command set this engine will execute.
    /// Approvals are bound to this digest and revalidated before every spawn.
    pub fn command_digest(&self) -> &str {
        &self.inner.command_digest
    }

    /// Every lifecycle command this engine will execute, with its canonical
    /// event key — the exact set an approval covers.
    pub fn command_previews(&self) -> Vec<LifecycleHookCommandPreview> {
        let mut previews = Vec::new();
        self.inner.hooks.for_each_command(|event, matcher, command| {
            previews.push(LifecycleHookCommandPreview {
                event,
                matcher: matcher.map(str::to_string),
                command: command.command.clone(),
                timeout_seconds: command.timeout_seconds,
            });
        });
        previews
    }

    /// Mark this engine's exact command set as approved for the session.
    /// Only a digest equal to `command_digest()` satisfies the gate.
    pub async fn approve_workspace_hooks(&self) {
        let mut state = self.inner.state.lock().await;
        state.approved_workspace_digest = Some(self.inner.command_digest.clone());
    }

    /// Whether the gate is active and the current command set is not yet
    /// approved. The binary prompts the user before the first lifecycle run
    /// when this is true.
    pub async fn workspace_hooks_need_approval(&self) -> bool {
        if !self.inner.workspace_gated {
            return false;
        }
        let state = self.inner.state.lock().await;
        state.approved_workspace_digest.as_deref() != Some(self.inner.command_digest.as_str())
    }

    /// Fail-closed gate: when workspace-controlled hook content is present,
    /// the engine refuses to spawn any lifecycle command until its exact
    /// command set has been approved.
    async fn workspace_approval_granted(&self) -> bool {
        if !self.inner.workspace_gated {
            return true;
        }
        let state = self.inner.state.lock().await;
        state.approved_workspace_digest.as_deref() == Some(self.inner.command_digest.as_str())
    }

    async fn execute_command(
        &self,
        event_name: &str,
        command: &HookCommandConfig,
        payload: &Value,
    ) -> Result<HookCommandResult> {
        if !self.workspace_approval_granted().await {
            return Err(anyhow::anyhow!(
                "skipped: the workspace configuration defines lifecycle hooks that are not approved for this workspace. \
                 Approve workspace lifecycle hooks at session start to enable `{}` (event: {event_name}).",
                command.command
            ));
        }

        let mut process = Command::new("sh");
        process.arg("-c").arg(&command.command);
        process.current_dir(&self.inner.workspace);
        process.stdin(Stdio::piped());
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        process.kill_on_drop(true);
        // Own process group (PGID == PID) on Unix so timeout/abort kills reach
        // shell grandchildren via killpg, not just the direct `sh` child.
        #[cfg(unix)]
        process.process_group(0);

        let workspace_str = self.inner.workspace.to_string_lossy().into_owned();
        process.env("VT_PROJECT_DIR", &workspace_str);
        process.env("CLAUDE_PROJECT_DIR", &workspace_str);
        process.env("VT_SESSION_ID", &self.inner.session_id);
        process.env("CLAUDE_SESSION_ID", &self.inner.session_id);
        process.env("VT_HOOK_EVENT", event_name);

        if let Some(transcript_path) = self.current_transcript_path().await {
            process.env("VT_TRANSCRIPT_PATH", &transcript_path);
            process.env("CLAUDE_TRANSCRIPT_PATH", &transcript_path);
        }

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to spawn lifecycle hook `{}`", command.command))?;
        #[cfg(unix)]
        let mut group_guard = HookProcessGroupGuard::new(child.id());

        if let Some(mut stdin) = child.stdin.take() {
            let mut payload_bytes =
                serde_json::to_vec(payload).context("failed to serialize lifecycle hook payload")?;
            payload_bytes.push(b'\n');
            stdin
                .write_all(&payload_bytes)
                .await
                .context("failed to write lifecycle hook payload")?;
            stdin.shutdown().await.context("failed to close lifecycle hook stdin")?;
        }

        let mut stdout_pipe = child.stdout.take().context("lifecycle hook missing stdout pipe")?;
        let mut stderr_pipe = child.stderr.take().context("lifecycle hook missing stderr pipe")?;

        let stdout_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            stdout_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
        });
        let stderr_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            stderr_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
        });

        let timeout_secs = command.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECS).max(1);
        let wait_result = time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;

        let (exit_code, timed_out) = match wait_result {
            Ok(status_res) => {
                let status = status_res.context("failed to wait for lifecycle hook")?;
                #[cfg(unix)]
                group_guard.disarm();
                (status.code(), false)
            }
            Err(_) => {
                #[cfg(unix)]
                group_guard.kill();
                #[cfg(not(unix))]
                let _ = child.start_kill();
                let _ = child.wait().await;
                (None, true)
            }
        };

        // After a timeout the group kill should close every writer, but a
        // setsid-escaped descendant can still hold a pipe open — bound the
        // drain so a hook can never stall the lifecycle engine.
        let stdout_bytes = if timed_out {
            match time::timeout(HOOK_POST_KILL_DRAIN_TIMEOUT, stdout_task).await {
                Ok(result) => result.unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            stdout_task.await.unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default()
        };
        let stderr_bytes = if timed_out {
            match time::timeout(HOOK_POST_KILL_DRAIN_TIMEOUT, stderr_task).await {
                Ok(result) => result.unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            stderr_task.await.unwrap_or_else(|_| Ok(Vec::new())).unwrap_or_default()
        };

        Ok(HookCommandResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
            timed_out,
            timeout_seconds: timeout_secs,
        })
    }

    async fn current_transcript_path(&self) -> Option<String> {
        let state = self.inner.state.lock().await;
        state.transcript_path.as_ref().and_then(|path| path_to_string(path))
    }
}

struct LifecycleHookInner {
    workspace: PathBuf,
    session_id: String,
    trigger: SessionStartTrigger,
    hooks: CompiledLifecycleHooks,
    workspace_gated: bool,
    command_digest: String,
    state: Mutex<LifecycleHookState>,
}

struct LifecycleHookState {
    transcript_path: Option<PathBuf>,
    approved_workspace_digest: Option<String>,
}

/// A lifecycle hook command exactly as the engine will execute it, with its
/// canonical snake_case event key. Displayed to the user so an approval covers
/// the precise command set.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LifecycleHookCommandPreview {
    /// Canonical event key, e.g. `session_start`.
    pub event: &'static str,
    /// Optional regex matcher of the containing group.
    pub matcher: Option<String>,
    /// The shell command string executed via `sh -c`.
    pub command: String,
    /// Optional execution timeout in seconds.
    pub timeout_seconds: Option<u64>,
}

/// Stable digest over the engine's exact compiled command set. Deterministic
/// across processes so approvals can be persisted and revalidated. Commands
/// are serialized as JSON lines (length-safe, no delimiter ambiguity) and
/// hashed with SHA-256, matching the codebase's config-fingerprint approach
/// (collision-resistant; a crafted command set cannot collide with an
/// approved one).
fn commands_digest(hooks: &CompiledLifecycleHooks) -> String {
    let mut lines: Vec<String> = Vec::new();
    hooks.for_each_command(|event, matcher, command| {
        let entry = LifecycleHookCommandPreview {
            event,
            matcher: matcher.map(str::to_string),
            command: command.command.clone(),
            timeout_seconds: command.timeout_seconds,
        };
        if let Ok(line) = serde_json::to_string(&entry) {
            lines.push(line);
        }
    });
    lines.sort();
    vtcode_commons::utils::calculate_sha256(lines.join("\n").as_bytes())
}

/// Restore a persisted workspace lifecycle-hook approval onto a freshly built
/// engine. The persisted record only applies when its digest still matches the
/// engine's exact command set; any configuration change since approval leaves
/// the engine fail-closed.
pub async fn restore_workspace_hook_approval(engine: &LifecycleHookEngine, workspace: &std::path::Path) {
    if !engine.workspace_gated() {
        return;
    }
    let digest = engine.command_digest().to_string();
    let Ok(Some(record)) = load_lifecycle_hook_approval(workspace).await else {
        return;
    };
    if record.config_digest == digest {
        engine.approve_workspace_hooks().await;
    }
}

/// Carry an in-memory approval from a previous engine onto a rebuilt engine
/// (primary-agent switch or config reload), falling back to the persisted
/// record. A session-only approval that could not be persisted (e.g. dot-config
/// write failure) survives the rebuild while the command set is unchanged,
/// instead of being silently dropped mid-session. Fails closed in every other
/// case: a changed command set, a previously unapproved engine, or a gated
/// engine with no matching persisted record leaves the rebuilt engine gated.
pub async fn carry_or_restore_workspace_hook_approval(
    previous: Option<&LifecycleHookEngine>,
    next: &LifecycleHookEngine,
    workspace: &std::path::Path,
) {
    let previous_approved_same_set = match previous {
        Some(prev) => {
            prev.workspace_gated()
                && prev.command_digest() == next.command_digest()
                && !prev.workspace_hooks_need_approval().await
        }
        None => false,
    };
    if previous_approved_same_set {
        next.approve_workspace_hooks().await;
    } else {
        restore_workspace_hook_approval(next, workspace).await;
    }
}
