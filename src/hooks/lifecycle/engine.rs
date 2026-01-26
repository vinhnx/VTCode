use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time;

use vtcode_core::config::{HookCommandConfig, HooksConfig};

use crate::hooks::lifecycle::compiled::CompiledLifecycleHooks;
use crate::hooks::lifecycle::interpret::{
    HookCommandResult, interpret_post_tool, interpret_pre_tool, interpret_session_end,
    interpret_session_start, interpret_task_completion, interpret_user_prompt,
};
use crate::hooks::lifecycle::types::{
    HookMessage, PostToolHookOutcome, PreToolHookDecision, PreToolHookOutcome, SessionEndReason,
    SessionStartHookOutcome, SessionStartTrigger, UserPromptHookOutcome,
};
use crate::hooks::lifecycle::utils::{generate_session_id, path_to_string};

const DEFAULT_TIMEOUT_SECS: u64 = 60;

#[derive(Clone)]
pub struct LifecycleHookEngine {
    inner: Arc<LifecycleHookInner>,
}

impl LifecycleHookEngine {
    pub fn new(
        workspace: PathBuf,
        config: &HooksConfig,
        trigger: SessionStartTrigger,
    ) -> Result<Option<Self>> {
        if config.lifecycle.is_empty() {
            return Ok(None);
        }

        let compiled = CompiledLifecycleHooks::from_config(&config.lifecycle)?;
        if compiled.is_empty() {
            return Ok(None);
        }

        let session_id = generate_session_id();
        Ok(Some(Self {
            inner: Arc::new(LifecycleHookInner {
                workspace,
                session_id,
                trigger,
                hooks: compiled,
                state: Mutex::new(LifecycleHookState {
                    transcript_path: None,
                }),
            }),
        }))
    }

    pub async fn run_session_start(&self) -> Result<SessionStartHookOutcome> {
        let mut messages = Vec::new();
        let mut additional_context = Vec::new();

        if self.inner.hooks.session_start.is_empty() {
            return Ok(SessionStartHookOutcome {
                messages,
                additional_context,
            });
        }

        let trigger_value = self.inner.trigger.as_str().to_owned();
        let payload = self.build_session_start_payload().await?;

        for group in &self.inner.hooks.session_start {
            if !group.matcher.matches(&trigger_value) {
                continue;
            }

            for command in &group.commands {
                match self
                    .execute_command("SessionStart", command, &payload)
                    .await
                {
                    Ok(result) => interpret_session_start(
                        command,
                        &result,
                        &mut messages,
                        &mut additional_context,
                    ),
                    Err(err) => messages.push(HookMessage::error(format!(
                        "SessionStart hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(SessionStartHookOutcome {
            messages,
            additional_context,
        })
    }

    pub async fn run_session_end(&self, reason: SessionEndReason) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.session_end.is_empty() {
            return Ok(messages);
        }

        let payload = self.build_session_end_payload(reason).await?;
        let reason_value = reason.as_str().to_owned();

        for group in &self.inner.hooks.session_end {
            if !group.matcher.matches(&reason_value) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("SessionEnd", command, &payload).await {
                    Ok(result) => interpret_session_end(command, &result, &mut messages),
                    Err(err) => messages.push(HookMessage::error(format!(
                        "SessionEnd hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(messages)
    }

    pub async fn run_user_prompt_submit(&self, prompt: &str) -> Result<UserPromptHookOutcome> {
        let mut outcome = UserPromptHookOutcome::default();

        if self.inner.hooks.user_prompt_submit.is_empty() {
            return Ok(outcome);
        }

        let payload = self.build_user_prompt_payload(prompt).await?;

        for group in &self.inner.hooks.user_prompt_submit {
            if !group.matcher.matches(prompt) {
                continue;
            }

            for command in &group.commands {
                match self
                    .execute_command("UserPromptSubmit", command, &payload)
                    .await
                {
                    Ok(result) => {
                        interpret_user_prompt(command, &result, &mut outcome);
                        if !outcome.allow_prompt {
                            return Ok(outcome);
                        }
                    }
                    Err(err) => outcome.messages.push(HookMessage::error(format!(
                        "UserPromptSubmit hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(outcome)
    }

    pub async fn run_pre_tool_use(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
    ) -> Result<PreToolHookOutcome> {
        let mut outcome = PreToolHookOutcome::default();

        if self.inner.hooks.pre_tool_use.is_empty() {
            return Ok(outcome);
        }

        let payload = self.build_pre_tool_payload(tool_name, tool_input).await?;

        for group in &self.inner.hooks.pre_tool_use {
            if !group.matcher.matches(tool_name) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PreToolUse", command, &payload).await {
                    Ok(result) => {
                        interpret_pre_tool(command, &result, &mut outcome);
                        match outcome.decision {
                            PreToolHookDecision::Allow | PreToolHookDecision::Deny => {
                                return Ok(outcome);
                            }
                            _ => {}
                        }
                    }
                    Err(err) => outcome.messages.push(HookMessage::error(format!(
                        "PreToolUse hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(outcome)
    }

    pub async fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_output: &Value,
    ) -> Result<PostToolHookOutcome> {
        let mut outcome = PostToolHookOutcome::default();

        if self.inner.hooks.post_tool_use.is_empty() {
            return Ok(outcome);
        }

        let payload = self
            .build_post_tool_payload(tool_name, tool_input, tool_output)
            .await?;

        for group in &self.inner.hooks.post_tool_use {
            if !group.matcher.matches(tool_name) {
                continue;
            }

            for command in &group.commands {
                match self.execute_command("PostToolUse", command, &payload).await {
                    Ok(result) => interpret_post_tool(command, &result, &mut outcome),
                    Err(err) => outcome.messages.push(HookMessage::error(format!(
                        "PostToolUse hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(outcome)
    }

    #[allow(dead_code)]
    pub async fn run_task_completion(
        &self,
        task_name: &str,
        status: &str,
        details: Option<&Value>,
    ) -> Result<Vec<HookMessage>> {
        let mut messages = Vec::new();

        if self.inner.hooks.task_completion.is_empty() {
            return Ok(messages);
        }

        let payload = self
            .build_task_completion_payload(task_name, status, details)
            .await?;

        for group in &self.inner.hooks.task_completion {
            if !group.matcher.matches(task_name) {
                continue;
            }

            for command in &group.commands {
                match self
                    .execute_command("TaskCompletion", command, &payload)
                    .await
                {
                    Ok(result) => interpret_task_completion(command, &result, &mut messages),
                    Err(err) => messages.push(HookMessage::error(format!(
                        "TaskCompletion hook `{}` failed: {err}",
                        command.command
                    ))),
                }
            }
        }

        Ok(messages)
    }

    pub async fn update_transcript_path(&self, path: Option<PathBuf>) {
        let mut state = self.inner.state.lock().await;
        state.transcript_path = path;
    }

    async fn build_session_start_payload(&self) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "SessionStart",
            "source": self.inner.trigger.as_str(),
            "transcript_path": transcript_path,
        }))
    }

    async fn build_session_end_payload(&self, reason: SessionEndReason) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "SessionEnd",
            "reason": reason.as_str(),
            "transcript_path": transcript_path,
        }))
    }

    async fn build_user_prompt_payload(&self, prompt: &str) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "UserPromptSubmit",
            "prompt": prompt,
            "transcript_path": transcript_path,
        }))
    }

    async fn build_pre_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "PreToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input.cloned().unwrap_or(Value::Null),
            "transcript_path": transcript_path,
        }))
    }

    async fn build_post_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_output: &Value,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "PostToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input.cloned().unwrap_or(Value::Null),
            "tool_response": tool_output.clone(),
            "transcript_path": transcript_path,
        }))
    }

    #[allow(dead_code)]
    async fn build_task_completion_payload(
        &self,
        task_name: &str,
        status: &str,
        details: Option<&Value>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "TaskCompletion",
            "task_name": task_name,
            "status": status,
            "details": details.cloned().unwrap_or(Value::Null),
            "transcript_path": transcript_path,
        }))
    }

