use crate::config::ConfigManager;
use crate::config::constants::tools;
use crate::exec::skill_manager::{Skill, SkillMetadata};
use crate::tools::file_tracker::FileTracker;
use crate::tools::native_memory;
use crate::tools::registry::unified_actions::{
    UnifiedExecAction, UnifiedFileAction, UnifiedSearchAction,
};
use crate::tools::tool_intent;
use crate::tools::traits::Tool;

use anyhow::{Context, Result, anyhow, bail};
use chrono;
use futures::future::BoxFuture;
use hashbrown::HashMap;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};

use super::{ExecSettlementMode, ToolRegistry};
use exec_support::*;
use sandbox_runtime::*;

#[cfg(test)]
use cargo_failure_diagnostics::{
    CargoTestCommandKind, attach_exec_recovery_guidance, attach_failure_diagnostics_metadata,
    cargo_selector_error_diagnostics, cargo_test_failure_diagnostics, cargo_test_rerun_hint,
};

mod cargo_failure_diagnostics;
mod exec_sessions;
mod exec_support;
mod patch_pipeline;
mod sandbox_runtime;
mod search_introspection;
mod subagents;

#[derive(Clone, Copy)]
enum ExecRunBackendKind {
    Pty,
    Pipe,
}

struct PreparedExecRunRequest {
    prepared_command: PreparedExecCommand,
    working_dir_path: PathBuf,
    output_config: ExecRunOutputConfig,
    yield_duration: Duration,
    session_id: String,
    shell_program: String,
    env_overrides: HashMap<String, String>,
    is_git_diff: bool,
    confirm: bool,
    rows: Option<u16>,
    cols: Option<u16>,
}

struct ResolvedExecSandboxRequest {
    working_dir_path: PathBuf,
    sandbox_permissions: crate::sandboxing::SandboxPermissions,
    additional_permissions: Option<crate::sandboxing::AdditionalPermissions>,
}

fn set_payload_default(payload: &mut serde_json::Map<String, Value>, key: &str, value: Value) {
    payload.entry(key.to_string()).or_insert(value);
}

fn normalize_unified_exec_run_alias_args(args: &Value, tty: bool) -> Result<Value> {
    let mut args =
        crate::tools::command_args::normalize_shell_args(args).map_err(|error| anyhow!(error))?;
    if let Some(payload) = args.as_object_mut() {
        set_payload_default(payload, "action", json!("run"));
        if tty {
            set_payload_default(payload, "tty", json!(true));
        }
    }
    Ok(args)
}

fn with_unified_exec_action_default(mut args: Value, action: &'static str) -> Value {
    if let Some(payload) = args.as_object_mut() {
        set_payload_default(payload, "action", json!(action));
    }
    args
}

fn annotate_exec_run_response(response: &mut Value, is_git_diff: bool) {
    if is_git_diff {
        response["no_spool"] = json!(true);
        response["content_type"] = json!("git_diff");
    }
}

fn acquire_executor_rate_limit(bucket: &str, multiplier: f64) -> Result<()> {
    let mut guard = crate::tools::rate_limiter::PER_TOOL_RATE_LIMITER
        .lock()
        .map_err(|err| anyhow!("per-tool rate limiter poisoned: {}", err))?;
    guard
        .try_acquire_for_scaled(bucket, multiplier)
        .map_err(|_| anyhow!("tool rate limit exceeded for {}", bucket))
}

fn parse_action<T>(action_str: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(json!(action_str))
        .with_context(|| format!("Invalid action: {}", action_str))
}

