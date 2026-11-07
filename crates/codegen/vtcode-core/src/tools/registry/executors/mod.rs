use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow, bail};
#[cfg(test)]
use cargo_failure_diagnostics::{
    CargoTestCommandKind, attach_exec_recovery_guidance, attach_failure_diagnostics_metadata,
    cargo_selector_error_diagnostics, cargo_test_failure_diagnostics, cargo_test_rerun_hint,
};
use chrono;
use exec_support::*;
use futures::future::BoxFuture;
use hashbrown::HashMap;
use sandbox_runtime::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{ExecSettlementMode, ToolRegistry};
use crate::config::constants::tools;
use crate::tools::exec_session::MonitorSpec;
use crate::tools::file_tracker::FileTracker;
use crate::tools::registry::unified_actions::CommandSessionAction;
use crate::tools::{native_memory, tool_intent};

mod cargo_failure_diagnostics;
mod exec_command;
mod exec_output;
mod exec_sessions;
mod exec_support;
mod matrix;
mod patch_pipeline;
mod sandbox_runtime;
mod search_introspection;
mod session_messaging;
mod subagents;

pub use sandbox_runtime::sandbox_policy_from_runtime_config;

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
    sandbox_active: bool,
    background: bool,
    stdin: bool,
    monitor: Option<MonitorSpec>,
}

struct ResolvedExecSandboxRequest {
    working_dir_path: PathBuf,
    sandbox_permissions: crate::sandboxing::SandboxPermissions,
    additional_permissions: Option<crate::sandboxing::AdditionalPermissions>,
}

fn set_payload_default(payload: &mut serde_json::Map<String, Value>, key: &str, value: Value) {
    payload.entry(key.to_string()).or_insert(value);
}

pub(super) fn normalize_command_session_run_alias_args(args: &Value, tty: bool) -> Result<Value> {
    let mut args = crate::tools::command_args::normalize_shell_args(args).map_err(|error| anyhow!(error))?;
    if let Some(payload) = args.as_object_mut() {
        set_payload_default(payload, "action", json!("run"));
        if tty {
            set_payload_default(payload, "tty", json!(true));
        }
    }
    Ok(args)
}

fn with_command_session_action_default(mut args: Value, action: &'static str) -> Value {
    if let Some(payload) = args.as_object_mut() {
        set_payload_default(payload, "action", json!(action));
    }
    args
}

pub(super) fn normalize_write_stdin_args(
    args: &Value,
) -> Result<(Value, crate::tools::command_args::WriteStdinDispatch)> {
    let dispatch = crate::tools::command_args::write_stdin_dispatch(args).map_err(|error| anyhow!(error))?;
    let mut args = crate::tools::command_args::normalize_shell_args(args).map_err(|error| anyhow!(error))?;
    let payload = args
        .as_object_mut()
        .ok_or_else(|| anyhow!("write_stdin requires a JSON object"))?;
    payload.insert("action".to_string(), json!(dispatch.command_session_action()));
    if dispatch == crate::tools::command_args::WriteStdinDispatch::Poll {
        payload.remove("input");
    }
    Ok((args, dispatch))
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
        .map_err(|err| anyhow!("per-tool rate limiter poisoned: {err}"))?;
    guard
        .try_acquire_for_scaled(bucket, multiplier)
        .map_err(|e| anyhow!("tool rate limit exceeded for {bucket}: {e}"))
}

fn parse_action<T>(action_str: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(json!(action_str)).with_context(|| format!("Invalid action: {action_str}"))
}

/// Generate an executor that delegates to a cloned tool instance from the inventory.
macro_rules! delegate_to_tool {
    ($name:ident, $tool_accessor:ident, $method:ident) => {
        pub(super) fn $name(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
            let tool = self.inventory.$tool_accessor().clone();
            Box::pin(async move { tool.$method(args).await })
        }
    };
}

/// Generate an executor that delegates to an async method on `self`.
macro_rules! delegate_to_self {
    ($name:ident, $method:ident) => {
        pub(super) fn $name(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
            Box::pin(async move { self.$method(args).await })
        }
    };
}

