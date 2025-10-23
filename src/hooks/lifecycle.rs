use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time;

use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};

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

        let trigger_value = self.inner.trigger.as_str().to_string();
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
        let reason_value = reason.as_str().to_string();

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

    pub async fn update_transcript_path(&self, path: Option<PathBuf>) {
        let mut state = self.inner.state.lock().await;
        state.transcript_path = path;
    }

    async fn build_session_start_payload(&self) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().to_string();
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
        let cwd = self.inner.workspace.to_string_lossy().to_string();
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
        let cwd = self.inner.workspace.to_string_lossy().to_string();
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
        let cwd = self.inner.workspace.to_string_lossy().to_string();
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
        let cwd = self.inner.workspace.to_string_lossy().to_string();
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

        let workspace_str = self.inner.workspace.to_string_lossy().to_string();
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
            stdout: String::from_utf8_lossy(&stdout_bytes).to_string(),
            stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
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

#[derive(Debug, Clone)]
pub struct HookMessage {
    pub level: HookMessageLevel,
    pub text: String,
}

impl HookMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Info,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            level: HookMessageLevel::Error,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum HookMessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Default)]
pub struct SessionStartHookOutcome {
    pub messages: Vec<HookMessage>,
    pub additional_context: Vec<String>,
}

pub struct UserPromptHookOutcome {
    pub allow_prompt: bool,
    pub block_reason: Option<String>,
    pub additional_context: Vec<String>,
    pub messages: Vec<HookMessage>,
}

impl Default for UserPromptHookOutcome {
    fn default() -> Self {
        Self {
            allow_prompt: true,
            block_reason: None,
            additional_context: Vec::new(),
            messages: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct PreToolHookOutcome {
    pub decision: PreToolHookDecision,
    pub messages: Vec<HookMessage>,
}

#[derive(Default)]
pub struct PostToolHookOutcome {
    pub messages: Vec<HookMessage>,
    pub additional_context: Vec<String>,
    pub block_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum PreToolHookDecision {
    Continue,
    Allow,
    Deny,
    Ask,
}

impl Default for PreToolHookDecision {
    fn default() -> Self {
        Self::Continue
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum SessionStartTrigger {
    Startup,
    Resume,
    Clear,
    Compact,
}

impl SessionStartTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Clear => "clear",
            Self::Compact => "compact",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum SessionEndReason {
    Completed,
    Exit,
    Cancelled,
    Error,
    Other,
}

impl SessionEndReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Exit => "exit",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::Other => "other",
        }
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

struct CompiledLifecycleHooks {
    session_start: Vec<CompiledHookGroup>,
    session_end: Vec<CompiledHookGroup>,
    user_prompt_submit: Vec<CompiledHookGroup>,
    pre_tool_use: Vec<CompiledHookGroup>,
    post_tool_use: Vec<CompiledHookGroup>,
}

impl CompiledLifecycleHooks {
    fn from_config(config: &LifecycleHooksConfig) -> Result<Self> {
        Ok(Self {
            session_start: compile_groups(&config.session_start)?,
            session_end: compile_groups(&config.session_end)?,
            user_prompt_submit: compile_groups(&config.user_prompt_submit)?,
            pre_tool_use: compile_groups(&config.pre_tool_use)?,
            post_tool_use: compile_groups(&config.post_tool_use)?,
        })
    }

    fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.user_prompt_submit.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
    }
}

struct CompiledHookGroup {
    matcher: HookMatcher,
    commands: Vec<HookCommandConfig>,
}

#[derive(Clone)]
enum HookMatcher {
    Any,
    Pattern(Regex),
}

impl HookMatcher {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Pattern(regex) => regex.is_match(value),
        }
    }
}

struct HookCommandResult {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    timeout_seconds: u64,
}

fn compile_groups(groups: &[HookGroupConfig]) -> Result<Vec<CompiledHookGroup>> {
    let mut compiled = Vec::new();
    for group in groups {
        let matcher = if let Some(pattern) = group.matcher.as_ref() {
            let trimmed = pattern.trim();
            if trimmed.is_empty() || trimmed == "*" {
                HookMatcher::Any
            } else {
                let regex = Regex::new(&format!("^(?:{})$", trimmed))
                    .with_context(|| format!("invalid lifecycle hook matcher: {pattern}"))?;
                HookMatcher::Pattern(regex)
            }
        } else {
            HookMatcher::Any
        };

        compiled.push(CompiledHookGroup {
            matcher,
            commands: group.hooks.clone(),
        });
    }

    Ok(compiled)
}

fn generate_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("vt-{}-{nanos}", std::process::id())
}

fn path_to_string(path: &Path) -> Option<String> {
    Some(path.to_string_lossy().to_string())
}

fn parse_json_output(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str(trimmed).ok()
}

struct CommonJsonFields {
    continue_decision: Option<bool>,
    stop_reason: Option<String>,
    suppress_stdout: bool,
    decision: Option<String>,
    decision_reason: Option<String>,
    hook_specific: Option<Value>,
}

fn extract_common_fields(json: &Value, messages: &mut Vec<HookMessage>) -> CommonJsonFields {
    if let Some(system_message) = json
        .get("systemMessage")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        messages.push(HookMessage::info(system_message.to_string()));
    }