impl ToolRegistry {
    pub(super) fn cron_create_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("cron_create requires a non-empty prompt"))?
                .to_string();
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let cron = args.get("cron").and_then(Value::as_str);
            let delay_minutes = args.get("delay_minutes").and_then(Value::as_u64);
            let run_at = args.get("run_at").and_then(Value::as_str);

            let schedule = match (cron, delay_minutes, run_at) {
                (Some(expression), None, None) => {
                    crate::scheduler::ScheduleSpec::cron5(expression)?
                }
                (None, Some(minutes), None) => {
                    crate::scheduler::ScheduleSpec::fixed_interval(Duration::from_secs(
                        minutes
                            .checked_mul(60)
                            .ok_or_else(|| anyhow!("delay_minutes is too large"))?,
                    ))?
                }
                (None, None, Some(raw)) => crate::scheduler::ScheduleSpec::one_shot(
                    crate::scheduler::parse_local_datetime(raw, chrono::Local::now())?,
                ),
                _ => bail!("Choose exactly one of cron, delay_minutes, or run_at"),
            };

            let summary = self
                .create_session_prompt_task(name, prompt, schedule, chrono::Utc::now())
                .await?;
            serde_json::to_value(summary).context("Failed to serialize cron_create response")
        })
    }

    pub(super) fn cron_list_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            Ok(json!({
                "tasks": self.list_session_tasks().await,
            }))
        })
    }

    pub(super) fn cron_delete_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("cron_delete requires id"))?;
            let deleted = self.delete_session_task(id).await;
            Ok(json!({
                "deleted": deleted.is_some(),
                "task": deleted,
            }))
        })
    }

    pub(super) fn memory_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let workspace_root = self.workspace_root_owned();
            let config = ConfigManager::load_from_workspace(&workspace_root)
                .map(|manager| manager.config().clone())
                .unwrap_or_default();
            native_memory::execute_with_vt_config(&workspace_root, &config, args).await
        })
    }

    pub async fn shell_run_approval_reason(
        &self,
        tool_name: &str,
        tool_args: Option<&Value>,
    ) -> Result<Option<String>> {
        let resolved_tool_name = self
            .resolve_public_tool_name_sync(tool_name)
            .unwrap_or_else(|_| tool_name.to_string());
        let Some(payload) = shell_run_payload(&resolved_tool_name, tool_args) else {
            return Ok(None);
        };

        let (requested_command, _) = parse_command_parts(
            payload,
            "shell run request requires a command",
            "shell run request command cannot be empty",
        )?;
        let sandbox_request = self.resolve_exec_sandbox_request(payload).await?;
        let sandbox_config = self.sandbox_config();
        let plan = build_shell_execution_plan(
            &sandbox_config,
            self.workspace_root(),
            &requested_command,
            sandbox_request.sandbox_permissions,
            sandbox_request.additional_permissions.as_ref(),
        )?;

        Ok(plan.approval_reason)
    }

    pub(super) fn unified_exec_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_exec(args).await })
    }

    pub(super) fn unified_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_file(args).await })
    }

    pub(super) fn unified_search_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_search(args).await })
    }

    async fn prepare_exec_run_request(
        &self,
        args: &Value,
        backend: ExecRunBackendKind,
        missing_error: &str,
        empty_error: &str,
    ) -> Result<PreparedExecRunRequest> {
        acquire_executor_rate_limit("unified_exec:run", 2.0)?;

        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("command execution requires a JSON object"))?;

        let (command, auto_raw_command) = parse_command_parts(payload, missing_error, empty_error)?;
        let shell_program = match backend {
            ExecRunBackendKind::Pty => resolve_shell_preference_with_zsh_fork(
                payload.get("shell").and_then(|value| value.as_str()),
                self.pty_config(),
            )?,
            ExecRunBackendKind::Pipe => resolve_shell_preference(
                payload.get("shell").and_then(|value| value.as_str()),
                self.pty_config(),
            ),
        };
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let confirm = payload
            .get("confirm")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let mut prepared_command = prepare_exec_command(
            payload,
            &shell_program,
            login_shell,
            command,
            auto_raw_command,
        );
        let is_git_diff = is_git_diff_command(&prepared_command.requested_command);

        let sandbox_request = self.resolve_exec_sandbox_request(payload).await?;
        let output_config = exec_run_output_config(payload, &prepared_command.display_command);

        enforce_pty_command_policy(&prepared_command.display_command, confirm)?;
        let sandbox_config = self.sandbox_config();
        prepared_command.command = apply_runtime_sandbox_to_command(
            prepared_command.command,
            &prepared_command.requested_command,
            &sandbox_config,
            self.workspace_root(),
            &sandbox_request.working_dir_path,
            sandbox_request.sandbox_permissions,
            sandbox_request.additional_permissions.as_ref(),
        )?;

        let rows = match backend {
            ExecRunBackendKind::Pty => Some(parse_pty_dimension(
                "rows",
                payload.get("rows"),
                self.pty_config().default_rows,
            )?),
            ExecRunBackendKind::Pipe => None,
        };
        let cols = match backend {
            ExecRunBackendKind::Pty => Some(parse_pty_dimension(
                "cols",
                payload.get("cols"),
                self.pty_config().default_cols,
            )?),
            ExecRunBackendKind::Pipe => None,
        };

        Ok(PreparedExecRunRequest {
            prepared_command,
            working_dir_path: sandbox_request.working_dir_path,
            output_config,
            yield_duration: Duration::from_millis(clamp_exec_yield_ms(
                payload.get("yield_time_ms").and_then(Value::as_u64),
                10_000,
            )),
            session_id: resolve_exec_run_session_id(payload)?,
            shell_program,
            env_overrides: parse_exec_env_overrides(payload)?,
            is_git_diff,
            confirm,
            rows,
            cols,
        })
    }

    pub(super) async fn execute_unified_exec(&self, args: Value) -> Result<Value> {
        self.execute_unified_exec_internal(args, ExecSettlementMode::Manual)
            .await
    }

    pub(super) async fn execute_harness_unified_exec_terminal_run_raw(
        &self,
        args: Value,
    ) -> Result<Value> {
        let args = normalize_unified_exec_run_alias_args(&args, true)?;
        self.execute_command_session_run_pty(args, true).await
    }

    fn dispatch_unified_exec_alias(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    fn dispatch_unified_exec_run_alias(
        &self,
        args: Value,
        tty: bool,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let args = normalize_unified_exec_run_alias_args(&args, tty)?;
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    fn dispatch_unified_exec_action_alias(
        &self,
        args: Value,
        action: &'static str,
    ) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_alias(with_unified_exec_action_default(args, action))
    }

    pub(super) async fn execute_unified_exec_internal(
        &self,
        args: Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let args = crate::tools::command_args::normalize_shell_args(&args)
            .map_err(|error| anyhow!(error))?;

        let action_str = tool_intent::unified_exec_action(&args)
            .ok_or_else(|| missing_unified_exec_action_error(&args))?;
        let action: UnifiedExecAction = parse_action(action_str)?;

        match action {
            UnifiedExecAction::Run => {
                self.execute_command_session_run_internal(args, exec_settlement_mode)
                    .await
            }
            UnifiedExecAction::Write => self.execute_command_session_write(args).await,
            UnifiedExecAction::Poll => {
                self.execute_command_session_poll_internal(args, exec_settlement_mode)
                    .await
            }
            UnifiedExecAction::Continue => {
                self.execute_command_session_continue_internal(args, exec_settlement_mode)
                    .await
            }
            UnifiedExecAction::Inspect => self.execute_command_session_inspect(args).await,
            UnifiedExecAction::List => self.execute_command_session_list().await,
            UnifiedExecAction::Close => self.execute_command_session_close(args).await,
            UnifiedExecAction::Code => self.execute_code(args).await,
        }
    }

    async fn execute_command_session_run_internal(
        &self,
        args: Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let tty = args.get("tty").and_then(Value::as_bool).unwrap_or(false);
        if tty {
            self.execute_command_session_run_pty(args, false).await
        } else {
            self.execute_run_pipe_cmd(args, exec_settlement_mode).await
        }
    }

    pub(super) async fn execute_unified_file(&self, args: Value) -> Result<Value> {
        let action_str = tool_intent::unified_file_action(&args)
            .ok_or_else(|| missing_unified_file_action_error(&args))?;

        let action: UnifiedFileAction = parse_action(action_str)?;
        self.log_unified_file_payload_diagnostics(action_str, &args);
        let tool = self.inventory.file_ops_tool().clone();

        match action {
            UnifiedFileAction::Read => {
                self.execute_unified_file_read_with_recovery(&tool, args)
                    .await
            }
            UnifiedFileAction::Write => tool.write_file(args).await,
            UnifiedFileAction::Edit => self.edit_file(args).await,
            UnifiedFileAction::Patch => self.execute_apply_patch(args).await,
            UnifiedFileAction::Delete => tool.delete_file(args).await,
            UnifiedFileAction::Move => tool.move_file(args).await,
            UnifiedFileAction::Copy => tool.copy_file(args).await,
        }
    }

    async fn execute_unified_file_read_with_recovery(
        &self,
        tool: &crate::tools::file_ops::FileOpsTool,
        args: Value,
    ) -> Result<Value> {
        match tool.read_file(args.clone()).await {
            Ok(response) => Ok(response),
            Err(read_err) => {
                let read_err_text = read_err.to_string();
                if let Some(fallback_args) = build_read_pty_fallback_args(&args, &read_err_text) {
                    let session_id = fallback_args
                        .get("session_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    tracing::info!(
                        session_id = %session_id,
                        "Auto-recovering unified_file read via unified_exec poll"
                    );
                    match self.execute_command_session_poll(fallback_args).await {
                        Ok(mut recovered) => {
                            if let Some(obj) = recovered.as_object_mut() {
                                obj.insert("auto_recovered".to_string(), json!(true));
                                obj.insert("recovery_tool".to_string(), json!(tools::UNIFIED_EXEC));
                                obj.insert("recovery_action".to_string(), json!("poll"));
                                obj.insert(
                                    "recovery_reason".to_string(),
                                    json!("missing_pty_spool_file"),
                                );
                            }
                            return Ok(recovered);
                        }
                        Err(recovery_err) => {
                            tracing::warn!(
                                session_id = %session_id,
                                error = %recovery_err,
                                "Failed auto-recovery via unified_exec poll"
                            );
                        }
                    }
                }
                Err(read_err)
            }
        }
    }

    pub(super) async fn execute_unified_search(&self, args: Value) -> Result<Value> {
        let mut args = tool_intent::normalize_unified_search_args(&args);

        let action_str = tool_intent::unified_search_action(&args)
            .ok_or_else(|| missing_unified_search_action_error(&args))?;

        let action: UnifiedSearchAction = parse_action(action_str)?;

        // Default to workspace root when path is omitted for list/grep actions to reduce friction
        if matches!(
            action,
            UnifiedSearchAction::Grep | UnifiedSearchAction::List
        ) {
            let has_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false);
            if !has_path {
                args["path"] = json!(".");
            }
        }

        match action {
            UnifiedSearchAction::Grep => {
                let manager = self.inventory.grep_file_manager();
                manager
                    .perform_search(serde_json::from_value(args)?)
                    .await
                    .map(|r| json!(r))
            }
            UnifiedSearchAction::List => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.execute(args).await
            }
            UnifiedSearchAction::Structural => {
                crate::tools::structural_search::execute_structural_search(
                    self.workspace_root(),
                    args,
                )
                .await
            }
            UnifiedSearchAction::Intelligence => Ok(
                serde_json::json!({"error": "Action 'intelligence' is deprecated. Use action='grep' or action='list'."}),
            ),
            UnifiedSearchAction::Tools => self.execute_search_tools(args).await,
            UnifiedSearchAction::Errors => self.execute_get_errors(args).await,
            UnifiedSearchAction::Agent => self.execute_agent_info().await,
            UnifiedSearchAction::Web => self.execute_web_fetch(args).await,
            UnifiedSearchAction::Skill => self.execute_skill(args).await,
        }
    }

    pub(super) async fn execute_code(&self, args: Value) -> Result<Value> {
        let code = args
            .get("command")
            .or_else(|| args.get("code"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing code/command in execute_code"))?;

        let language = code_language_from_args(&args);

        let track_files = args
            .get("track_files")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;

        let workspace_root = self.workspace_root_owned();
        let executor = crate::exec::code_executor::CodeExecutor::new(
            language,
            mcp_client.clone(),
            workspace_root.clone(),
        );
        let execution_start = SystemTime::now();

        let result = executor.execute(code).await?;

        let mut response = json!(result);

        if track_files {
            let tracker = FileTracker::new(workspace_root);
            if let Ok(changes) = tracker.detect_new_files(execution_start).await {
                response["generated_files"] = json!({
                    "count": changes.len(),
                    "files": changes,
                    "summary": tracker.generate_file_summary(&changes),
                });
            }
        }

        Ok(response)
    }

    pub(super) async fn execute_web_fetch(&self, args: Value) -> Result<Value> {
        acquire_executor_rate_limit("unified_search:web", 1.0)?;

        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing url in web_fetch"))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("VT Code/1.0")
            .build()?;

        let response = client.get(url).send().await?;
        let status = response.status();

        if !status.is_success() {
            return Err(anyhow!("Web fetch failed with status: {}", status));
        }

        let body = response.text().await?;
        Ok(json!({ "success": true, "content": body, "url": url }))
    }

    pub(super) async fn execute_skill(&self, args: Value) -> Result<Value> {
        let sub_action = args
            .get("sub_action")
            .and_then(|v| v.as_str())
            .or_else(|| {
                if args.get("name").is_some() {
                    Some("load")
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("Missing sub_action in skill"))?;

        let skill_manager = self.inventory.skill_manager();

        match sub_action {
            "save" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing name in skill save"))?;
                let code = args
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing code in skill save"))?;
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let language = args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python3");

                let metadata = SkillMetadata {
                    name: name.to_string(),
                    description: description.to_string(),
                    language: language.to_string(),
                    inputs: vec![],
                    output: "".to_string(),
                    examples: vec![],
                    tags: vec![],
                    created_at: chrono::Utc::now().to_rfc3339(),
                    modified_at: chrono::Utc::now().to_rfc3339(),
                    tool_dependencies: vec![],
                };

                let skill = Skill {
                    metadata,
                    code: code.to_string(),
                };

                skill_manager.save_skill(skill).await?;
                Ok(json!({ "success": true, "name": name }))
            }
            "load" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing name in skill load"))?;
                let skill = skill_manager.load_skill(name).await?;
                Ok(json!({
                    "success": true,
                    "name": skill.metadata.name,
                    "code": skill.code,
                    "language": skill.metadata.language
                }))
            }
            "list" => {
                let skills = skill_manager.list_skills().await?;
                Ok(json!({ "success": true, "skills": skills }))
            }
            _ => Err(anyhow!("Unknown skill sub_action: {}", sub_action)),
        }
    }

    pub(super) async fn execute_apply_patch(&self, args: Value) -> Result<Value> {
        let (patch_args, patch_input_bytes, patch_base64) = self.prepare_apply_patch_args(args)?;
        let context = self.harness_context_snapshot();
        tracing::debug!(
            tool = tools::UNIFIED_FILE,
            action = "patch",
            payload_bytes = serialized_payload_size_bytes(&patch_args),
            patch_input_bytes,
            patch_base64,
            patch_decoded_bytes = patch_args
                .get("input")
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0),
            session_id = %context.session_id,
            task_id = %context.task_id.as_deref().unwrap_or(""),
            "Prepared patch payload for apply_patch"
        );

        self.execute_apply_patch_internal(patch_args).await
    }

    fn prepare_apply_patch_args(&self, args: Value) -> Result<(Value, usize, bool)> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input"))?;
        let patch_input_bytes = patch_input.source_bytes;
        let patch_base64 = patch_input.was_base64;

        let mut patch_args = args;
        patch_args["input"] = json!(patch_input.text);
        Ok((patch_args, patch_input_bytes, patch_base64))
    }

    fn log_unified_file_payload_diagnostics(&self, action: &str, args: &Value) {
        let context = self.harness_context_snapshot();
        let (patch_source_bytes, patch_base64) =
            crate::tools::apply_patch::patch_source_from_args(args)
                .map(|source| (source.len(), source.starts_with("base64:")))
                .unwrap_or((0, false));

        tracing::trace!(
            tool = tools::UNIFIED_FILE,
            action,
            payload_bytes = serialized_payload_size_bytes(args),
            patch_source_bytes,
            patch_base64,
            session_id = %context.session_id,
            task_id = %context.task_id.as_deref().unwrap_or(""),
            "Captured unified_file payload diagnostics"
        );
    }

    async fn resolve_exec_sandbox_request(
        &self,
        payload: &serde_json::Map<String, Value>,
    ) -> Result<ResolvedExecSandboxRequest> {
        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(shell_working_dir_value(payload))
            .await?;
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(payload, &working_dir_path)?;

        Ok(ResolvedExecSandboxRequest {
            working_dir_path,
            sandbox_permissions,
            additional_permissions,
        })
    }

    // ============================================================
    // SPECIALIZED EXECUTORS (Hidden from LLM, used by unified tools)
    // ============================================================

    pub(super) fn read_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn list_files_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.list_files(args).await })
    }

    pub(super) fn write_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn edit_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn run_pty_cmd_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_run_alias(args, true)
    }

    pub(super) fn send_pty_input_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_action_alias(args, "write")
    }

    pub(super) fn read_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_action_alias(args, "poll")
    }

    pub(super) fn create_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_run_alias(args, true)
    }

    pub(super) fn list_pty_sessions_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_alias(json!({"action": "list"}))
    }

    pub(super) fn close_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_unified_exec_action_alias(args, "close")
    }

    pub(super) fn get_errors_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_get_errors(args).await })
    }

    pub(super) fn apply_patch_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    // ============================================================
    // INTERNAL IMPLEMENTATIONS
    // ============================================================
}