impl ToolRegistry {
    pub(super) fn record_decision_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let decision = crate::core::agent::events::validate_decision_input(args)?;
            let task = self
                .harness_context_snapshot()
                .task_id
                .context("decision recording requires current task identity")?;
            let validator = self
                .harness_context
                .decision_validator
                .read()
                .clone()
                .context("canonical decision recording is unavailable")?;
            validator(task, decision.evidence_ids).await?;
            Ok(json!({"recorded":true,"rationale_source":"agent-reported"}))
        })
    }
    /// Unified `cron` executor: dispatches on `action` (create | list | delete).
    /// For legacy alias calls that omit `action`, the action is inferred from
    /// the argument shape: `prompt` implies create, `id` implies delete,
    /// otherwise list.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "Legacy executor aliases remain available for compatibility tests."
        )
    )]
    pub(super) fn cron_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| {
                    if args.get("prompt").is_some() {
                        "create".to_string()
                    } else if args.get("id").is_some() {
                        "delete".to_string()
                    } else {
                        "list".to_string()
                    }
                });
            match action.as_str() {
                "create" => self.cron_create_executor(args).await,
                "list" => self.cron_list_executor(args).await,
                "delete" => self.cron_delete_executor(args).await,
                other => bail!(
                    "cron: unknown action '{other}'. Use action='create' (schedule a prompt), 'list', or 'delete' (requires id)."
                ),
            }
        })
    }

    fn cron_create_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("cron_create requires a non-empty prompt"))?
                .to_string();
            let name = args.get("name").and_then(Value::as_str).map(ToOwned::to_owned);
            let cron = args.get("cron").and_then(Value::as_str);
            let delay_minutes = args.get("delay_minutes").and_then(Value::as_u64);
            let run_at = args.get("run_at").and_then(Value::as_str);

            let schedule = match (cron, delay_minutes, run_at) {
                (Some(expression), None, None) => crate::scheduler::ScheduleSpec::cron5(expression)?,
                (None, Some(minutes), None) => crate::scheduler::ScheduleSpec::fixed_interval(Duration::from_secs(
                    minutes.checked_mul(60).ok_or_else(|| anyhow!("delay_minutes is too large"))?,
                ))?,
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

    fn cron_list_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            Ok(json!({
                "tasks": self.list_session_tasks().await,
            }))
        })
    }

    fn cron_delete_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
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
            native_memory::execute_with_persistent_memory_config(
                &workspace_root,
                self.persistent_memory_config.as_ref(),
                self.persistent_memory_enabled,
                args,
            )
            .await
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

        let (requested_command, raw_command) = parse_command_parts(
            payload,
            "shell run request requires a command",
            "shell run request command cannot be empty",
        )?;
        let shell_script = shell_script_text(payload, raw_command.as_deref(), &requested_command);
        let sandbox_request = self.resolve_exec_sandbox_request(payload).await?;
        let sandbox_config = self.sandbox_config();
        let plan = build_shell_execution_plan(
            &sandbox_config,
            self.workspace_root(),
            &sandbox_request.working_dir_path,
            &requested_command,
            Some(&shell_script),
            sandbox_request.sandbox_permissions,
            sandbox_request.additional_permissions.as_ref(),
        )?;

        Ok(plan.approval_reason)
    }

    pub(super) fn code_search_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_code_search(args).await })
    }

    async fn prepare_exec_run_request(
        &self,
        args: &Value,
        backend: ExecRunBackendKind,
        missing_error: &str,
        empty_error: &str,
    ) -> Result<PreparedExecRunRequest> {
        acquire_executor_rate_limit("exec_command:run", 2.0)?;

        // Direct registry callers do not necessarily run model preflight.
        // Normalize here as well so every pipe/PTY launch reports the same
        // truthful verifier status the gate classifies. This is idempotent.
        let executed_args = tool_intent::shell_args_as_executed(tools::EXEC_COMMAND, args);
        let payload = executed_args
            .as_object()
            .ok_or_else(|| anyhow!("command execution requires a JSON object"))?;
        let background = payload.get("background").and_then(Value::as_bool).unwrap_or(false);
        let stdin = match payload.get("stdin") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| anyhow!("stdin must be a boolean"))?,
        };
        let monitor = match payload.get("monitor_filter") {
            None => None,
            Some(value) => Some(
                MonitorSpec::new(value.as_str().ok_or_else(|| anyhow!("monitor_filter must be a string"))?, None, None)
                    .with_context(|| "invalid monitor_filter")?,
            ),
        };
        if monitor.is_some() && matches!(backend, ExecRunBackendKind::Pty) {
            bail!("monitor_filter is pipe-only in v1; pty runs cannot attach a line monitor");
        }

        let (command, auto_raw_command) = parse_command_parts(payload, missing_error, empty_error)?;
        let shell_program = match backend {
            ExecRunBackendKind::Pty => resolve_shell_preference_with_zsh_fork(
                payload.get("shell").and_then(|value| value.as_str()),
                self.pty_config(),
            )?,
            ExecRunBackendKind::Pipe => {
                resolve_shell_preference(payload.get("shell").and_then(|value| value.as_str()), self.pty_config())
            }
        };
        let login_shell = payload.get("login").and_then(|value| value.as_bool()).unwrap_or(false);
        let confirm = payload.get("confirm").and_then(|value| value.as_bool()).unwrap_or(false);

        let mut prepared_command =
            prepare_exec_command(payload, &shell_program, login_shell, command, auto_raw_command);
        let is_git_diff = is_git_diff_command(&prepared_command.requested_command);

        // Evaluate the concrete invocation so raw shell fragments retain their
        // original quoting. An explicitly supplied shell argv is already a
        // policy boundary; keep that request intact when the runtime's
        // preferred shell differs and would otherwise add a nested wrapper.
        let policy_command = if crate::tools::command_policy::is_shell_argv(&prepared_command.requested_command) {
            &prepared_command.requested_command
        } else {
            &prepared_command.command
        };
        if !self.inventory.command_policy_allows(policy_command) {
            return Err(anyhow!(
                "Execution policy violation: command '{}' is not permitted by the execution policy",
                prepared_command.requested_command_display
            ));
        }

        let inspection_cwd = self
            .pty_manager()
            .working_dir_candidate(crate::tools::command_args::working_dir_text(args))?;
        self.edited_file_monitor_ref()
            .source_inspection()
            .check_shell(args, &inspection_cwd)?;
        let sandbox_request = self.resolve_exec_sandbox_request(payload).await?;
        let output_config = exec_run_output_config(payload, &prepared_command.display_command);

        enforce_pty_command_policy(&prepared_command.display_command, confirm)?;
        let sandbox_config = self.sandbox_config();
        let sandbox_plan = build_shell_execution_plan(
            &sandbox_config,
            self.workspace_root(),
            &sandbox_request.working_dir_path,
            &prepared_command.requested_command,
            Some(&prepared_command.shell_script),
            sandbox_request.sandbox_permissions,
            sandbox_request.additional_permissions.as_ref(),
        )?;
        let sandbox_active = sandbox_plan.sandbox_policy.is_some();
        prepared_command.command = apply_runtime_sandbox_to_command(
            prepared_command.command,
            &prepared_command.requested_command,
            &sandbox_request.working_dir_path,
            sandbox_plan.sandbox_policy,
        )?;

        let rows = match backend {
            ExecRunBackendKind::Pty => {
                Some(parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?)
            }
            ExecRunBackendKind::Pipe => None,
        };
        let cols = match backend {
            ExecRunBackendKind::Pty => {
                Some(parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?)
            }
            ExecRunBackendKind::Pipe => None,
        };

        Ok(PreparedExecRunRequest {
            prepared_command,
            working_dir_path: sandbox_request.working_dir_path,
            output_config,
            yield_duration: Duration::from_millis(
                clamp_exec_yield_ms(
                    payload.get("yield_time_ms").and_then(Value::as_u64),
                    if background { 250 } else { 10_000 },
                )
                .min(if background { 1_000 } else { 30_000 }),
            ),
            session_id: resolve_exec_run_session_id(payload)?,
            shell_program,
            env_overrides: parse_exec_env_overrides(payload)?,
            is_git_diff,
            confirm,
            rows,
            cols,
            sandbox_active,
            background,
            stdin,
            monitor,
        })
    }

    pub(super) async fn execute_command_session(&self, args: Value) -> Result<Value> {
        self.execute_command_session_internal(args, ExecSettlementMode::Manual).await
    }

    pub(super) async fn execute_harness_command_session_terminal_run_raw(&self, args: Value) -> Result<Value> {
        let args = normalize_command_session_run_alias_args(&args, true)?;
        self.execute_command_session_run_pty(args, true).await
    }

    fn dispatch_command_session_alias(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_command_session(args).await.map(super::normalize_tool_output) })
    }

    fn dispatch_command_session_run_alias(&self, args: Value, tty: bool) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let args = normalize_command_session_run_alias_args(&args, tty)?;
            self.execute_command_session(args).await.map(super::normalize_tool_output)
        })
    }

    fn dispatch_command_session_action_alias(&self, args: Value, action: &'static str) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_alias(with_command_session_action_default(args, action))
    }

    pub(super) async fn execute_command_session_internal(
        &self,
        args: Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let launch = self.begin_completion_verifier(&args);
        let result = self.execute_command_session_observed(args.clone(), exec_settlement_mode).await;
        self.observe_completion_verifier(launch, &args, &result);
        result
    }

    async fn execute_command_session_observed(
        &self,
        args: Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let args = crate::tools::command_args::normalize_shell_args(&args).map_err(|error| anyhow!(error))?;

        let action_str =
            tool_intent::command_session_action(&args).ok_or_else(|| missing_command_session_action_error(&args))?;
        let action: CommandSessionAction = parse_action(action_str)?;

        match action {
            CommandSessionAction::Run => self.execute_command_session_run_internal(args, exec_settlement_mode).await,
            CommandSessionAction::Write => self.execute_command_session_write(args).await,
            CommandSessionAction::Poll => self.execute_command_session_poll_internal(args, exec_settlement_mode).await,
            CommandSessionAction::Wait => self.execute_command_session_wait(args).await,
            CommandSessionAction::Continue => {
                self.execute_command_session_continue_internal(args, exec_settlement_mode).await
            }
            CommandSessionAction::Inspect => self.execute_command_session_inspect(args).await,
            CommandSessionAction::List => self.execute_command_session_list().await,
            CommandSessionAction::Close => self.execute_command_session_close(args).await,
            CommandSessionAction::Code => self.execute_code(args).await,
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

    async fn execute_code_search(&self, args: Value) -> Result<Value> {
        let request = serde_json::from_value(args).context("invalid code_search request")?;
        let response = crate::tools::code_search::execute(self.workspace_root(), request).await?;
        serde_json::to_value(response).context("failed to serialise code_search response")
    }

    async fn execute_code(&self, args: Value) -> Result<Value> {
        acquire_executor_rate_limit("unified_exec:code", 2.0)?;

        let code = args
            .get("command")
            .or_else(|| args.get("code"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing code/command in execute_code"))?;

        let cwd = self.workspace_root();
        // Arbitrary interpreter code has the same file-writing authority as a
        // shell script. It cannot be a route around guarded file mutations.
        self.edited_file_monitor_ref().source_inspection().check_shell(&args, cwd)?;
        let language = code_language_from_args(&args);

        // The `code` action spawns the interpreter directly, so it must clear
        // the same command policy as `run`: a policy that excludes
        // `python3`/`node` must not be bypassed through code mode.
        let interpreter = language.interpreter().to_string();
        if !self.inventory.command_policy_allows(std::slice::from_ref(&interpreter)) {
            return Err(anyhow!("code execution via '{interpreter}' is not permitted by the execution policy"));
        }

        let track_files = args.get("track_files").and_then(|v| v.as_bool()).unwrap_or(false);

        let mcp_client = self.mcp_client().ok_or_else(|| anyhow!("MCP client not available"))?;

        let workspace_root = self.workspace_root_owned();
        // Expose built-in tools to the snippet as callable library functions
        // (curated, non-recursive subset) when a weak self-reference was
        // installed at session bootstrap.
        let builtin_executor = self.builtin_executor_for_code();
        let executor =
            crate::exec::code_executor::CodeExecutor::new(language, mcp_client.clone(), workspace_root.clone())
                .with_builtin_executor(builtin_executor);
        let execution_start = SystemTime::now();

        let result = executor.execute(code).await?;

        let mut response = json!(result);

        if track_files {
            let tracker = FileTracker::new(workspace_root);
            match tracker.detect_new_files(execution_start).await {
                Ok(changes) => {
                    response["generated_files"] = json!({
                        "count": changes.len(),
                        "files": changes,
                        "summary": tracker.generate_file_summary(&changes),
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "FileTracker failed to detect new files after code execution"
                    );
                }
            }
        }

        Ok(response)
    }

    async fn execute_apply_patch(&self, args: Value) -> Result<Value> {
        let (patch_args, patch_input_bytes, patch_base64) = self.prepare_apply_patch_args(args)?;
        let context = self.harness_context_snapshot();
        tracing::debug!(
            tool = tools::APPLY_PATCH,
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
            "Prepared apply_patch payload"
        );

        self.execute_apply_patch_internal(patch_args).await
    }

    fn prepare_apply_patch_args(&self, args: Value) -> Result<(Value, usize, bool)> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input {}", crate::tools::error_helpers::PATCH_PARAMETER_HINT))?;
        let patch_input_bytes = patch_input.source_bytes;
        let patch_base64 = patch_input.was_base64;

        // Guard against a bare-string `args` (some callers pass the patch text
        // directly as a JSON string rather than an object). Indexing a
        // `Value::String` with `["input"]` panics in serde_json, so normalize
        // to an object first.
        let mut patch_args = if args.is_object() { args } else { json!({}) };
        patch_args["input"] = json!(patch_input.text);
        Ok((patch_args, patch_input_bytes, patch_base64))
    }

    async fn resolve_exec_sandbox_request(
        &self,
        payload: &serde_json::Map<String, Value>,
    ) -> Result<ResolvedExecSandboxRequest> {
        let working_dir_path = self.pty_manager().resolve_working_dir(shell_working_dir_value(payload)).await?;
        let sandbox_config = self.sandbox_config();
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(payload, self.workspace_root(), &working_dir_path, &sandbox_config)
                .await?;

        Ok(ResolvedExecSandboxRequest {
            working_dir_path,
            sandbox_permissions,
            additional_permissions,
        })
    }

    // ============================================================
    // SPECIALIZED EXECUTORS (Hidden from LLM, used by unified tools)
    // ============================================================

    // File operation executors -- delegate to the file_ops_tool from inventory
    delegate_to_tool!(read_file_executor, file_ops_tool, read_file);
    delegate_to_tool!(write_file_executor, file_ops_tool, write_file);

    // Self-delegating executors -- forward to async methods on ToolRegistry
    delegate_to_self!(list_files_executor, list_files);
    delegate_to_self!(edit_file_executor, edit_file);
    delegate_to_self!(get_errors_executor, execute_get_errors);
    delegate_to_self!(search_tools_executor, execute_search_tools);
    delegate_to_self!(mcp_search_tools_executor, execute_mcp_search_tools);
    delegate_to_self!(mcp_get_tool_details_executor, execute_mcp_get_tool_details);
    delegate_to_self!(mcp_list_servers_executor, execute_mcp_list_servers);
    delegate_to_self!(mcp_connect_server_executor, execute_mcp_connect_server);
    delegate_to_self!(mcp_disconnect_server_executor, execute_mcp_disconnect_server);
    delegate_to_self!(apply_patch_executor, execute_apply_patch);

    /// Unified `mcp` executor: dispatches on `action`
    /// (search_tools | get_tool_details | list_servers | connect | disconnect).
    /// For legacy alias calls that omit `action`, the action is inferred from
    /// the argument shape: `query` implies search_tools, `name` implies
    /// get_tool_details, otherwise list_servers. `connect`/`disconnect` require
    /// an explicit `action` since the schema marks it required, and are
    /// evaluated under the action-qualified policy keys `mcp:connect` /
    /// `mcp:disconnect` so they keep their Prompt confirmation even though
    /// `mcp` itself is `ToolPolicy::Allow`.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "Legacy executor aliases remain available for compatibility tests."
        )
    )]
    pub(super) fn mcp_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| {
                    if args.get("query").is_some() {
                        "search_tools".to_string()
                    } else if args.get("name").is_some() {
                        "get_tool_details".to_string()
                    } else {
                        "list_servers".to_string()
                    }
                });
            match action.as_str() {
                "search_tools" => self.mcp_search_tools_executor(args).await,
                "get_tool_details" => self.mcp_get_tool_details_executor(args).await,
                "list_servers" => self.mcp_list_servers_executor(args).await,
                "connect" => self.mcp_connect_server_executor(args).await,
                "disconnect" => self.mcp_disconnect_server_executor(args).await,
                other => Err(anyhow!(
                    "mcp: unknown action '{other}'. Use action='search_tools' (query), 'get_tool_details' (name), 'list_servers', 'connect' (name), or 'disconnect' (name)."
                )),
            }
        })
    }

    // PTY executors -- distinct signatures, kept explicit.
    // `run_pty_cmd` and `create_pty_session` are intentional aliases: both
    // create a PTY session and run a command. The separate names exist for
    // backward compatibility with existing tool registrations.
    pub(super) fn run_pty_cmd_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_run_alias(args, true)
    }

    pub(super) fn exec_command_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_run_alias(args, false)
    }

    pub(super) fn write_stdin_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(self.execute_write_stdin(args, ExecSettlementMode::Manual))
    }

    pub(super) async fn execute_write_stdin(&self, args: Value, settlement_mode: ExecSettlementMode) -> Result<Value> {
        let (args, dispatch) = normalize_write_stdin_args(&args)?;

        let observed_args = args.clone();
        let result = async {
            let response = match dispatch {
                crate::tools::command_args::WriteStdinDispatch::Write => {
                    self.execute_command_session_write_for_tool(args, tools::WRITE_STDIN).await
                }
                crate::tools::command_args::WriteStdinDispatch::Poll => {
                    self.execute_command_session_poll_for_tool(args, settlement_mode, tools::WRITE_STDIN)
                        .await
                }
                crate::tools::command_args::WriteStdinDispatch::Wait => self.execute_command_session_wait(args).await,
                crate::tools::command_args::WriteStdinDispatch::Inspect => {
                    self.execute_command_session_inspect(args).await
                }
                crate::tools::command_args::WriteStdinDispatch::Terminate => {
                    self.execute_command_session_terminate(args).await
                }
                crate::tools::command_args::WriteStdinDispatch::Close => self.execute_command_session_close(args).await,
            }?;
            Ok(response)
        }
        .await;
        self.observe_completion_verifier(None, &observed_args, &result);
        result
    }

    pub(super) fn send_pty_input_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_action_alias(args, "write")
    }

    pub(super) fn read_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_action_alias(args, "poll")
    }

    pub(super) fn create_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_run_alias(args, true)
    }

    pub(super) fn list_pty_sessions_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_alias(json!({"action": "list"}))
    }

    pub(super) fn close_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        self.dispatch_command_session_action_alias(args, "close")
    }

    // ============================================================
    // INTERNAL IMPLEMENTATIONS
    // ============================================================
}

#[cfg(test)]
mod sandbox_runtime_tests;
#[cfg(test)]
mod session_messaging_tests;

#[cfg(test)]
mod execute_code_tests;
#[cfg(test)]
mod git_diff_tests;
#[cfg(test)]
mod mcp_action_dispatch_tests;
#[cfg(test)]
mod pty_context_tests;
#[cfg(test)]
mod pty_output_filter_tests;
#[cfg(test)]
mod shell_preference_tests;
#[cfg(test)]
mod subagent_tool_output_tests;
#[cfg(test)]
mod token_efficiency_tests;
#[cfg(test)]
mod unified_action_error_tests;