    let continue_decision = json.get("continue").and_then(|value| value.as_bool());
    let stop_reason = json
        .get("stopReason")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let suppress_stdout = json
        .get("suppressOutput")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let decision = json
        .get("decision")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let decision_reason = json
        .get("reason")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let hook_specific = json.get("hookSpecificOutput").cloned();

    CommonJsonFields {
        continue_decision,
        stop_reason,
        suppress_stdout,
        decision,
        decision_reason,
        hook_specific,
    }
}

fn matches_hook_event(spec: &serde_json::Map<String, Value>, event_name: &str) -> bool {
    match spec.get("hookEventName").and_then(|value| value.as_str()) {
        Some(name) => name.eq_ignore_ascii_case(event_name),
        None => true,
    }
}

fn handle_timeout(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
) {
    if result.timed_out {
        messages.push(HookMessage::error(format!(
            "Hook `{}` timed out after {}s",
            command.command, result.timeout_seconds
        )));
    }
}

fn handle_non_zero_exit(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    code: i32,
    messages: &mut Vec<HookMessage>,
    warn: bool,
) {
    let level = if warn {
        HookMessageLevel::Warning
    } else {
        HookMessageLevel::Error
    };

    let text = if result.stderr.trim().is_empty() {
        format!("Hook `{}` exited with status {code}", command.command)
    } else {
        format!(
            "Hook `{}` exited with status {code}: {}",
            command.command,
            result.stderr.trim()
        )
    };

    messages.push(HookMessage { level, text });
}