    async fn execute_command(
        &self,
        event_name: &str,
        command: &HookCommandConfig,
        payload: &Value,
    ) -> Result<HookCommandResult> {
        let mut process = Command::new("sh");
        process.arg("-c").arg(&command.command);
        process.current_dir(&self.inner.workspace);
        process.stdin(Stdio::piped());
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());
        process.kill_on_drop(true);

        let workspace_str = self.inner.workspace.to_string_lossy().into_owned();
        process.env("VT_PROJECT_DIR", &workspace_str);
        process.env("VT_SESSION_ID", &self.inner.session_id);
        process.env("VT_HOOK_EVENT", event_name);

        if let Some(transcript_path) = self.current_transcript_path().await {
            process.env("VT_TRANSCRIPT_PATH", &transcript_path);
        }

        let mut child = process
            .spawn()
            .with_context(|| format!("failed to spawn lifecycle hook `{}`", command.command))?;

        if let Some(mut stdin) = child.stdin.take() {
            let mut payload_bytes = serde_json::to_vec(payload)
                .context("failed to serialize lifecycle hook payload")?;
            payload_bytes.push(b'\n');
            stdin
                .write_all(&payload_bytes)
                .await
                .context("failed to write lifecycle hook payload")?;
            stdin
                .shutdown()
                .await
                .context("failed to close lifecycle hook stdin")?;
        }

        let mut stdout_pipe = child
            .stdout
            .take()
            .context("lifecycle hook missing stdout pipe")?;
        let mut stderr_pipe = child
            .stderr
            .take()
            .context("lifecycle hook missing stderr pipe")?;

        let stdout_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            stdout_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
        });
        let stderr_task = tokio::spawn(async move {
            let mut buffer = Vec::new();
            stderr_pipe.read_to_end(&mut buffer).await.map(|_| buffer)
        });

        let timeout_secs = command
            .timeout_seconds
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .max(1);
        let wait_result = time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;

        let (exit_code, timed_out) = match wait_result {
            Ok(status_res) => {
                let status = status_res.context("failed to wait for lifecycle hook")?;
                (status.code(), false)
            }
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                (None, true)
            }
        };

        let stdout_bytes = stdout_task
            .await
            .unwrap_or_else(|_| Ok(Vec::new()))
            .unwrap_or_default();
        let stderr_bytes = stderr_task
            .await
            .unwrap_or_else(|_| Ok(Vec::new()))
            .unwrap_or_default();

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
        state
            .transcript_path
            .as_ref()
            .and_then(|path| path_to_string(path))
    }
}

struct LifecycleHookInner {
    workspace: PathBuf,
    session_id: String,
    trigger: SessionStartTrigger,
    hooks: CompiledLifecycleHooks,
    state: Mutex<LifecycleHookState>,
}

struct LifecycleHookState {
    transcript_path: Option<PathBuf>,
}