#[cfg(test)]
mod execute_code_tests {
    use super::code_language_from_args;
    use crate::exec::code_executor::Language;
    use serde_json::json;

    #[test]
    fn code_language_uses_language_field_instead_of_action() {
        assert_eq!(
            code_language_from_args(&json!({
                "action": "code",
                "language": "javascript",
            })),
            Language::JavaScript
        );
        assert_eq!(
            code_language_from_args(&json!({
                "action": "code",
                "lang": "js",
            })),
            Language::JavaScript
        );
        assert_eq!(
            code_language_from_args(&json!({
                "action": "code",
            })),
            Language::Python3
        );
    }
}

#[cfg(test)]
mod subagent_tool_output_tests {
    use super::sanitize_subagent_tool_output_paths;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn strips_transcript_paths_outside_workspace() {
        let temp = TempDir::new().expect("tempdir");
        let mut value = json!({
            "completed": true,
            "entry": {
                "id": "agent-1",
                "transcript_path": "/Users/example/.vtcode/sessions/agent-1.json",
            }
        });

        sanitize_subagent_tool_output_paths(temp.path(), &mut value);

        assert!(value["entry"].get("transcript_path").is_none());
    }

    #[test]
    fn keeps_transcript_paths_inside_workspace() {
        let temp = TempDir::new().expect("tempdir");
        let transcript_path = temp.path().join(".vtcode/context/subagents/agent-1.json");
        let mut value = json!({
            "id": "agent-1",
            "transcript_path": transcript_path,
        });

        sanitize_subagent_tool_output_paths(temp.path(), &mut value);

        assert_eq!(value["transcript_path"].as_str(), transcript_path.to_str());
    }
}