fn select_message<'a>(stderr: &'a str, fallback: &'a str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn interpret_session_start(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
    additional_context: &mut Vec<String>,
) {
    handle_timeout(command, result, messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code != 0 {
            handle_non_zero_exit(command, result, code, messages, false);
        }
    }

    if !result.stderr.trim().is_empty() {
        messages.push(HookMessage::error(format!(
            "SessionStart hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, messages);
        if let Some(Value::Object(spec)) = common.hook_specific {
            if matches_hook_event(&spec, "SessionStart") {
                if let Some(additional) = spec
                    .get("additionalContext")
                    .and_then(|value| value.as_str())
                {
                    if !additional.trim().is_empty() {
                        additional_context.push(additional.trim().to_string());
                    }
                }
            }
        }

        if !common.suppress_stdout {
            if let Some(text) = json
                .get("additional_context")
                .and_then(|value| value.as_str())
            {
                if !text.trim().is_empty() {
                    additional_context.push(text.trim().to_string());
                }
            }
        }
    } else if !result.stdout.trim().is_empty() {
        additional_context.push(result.stdout.trim().to_string());
    }
}

fn interpret_session_end(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    messages: &mut Vec<HookMessage>,
) {
    handle_timeout(command, result, messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code != 0 {
            handle_non_zero_exit(command, result, code, messages, false);
        }
    }

    if !result.stderr.trim().is_empty() {
        messages.push(HookMessage::error(format!(
            "SessionEnd hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let _ = extract_common_fields(&json, messages);
    } else if !result.stdout.trim().is_empty() {
        messages.push(HookMessage::info(result.stdout.trim().to_string()));
    }
}

fn interpret_user_prompt(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut UserPromptHookOutcome,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code == 2 {
            outcome.allow_prompt = false;
            let reason = select_message(result.stderr.trim(), "Prompt blocked by lifecycle hook.");
            outcome.block_reason = Some(reason.clone());
            outcome.messages.push(HookMessage::error(reason));
            return;
        } else if code != 0 {
            handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
        }
    }

    if !result.stderr.trim().is_empty() {
        outcome.messages.push(HookMessage::warning(format!(
            "UserPromptSubmit hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(false) = common.continue_decision {
            outcome.allow_prompt = false;
            outcome.block_reason = common
                .stop_reason
                .clone()
                .or(common.decision_reason.clone())
                .or_else(|| Some("Prompt blocked by lifecycle hook.".to_string()));
        }

        if let Some(decision) = common.decision.as_deref() {
            if decision.eq_ignore_ascii_case("block") {
                outcome.allow_prompt = false;
                outcome.block_reason = common
                    .decision_reason
                    .clone()
                    .or_else(|| Some("Prompt blocked by lifecycle hook.".to_string()));
            }
        }

        if let Some(Value::Object(spec)) = common.hook_specific {
            if matches_hook_event(&spec, "UserPromptSubmit") {
                if let Some(additional) = spec
                    .get("additionalContext")
                    .and_then(|value| value.as_str())
                {
                    if !additional.trim().is_empty() {
                        outcome
                            .additional_context
                            .push(additional.trim().to_string());
                    }
                }
            }
        }

        if !common.suppress_stdout {
            if let Some(text) = json
                .get("additional_context")
                .and_then(|value| value.as_str())
            {
                if !text.trim().is_empty() {
                    outcome.additional_context.push(text.trim().to_string());
                }
            }
        }

        if !outcome.allow_prompt {
            if let Some(reason) = outcome.block_reason.clone() {
                outcome.messages.push(HookMessage::error(reason));
            }
        }
    } else if !result.stdout.trim().is_empty() {
        outcome
            .additional_context
            .push(result.stdout.trim().to_string());
    }
}

fn interpret_pre_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PreToolHookOutcome,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        if matches!(outcome.decision, PreToolHookDecision::Continue) {
            outcome.decision = PreToolHookDecision::Deny;
            outcome.messages.push(HookMessage::error(format!(
                "Tool call blocked because hook `{}` timed out",
                command.command
            )));
        }
        return;
    }

    if let Some(code) = result.exit_code {
        if code == 2 {
            outcome.decision = PreToolHookDecision::Deny;
            let reason =
                select_message(result.stderr.trim(), "Tool call blocked by lifecycle hook.");
            outcome.messages.push(HookMessage::error(reason));
            return;
        } else if code != 0 {
            handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
        }
    }

    if !result.stderr.trim().is_empty() {
        outcome.messages.push(HookMessage::warning(format!(
            "PreToolUse hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(false) = common.continue_decision {
            outcome.decision = PreToolHookDecision::Deny;
            if let Some(reason) = common.stop_reason.or(common.decision_reason) {
                outcome.messages.push(HookMessage::error(reason));
            }
            return;
        }

        if let Some(Value::Object(spec)) = common.hook_specific {
            if matches_hook_event(&spec, "PreToolUse") {
                if let Some(decision) = spec
                    .get("permissionDecision")
                    .and_then(|value| value.as_str())
                {
                    match decision {
                        "allow" => outcome.decision = PreToolHookDecision::Allow,
                        "deny" => outcome.decision = PreToolHookDecision::Deny,
                        "ask" => {
                            if matches!(outcome.decision, PreToolHookDecision::Continue) {
                                outcome.decision = PreToolHookDecision::Ask;
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(reason) = spec
                    .get("permissionDecisionReason")
                    .and_then(|value| value.as_str())
                {
                    if !reason.trim().is_empty() {
                        outcome
                            .messages
                            .push(HookMessage::info(reason.trim().to_string()));
                    }
                }
            }
        }

        if !common.suppress_stdout && !result.stdout.trim().is_empty() {
            outcome
                .messages
                .push(HookMessage::info(result.stdout.trim().to_string()));
        }
    } else if !result.stdout.trim().is_empty() {
        outcome
            .messages
            .push(HookMessage::info(result.stdout.trim().to_string()));
    }
}

fn interpret_post_tool(
    command: &HookCommandConfig,
    result: &HookCommandResult,
    outcome: &mut PostToolHookOutcome,
) {
    handle_timeout(command, result, &mut outcome.messages);
    if result.timed_out {
        return;
    }

    if let Some(code) = result.exit_code {
        if code != 0 {
            handle_non_zero_exit(command, result, code, &mut outcome.messages, true);
        }
    }

    if !result.stderr.trim().is_empty() {
        outcome.messages.push(HookMessage::warning(format!(
            "PostToolUse hook `{}` stderr: {}",
            command.command,
            result.stderr.trim()
        )));
    }

    if let Some(json) = parse_json_output(&result.stdout) {
        let common = extract_common_fields(&json, &mut outcome.messages);
        if let Some(decision) = common.decision.as_deref() {
            if decision.eq_ignore_ascii_case("block") {
                outcome.block_reason = common
                    .decision_reason
                    .clone()
                    .or_else(|| Some("Tool execution requires attention.".to_string()));
            }
        }

        if let Some(Value::Object(spec)) = common.hook_specific {
            if matches_hook_event(&spec, "PostToolUse") {
                if let Some(additional) = spec
                    .get("additionalContext")
                    .and_then(|value| value.as_str())
                {
                    if !additional.trim().is_empty() {
                        outcome
                            .additional_context
                            .push(additional.trim().to_string());
                    }
                }
            }
        }

        if !common.suppress_stdout {
            if let Some(text) = json
                .get("additional_context")
                .and_then(|value| value.as_str())
            {
                if !text.trim().is_empty() {
                    outcome.additional_context.push(text.trim().to_string());
                }
            }
        }
    } else if !result.stdout.trim().is_empty() {
        outcome
            .messages
            .push(HookMessage::info(result.stdout.trim().to_string()));
    }
}