#[cfg(test)]
mod shell_preference_tests {
    use super::{resolve_shell_preference, resolve_shell_preference_with_zsh_fork};
    use crate::config::PtyConfig;
    use crate::tools::shell::resolve_fallback_shell;

    #[test]
    fn explicit_shell_overrides_config_preference() {
        let config = PtyConfig {
            preferred_shell: Some("/bin/bash".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(Some(" /bin/zsh "), &config);
        assert_eq!(resolved, "/bin/zsh");
    }

    #[test]
    fn config_preferred_shell_used_when_explicit_missing() {
        let config = PtyConfig {
            preferred_shell: Some("zsh".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, "zsh");
    }

    #[test]
    fn blank_explicit_shell_falls_back_to_config_preference() {
        let config = PtyConfig {
            preferred_shell: Some("bash".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(Some("   "), &config);
        assert_eq!(resolved, "bash");
    }

    #[test]
    fn blank_config_shell_falls_back_to_default_resolver() {
        let config = PtyConfig {
            preferred_shell: Some("   ".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, resolve_fallback_shell());
    }

    #[test]
    fn missing_preferences_fall_back_to_default_resolver() {
        let config = PtyConfig::default();
        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, resolve_fallback_shell());
    }

    #[test]
    fn zsh_fork_disabled_uses_standard_shell_resolution() -> anyhow::Result<()> {
        let config = PtyConfig {
            preferred_shell: Some("/bin/bash".to_string()),
            ..Default::default()
        };
        let resolved = resolve_shell_preference_with_zsh_fork(None, &config)?;
        assert_eq!(resolved, "/bin/bash");
        Ok(())
    }

    #[test]
    fn zsh_fork_missing_path_returns_error() {
        let config = PtyConfig {
            shell_zsh_fork: true,
            zsh_path: None,
            ..PtyConfig::default()
        };
        assert!(resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn zsh_fork_ignores_explicit_shell_and_uses_configured_path() -> anyhow::Result<()> {
        let zsh = tempfile::NamedTempFile::new()?;
        let expected = zsh.path().to_string_lossy().to_string();
        let config = PtyConfig {
            shell_zsh_fork: true,
            zsh_path: Some(expected.clone()),
            ..PtyConfig::default()
        };
        let resolved = resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config)?;
        assert_eq!(resolved, expected);
        Ok(())
    }
}

#[cfg(test)]
mod token_efficiency_tests {
    use super::*;

    #[test]
    fn test_suggests_limit_for_cat() {
        assert_eq!(suggest_max_tokens_for_command("cat file.txt"), Some(250));
        assert_eq!(
            suggest_max_tokens_for_command("cat /path/to/file.rs"),
            Some(250)
        );
        assert_eq!(suggest_max_tokens_for_command("CAT file.txt"), Some(250)); // case insensitive
    }

    #[test]
    fn test_suggests_limit_for_bat() {
        assert_eq!(suggest_max_tokens_for_command("bat file.rs"), Some(250));
    }

    #[test]
    fn test_no_limit_when_already_limited() {
        assert_eq!(suggest_max_tokens_for_command("cat file.txt | head"), None);
        assert_eq!(suggest_max_tokens_for_command("head -n 50 file.txt"), None);
        assert_eq!(suggest_max_tokens_for_command("tail -n 20 file.txt"), None);
    }

    #[test]
    fn test_no_limit_for_other_commands() {
        assert_eq!(suggest_max_tokens_for_command("ls -la"), None);
        assert_eq!(suggest_max_tokens_for_command("grep pattern file"), None);
        assert_eq!(suggest_max_tokens_for_command("echo hello"), None);
    }
}

#[cfg(test)]
mod pty_output_filter_tests {
    use super::filter_pty_output;

    #[test]
    fn normalizes_crlf_sequences() {
        let raw = "a\r\nb\rc\n";
        assert_eq!(filter_pty_output(raw), "a\nb\nc\n");
    }
}

#[cfg(test)]
mod pty_context_tests {
    use super::{
        ExecOutputPreview, PtyEphemeralCapture, attach_exec_response_context,
        attach_pty_continuation, build_exec_response, build_exec_session_command_display,
    };
    use crate::tools::types::VTCodeExecSession;
    use serde_json::json;

    #[test]
    fn build_exec_session_command_display_unwraps_shell_c_argument() {
        let session = VTCodeExecSession {
            id: "run-123".to_string().into(),
            backend: "pty".to_string(),
            command: "zsh".to_string(),
            args: vec![
                "-l".to_string(),
                "-c".to_string(),
                "cargo check".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: Some(24),
            cols: Some(80),
            child_pid: None,
            started_at: None,
            lifecycle_state: None,
            exit_code: None,
        };

        assert_eq!(build_exec_session_command_display(&session), "cargo check");
    }

    #[test]
    fn attach_exec_response_context_sets_expected_keys() {
        let mut response = json!({ "output": "ok" });
        let session = VTCodeExecSession {
            id: "run-123".to_string().into(),
            backend: "pty".to_string(),
            command: "zsh".to_string(),
            args: vec![
                "-l".to_string(),
                "-c".to_string(),
                "cargo check".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: Some(30),
            cols: Some(120),
            child_pid: None,
            started_at: None,
            lifecycle_state: None,
            exit_code: None,
        };

        attach_exec_response_context(&mut response, &session, "cargo check", false);

        assert_eq!(response["session_id"], "run-123");
        assert_eq!(response["command"], "cargo check");
        assert_eq!(response["working_directory"], ".");
        assert_eq!(response["backend"], "pty");
        assert_eq!(response["rows"], 30);
        assert_eq!(response["cols"], 120);
        assert_eq!(response["is_exited"], false);
    }

    #[test]
    fn attach_pty_continuation_compacts_next_continue_args() {
        let mut response = json!({ "output": "ok" });
        attach_pty_continuation(&mut response, "run-123");

        assert!(response.get("follow_up_prompt").is_none());
        assert!(response.get("next_poll_args").is_none());
        assert_eq!(
            response["next_continue_args"],
            json!({ "session_id": "run-123" })
        );
        assert!(response.get("preferred_next_action").is_none());
    }

    #[test]
    fn attach_pty_continuation_keeps_payload_compact() {
        let mut response = json!({ "output": "ok" });
        attach_pty_continuation(&mut response, "run-123");

        assert!(response.get("follow_up_prompt").is_none());
        assert!(response.get("next_poll_args").is_none());
        assert_eq!(
            response["next_continue_args"],
            json!({ "session_id": "run-123" })
        );
    }

    #[test]
    fn build_exec_response_skips_continuation_after_exit() {
        let session = VTCodeExecSession {
            id: "run-123".to_string().into(),
            backend: "pipe".to_string(),
            command: "cargo".to_string(),
            args: vec!["check".to_string()],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
            child_pid: None,
            started_at: None,
            lifecycle_state: None,
            exit_code: None,
        };
        let capture = PtyEphemeralCapture {
            output: "first\nsecond\n".to_string(),
            exit_code: Some(0),
            duration: std::time::Duration::from_millis(25),
        };

        let response = build_exec_response(
            &session,
            "cargo check",
            &capture,
            ExecOutputPreview {
                raw_output: "first\nsecond\n".to_string(),
                output: "first\n[Output truncated]".to_string(),
                truncated: true,
            },
            None,
            false,
            None,
        );

        assert_eq!(response["exit_code"], 0);
        assert!(response.get("next_continue_args").is_none());
    }
}

#[cfg(test)]
mod git_diff_tests {
    use super::is_git_diff_command;

    #[test]
    fn detects_git_diff() {
        let cmd = vec!["git".to_string(), "diff".to_string()];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn detects_git_diff_with_flags() {
        let cmd = vec![
            "git".to_string(),
            "-c".to_string(),
            "color.ui=always".to_string(),
            "diff".to_string(),
            "--stat".to_string(),
        ];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn detects_git_diff_with_path() {
        let cmd = vec!["/usr/bin/git".to_string(), "diff".to_string()];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn ignores_other_git_commands() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!is_git_diff_command(&cmd));
    }
}

#[cfg(test)]
mod unified_action_error_tests {
    use super::{
        CargoTestCommandKind, ExecOutputPreview, PtyEphemeralCapture,
        attach_exec_recovery_guidance, attach_failure_diagnostics_metadata,
        build_exec_output_preview, build_exec_response, build_head_tail_preview,
        cargo_selector_error_diagnostics, cargo_test_failure_diagnostics, cargo_test_rerun_hint,
        clamp_inspect_lines, clamp_max_matches, extract_run_session_id_from_read_file_error,
        extract_run_session_id_from_tool_output_path, filter_lines,
        missing_unified_exec_action_error, missing_unified_search_action_error,
        resolve_exec_run_session_id, summarized_arg_keys,
    };
    use crate::tools::types::VTCodeExecSession;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn summarized_arg_keys_reports_shape_for_non_object_payloads() {
        assert_eq!(summarized_arg_keys(&json!(null)), "<null>");
        assert_eq!(summarized_arg_keys(&json!(["a", "b"])), "<array>");
        assert_eq!(summarized_arg_keys(&json!("x")), "<string>");
    }

    #[test]
    fn unified_exec_missing_action_error_includes_received_keys() {
        let err = missing_unified_exec_action_error(&json!({
            "foo": "bar",
            "session_id": "123"
        }));
        let text = err.to_string();
        assert!(text.contains("Missing unified_exec action"));
        assert!(text.contains("foo"));
        assert!(text.contains("session_id"));
    }

    #[test]
    fn unified_search_missing_action_error_includes_received_keys() {
        let err = missing_unified_search_action_error(&json!({
            "unexpected": true
        }));
        let text = err.to_string();
        assert!(text.contains("Missing unified_search action"));
        assert!(text.contains("unexpected"));
    }

    #[test]
    fn extracts_run_session_id_from_tool_output_path() {
        assert_eq!(
            extract_run_session_id_from_tool_output_path(
                ".vtcode/context/tool_outputs/run-abc123.txt"
            ),
            Some("run-abc123".to_string())
        );
        assert_eq!(
            extract_run_session_id_from_tool_output_path(
                ".vtcode/context/tool_outputs/not-a-session.txt"
            ),
            None
        );
    }

    #[test]
    fn extracts_run_session_id_from_read_file_error() {
        let error = "Use unified_exec with session_id=\"run-zz9\" instead of read_file.";
        assert_eq!(
            extract_run_session_id_from_read_file_error(error),
            Some("run-zz9".to_string())
        );
        assert_eq!(
            extract_run_session_id_from_read_file_error("no session"),
            None
        );
    }

    #[test]
    fn resolve_exec_run_session_id_prefers_requested_session_id() {
        let payload = json!({ "session_id": " check_sh " });
        let payload = payload.as_object().expect("object");

        assert_eq!(
            resolve_exec_run_session_id(payload).expect("requested session id"),
            "check_sh"
        );
    }

    #[test]
    fn resolve_exec_run_session_id_generates_default_when_missing() {
        let payload = json!({});
        let payload = payload.as_object().expect("object");
        let session_id = resolve_exec_run_session_id(payload).expect("generated session id");

        assert!(session_id.starts_with("run-"));
    }

    #[test]
    fn resolve_exec_run_session_id_rejects_invalid_values() {
        let payload = json!({ "session_id": "bad id" });
        let payload = payload.as_object().expect("object");
        let err = resolve_exec_run_session_id(payload).expect_err("invalid session id");

        assert!(err.to_string().contains("Invalid session_id"));
    }

    #[test]
    fn inspect_helpers_clamp_limits() {
        assert_eq!(clamp_inspect_lines(Some(0), 30), 0);
        assert_eq!(clamp_inspect_lines(Some(9_999), 30), 5_000);
        assert_eq!(clamp_max_matches(None), 200);
        assert_eq!(clamp_max_matches(Some(0)), 1);
        assert_eq!(clamp_max_matches(Some(50_000)), 10_000);
    }

    #[test]
    fn inspect_helpers_build_head_tail_preview() {
        let content = "l1\nl2\nl3\nl4\nl5\nl6";
        let (preview, truncated) = build_head_tail_preview(content, 2, 2);
        assert!(truncated);
        assert!(preview.contains("l1"));
        assert!(preview.contains("l2"));
        assert!(preview.contains("l5"));
        assert!(preview.contains("l6"));
    }

    #[test]
    fn inspect_helpers_filter_lines_literal() {
        let (output, matched, truncated) =
            filter_lines("alpha\nbeta\nalpha2", "alpha", true, 1).expect("filter");
        assert_eq!(matched, 2);
        assert!(truncated);
        assert!(output.contains("1: alpha"));
    }

    #[test]
    fn exec_output_preview_truncates_on_utf8_boundaries() {
        let preview = build_exec_output_preview("a🙂b".to_string(), 1);

        assert!(preview.truncated);
        assert_eq!(preview.raw_output, "a🙂b");
        assert_eq!(preview.output, "a\n[Output truncated]");
        assert!(std::str::from_utf8(preview.output.as_bytes()).is_ok());
    }

    #[test]
    fn exec_recovery_guidance_sets_command_not_found_metadata() {
        let session = VTCodeExecSession {
            id: "run-123".to_string().into(),
            backend: "pipe".to_string(),
            command: "zsh".to_string(),
            args: vec!["-c".to_string(), "pip install pymupdf".to_string()],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
            child_pid: None,
            started_at: None,
            lifecycle_state: None,
            exit_code: None,
        };
        let capture = PtyEphemeralCapture {
            output: String::new(),
            exit_code: Some(127),
            duration: Duration::from_millis(42),
        };

        let response = build_exec_response(
            &session,
            "pip install pymupdf",
            &capture,
            ExecOutputPreview {
                raw_output: "bash: pip: command not found".to_string(),
                output: "bash: pip: command not found".to_string(),
                truncated: false,
            },
            None,
            false,
            None,
        );

        assert_eq!(response["output"], "bash: pip: command not found");
        assert_eq!(response["exit_code"], 127);
        assert_eq!(response["session_id"], "run-123");
        assert_eq!(response["command"], "pip install pymupdf");
        assert_eq!(
            response["critical_note"],
            "Command `pip` was not found in PATH."
        );
        assert_eq!(
            response["next_action"],
            "Check the command name or install the missing binary, then rerun the command."
        );
    }

    #[test]
    fn exec_recovery_guidance_ignores_non_command_not_found_exit_codes() {
        let mut response = json!({});
        attach_exec_recovery_guidance(&mut response, "cargo test", Some(1));
        assert!(response.get("critical_note").is_none());
        assert!(response.get("next_action").is_none());
    }

    #[test]
    fn cargo_selector_error_diagnostics_classifies_missing_test_target() {
        let output = "error: no test target named `exec_only_policy_skips_when_full_auto_is_disabled` in `vtcode-core` package\n";

        let diagnostics = cargo_selector_error_diagnostics(
            CargoTestCommandKind::Nextest,
            "cargo nextest run --test exec_only_policy_skips_when_full_auto_is_disabled -p vtcode-core --no-capture",
            output,
        )
        .expect("selector diagnostics");

        assert_eq!(diagnostics["kind"], "cargo_test_selector_error");
        assert_eq!(diagnostics["package"], "vtcode-core");
        assert_eq!(
            diagnostics["requested_test_target"],
            "exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(diagnostics["selector_error"], true);
        assert_eq!(
            diagnostics["validation_hint"],
            "cargo test -p vtcode-core --lib -- --list | rg 'exec_only_policy_skips_when_full_auto_is_disabled'"
        );
        assert_eq!(
            diagnostics["rerun_hint"],
            "cargo nextest run -p vtcode-core exec_only_policy_skips_when_full_auto_is_disabled"
        );
    }

    #[test]
    fn cargo_test_failure_diagnostics_extracts_unit_test_failure_details() {
        let output = r#"────────────
    Nextest run ID 18fffe01-0ef9-4113-9a81-2344a7cc3c16 with nextest profile: default
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    stderr ───
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;

        let diagnostics =
            cargo_test_failure_diagnostics("cargo nextest run -p vtcode-core", output, Some(100))
                .expect("failure diagnostics");

        assert_eq!(diagnostics["kind"], "cargo_test_failure");
        assert_eq!(diagnostics["package"], "vtcode-core");
        assert_eq!(diagnostics["binary_kind"], "unit");
        assert_eq!(
            diagnostics["test_fqname"],
            "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(
            diagnostics["panic"],
            "task result: Invalid request: QueuedProvider has no queued responses"
        );
        assert_eq!(
            diagnostics["source_file"],
            "vtcode-core/src/core/agent/runner/tests.rs"
        );
        assert_eq!(diagnostics["source_line"], 692);
        assert_eq!(
            diagnostics["rerun_hint"],
            cargo_test_rerun_hint(
                CargoTestCommandKind::Nextest,
                "vtcode-core",
                "unit",
                "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled",
            )
        );
    }

    #[test]
    fn build_exec_response_attaches_cargo_failure_diagnostics() {
        let session = VTCodeExecSession {
            id: "run-123".to_string().into(),
            backend: "pipe".to_string(),
            command: "cargo".to_string(),
            args: vec![
                "nextest".to_string(),
                "run".to_string(),
                "-p".to_string(),
                "vtcode-core".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
            child_pid: None,
            started_at: None,
            lifecycle_state: None,
            exit_code: None,
        };
        let raw_output = r#"
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;
        let capture = PtyEphemeralCapture {
            output: raw_output.to_string(),
            exit_code: Some(100),
            duration: Duration::from_millis(42),
        };

        let response = build_exec_response(
            &session,
            "cargo nextest run -p vtcode-core",
            &capture,
            ExecOutputPreview {
                raw_output: raw_output.to_string(),
                output: raw_output.to_string(),
                truncated: false,
            },
            None,
            false,
            None,
        );

        assert_eq!(
            response["failure_diagnostics"]["test_fqname"],
            "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(response["package"], "vtcode-core");
        assert_eq!(response["binary_kind"], "unit");
        assert_eq!(
            response["source_file"],
            "vtcode-core/src/core/agent/runner/tests.rs"
        );
        assert_eq!(response["source_line"], 692);
        assert_eq!(
            response["rerun_hint"],
            "cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(
            response["next_action"],
            "Rerun the failing test directly with: cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
    }

    #[test]
    fn attach_failure_diagnostics_metadata_promotes_selector_hints() {
        let mut response = json!({
            "success": true,
            "command": "cargo nextest run --test bad -p vtcode-core"
        });
        let diagnostics = json!({
            "kind": "cargo_test_selector_error",
            "package": "vtcode-core",
            "binary_kind": "test_target_selector",
            "requested_test_target": "bad",
            "selector_error": true,
            "validation_hint": "cargo test -p vtcode-core --lib -- --list | rg 'bad'",
            "rerun_hint": "cargo nextest run -p vtcode-core bad",
            "critical_note": "selector mismatch",
            "next_action": "validate first"
        });

        attach_failure_diagnostics_metadata(&mut response, &diagnostics);

        assert_eq!(response["package"], "vtcode-core");
        assert_eq!(response["binary_kind"], "test_target_selector");
        assert_eq!(response["selector_error"], true);
        assert_eq!(
            response["validation_hint"],
            "cargo test -p vtcode-core --lib -- --list | rg 'bad'"
        );
        assert_eq!(
            response["rerun_hint"],
            "cargo nextest run -p vtcode-core bad"
        );
        assert_eq!(response["critical_note"], "selector mismatch");
        assert_eq!(response["next_action"], "validate first");
        assert_eq!(
            response["failure_diagnostics"]["kind"],
            "cargo_test_selector_error"
        );
    }
}

#[cfg(test)]
#[path = "executors/sandbox_runtime_tests.rs"]
mod sandbox_runtime_tests;
