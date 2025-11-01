use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use shell_words::split;
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::time::sleep;

use crate::tools::apply_patch::Patch;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::Tool;
use crate::tools::types::{EnhancedTerminalInput, VTCodePtySession};
use crate::tools::{
    PlanUpdateResult, PtyCommandRequest, PtyCommandResult, PtyManager, UpdatePlanArgs,
};

const DEFAULT_TERMINAL_TIMEOUT_SECS: u64 = 30;
const DEFAULT_PTY_TIMEOUT_SECS: u64 = 300;
const RUN_PTY_POLL_TIMEOUT_SECS: u64 = 5;
const INTERACTIVE_COMMANDS: &[&str] = &[
    "python",
    "python3",
    "node",
    "npm",
    "yarn",
    "pnpm",
    "bun",
    "irb",
    "pry",
    "node-repl",
    "mysql",
    "psql",
    "sqlite3",
    "vim",
    "nvim",
    "nano",
    "emacs",
    "code",
    "top",
    "htop",
    "ssh",
    "telnet",
    "ftp",
    "sftp",
    "cargo",
    "make",
    "cmake",
    "ninja",
    "gradle",
    "mvn",
    "ant",
    "go",
    "rustc",
    "gcc",
    "g++",
    "clang",
    "javac",
    "dotnet",
];

use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn grep_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.inventory.grep_file_manager();
        Box::pin(async move {
            #[derive(Debug, Deserialize)]
            struct GrepArgs {
                pattern: String,
                #[serde(default = "default_grep_path", alias = "root", alias = "search_path")]
                path: String,
                #[serde(default)]
                max_results: Option<usize>,
                #[serde(default)]
                case_sensitive: Option<bool>,
                #[serde(default)]
                literal: Option<bool>,
                #[serde(default)]
                glob_pattern: Option<String>,
                #[serde(default)]
                context_lines: Option<usize>,
                #[serde(default)]
                include_hidden: Option<bool>,
            }

            fn default_grep_path() -> String {
                ".".to_string()
            }

            let payload: GrepArgs =
                serde_json::from_value(args).context("grep_file requires a 'pattern' field")?;

            let input = GrepSearchInput {
                pattern: payload.pattern.clone(),
                path: payload.path.clone(),
                case_sensitive: payload.case_sensitive,
                literal: payload.literal,
                glob_pattern: payload.glob_pattern,
                context_lines: payload.context_lines,
                include_hidden: payload.include_hidden,
                max_results: payload.max_results,
            };

            let result = manager
                .perform_search(input)
                .await
                .with_context(|| format!("grep_file failed for pattern '{}'", payload.pattern))?;

            Ok(json!({
                "success": true,
                "query": result.query,
                "matches": result.matches,
            }))
        })
    }

    pub(super) fn list_files_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn run_command_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_command(args).await })
    }

    pub(super) fn create_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_create_pty_session(args).await })
    }

    pub(super) fn list_pty_sessions_executor(
        &mut self,
        _args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_list_pty_sessions().await })
    }

    pub(super) fn close_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_close_pty_session(args).await })
    }

    pub(super) fn send_pty_input_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_send_pty_input(args).await })
    }

    pub(super) fn read_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_read_pty_session(args).await })
    }

    pub(super) fn resize_pty_session_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_resize_pty_session(args).await })
    }

    pub(super) fn curl_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.curl_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn git_diff_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.git_diff_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn read_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn create_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.create_file(args).await })
    }

    pub(super) fn delete_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.delete_file(args).await })
    }

    pub(super) fn edit_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn ast_grep_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_ast_grep(args).await })
    }

    pub(super) fn apply_patch_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    pub(super) fn update_plan_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.inventory.plan_manager();
        Box::pin(async move {
            let parsed: UpdatePlanArgs = serde_json::from_value(args)
                .context("update_plan requires plan items with step and status")?;
            let updated_plan = manager
                .update_plan(parsed)
                .context("failed to update plan state")?;
            let payload = PlanUpdateResult::success(updated_plan);
            serde_json::to_value(payload).context("failed to serialize plan update result")
        })
    }

    pub(super) async fn execute_apply_patch(&self, args: Value) -> Result<Value> {
        let patch_source = args
            .get("input")
            .or_else(|| args.get("patch"))
            .or_else(|| args.get("diff"));

        let input = patch_source.and_then(|v| v.as_str()).ok_or_else(|| {
            anyhow!(
                "Error: Missing 'input' string with patch content (aliases: 'patch', 'diff'). Example: apply_patch({{ \"input\": '*** Begin Patch...*** End Patch' }})"
            )
        })?;
        let patch = Patch::parse(input)?;

        // Generate diff preview
        let mut diff_lines = Vec::new();
        for op in patch.operations() {
            match op {
                crate::tools::editing::PatchOperation::AddFile { path, content } => {
                    diff_lines.push(format!("--- /dev/null"));
                    diff_lines.push(format!("+++ {}", path));
                    for line in content.lines() {
                        diff_lines.push(format!("+{}", line));
                    }
                }
                crate::tools::editing::PatchOperation::DeleteFile { path } => {
                    diff_lines.push(format!("--- {}", path));
                    diff_lines.push(format!("+++ /dev/null"));
                }
                crate::tools::editing::PatchOperation::UpdateFile { path, chunks, .. } => {
                    diff_lines.push(format!("--- {}", path));
                    diff_lines.push(format!("+++ {}", path));
                    for chunk in chunks {
                        if let Some(ctx) = &chunk.change_context {
                            diff_lines.push(format!("@@ {} @@", ctx));
                        }
                        for line in &chunk.lines {
                            let (prefix, text) = match line {
                                crate::tools::editing::PatchLine::Addition(t) => ("+", t),
                                crate::tools::editing::PatchLine::Removal(t) => ("-", t),
                                crate::tools::editing::PatchLine::Context(t) => (" ", t),
                            };
                            diff_lines.push(format!("{}{}", prefix, text));
                        }
                    }
                }
            }
        }

        let results = patch.apply(self.workspace_root()).await?;
        Ok(json!({
            "success": true,
            "applied": results,
            "diff_preview": diff_lines.join("\n"),
        }))
    }

    /// Unified command execution that combines terminal and PTY modes
    async fn execute_run_command(&mut self, mut args: Value) -> Result<Value> {
        normalize_run_command_payload(&mut args)?;

        let resolved_mode = resolve_run_mode(&args);
        ensure_default_timeout(
            &mut args,
            "run_command expects an object payload",
            resolved_mode.default_timeout(),
        )?;

        if resolved_mode.is_pty() {
            self.execute_run_pty_command(args).await
        } else {
            self.execute_run_terminal_internal(args).await
        }
    }

    async fn execute_run_terminal_internal(&mut self, mut args: Value) -> Result<Value> {
        match prepare_terminal_execution(&mut args)? {
            TerminalExecution::Pty { args } => self.execute_run_pty_command(args).await,
            TerminalExecution::Terminal(execution) => {
                let plan = self.build_terminal_command_plan(execution).await?;
                plan.execute(self.pty_manager()).await
            }
        }
    }

    async fn execute_run_pty_command(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "run_pty_cmd expects an object payload")?;
        let setup = self.prepare_ephemeral_pty_command(payload).await?;
        self.run_ephemeral_pty_command(setup).await
    }

    async fn prepare_ephemeral_pty_command(
        &self,
        payload: &Map<String, Value>,
    ) -> Result<PtyCommandSetup> {
        let command = parse_command_parts(
            payload,
            "run_pty_cmd requires a 'command' value",
            "PTY command cannot be empty",
        )?;

        let timeout_secs = parse_timeout_secs(
            payload.get("timeout_secs"),
            self.pty_config().command_timeout_seconds,
        )?;
        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;
        let working_dir_display = self.pty_manager().describe_working_dir(&working_dir_path);

        Ok(PtyCommandSetup {
            command,
            working_dir_path,
            working_dir_display,
            session_id: generate_session_id("run"),
            rows,
            cols,
            timeout_secs,
        })
    }

    async fn run_ephemeral_pty_command(&mut self, setup: PtyCommandSetup) -> Result<Value> {
        let mut lifecycle = PtySessionLifecycle::start(self)?;
        self.pty_manager()
            .create_session(
                setup.session_id.clone(),
                setup.command.clone(),
                setup.working_dir_path.clone(),
                setup.size(),
            )
            .with_context(|| {
                format!(
                    "failed to create PTY session '{}' for command {:?}",
                    setup.session_id, setup.command
                )
            })?;
        lifecycle.commit();

        let capture = collect_ephemeral_session_output(
            self.pty_manager(),
            &setup.session_id,
            Duration::from_secs(RUN_PTY_POLL_TIMEOUT_SECS),
        )
        .await;

        let snapshot = self
            .pty_manager()
            .snapshot_session(&setup.session_id)
            .with_context(|| format!("failed to snapshot PTY session '{}'", setup.session_id))?;

        Ok(build_ephemeral_pty_response(&setup, capture, snapshot))
    }

    async fn build_terminal_command_plan(
        &mut self,
        execution: TerminalExecutionInput,
    ) -> Result<TerminalCommandPlan> {
        let TerminalExecutionInput { input, mode_label } = execution;
        let invocation = self
            .inventory
            .command_tool()
            .prepare_invocation(&input)
            .await?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(input.working_dir.as_deref())
            .await?;

        let timeout_secs = validated_timeout_secs(
            input.timeout_secs,
            self.pty_config().command_timeout_seconds,
        )?;

        let command = assemble_command_segments(&invocation.program, &invocation.args);
        let request = PtyCommandRequest {
            command,
            working_dir: working_dir_path.clone(),
            timeout: Duration::from_secs(timeout_secs),
            size: default_pty_size(
                self.pty_config().default_rows,
                self.pty_config().default_cols,
            ),
        };

        let working_directory = self.pty_manager().describe_working_dir(&working_dir_path);

        Ok(TerminalCommandPlan {
            request,
            command_display: invocation.display,
            working_directory,
            mode_label,
            timeout_secs,
        })
    }

    async fn execute_create_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "create_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "create_pty_session requires a 'session_id' string")?;

        let command_parts = parse_command_parts(
            payload,
            "create_pty_session requires a 'command' value",
            "PTY session command cannot be empty",
        )?;

        let working_dir = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;

        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.start_pty_session()?;
        let result = match self.pty_manager().create_session(
            session_id.clone(),
            command_parts.clone(),
            working_dir,
            size,
        ) {
            Ok(meta) => meta,
            Err(error) => {
                self.end_pty_session();
                return Err(error);
            }
        };

        let mut response = snapshot_to_map(result, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        Ok(Value::Object(response))
    }

    async fn execute_list_pty_sessions(&self) -> Result<Value> {
        let sessions = self.pty_manager().list_sessions();
        let identifiers: Vec<String> = sessions.iter().map(|session| session.id.clone()).collect();
        let details: Vec<Value> = sessions
            .into_iter()
            .map(|session| {
                Value::Object(snapshot_to_map(session, PtySnapshotViewOptions::default()))
            })
            .collect();

        Ok(json!({
            "success": true,
            "sessions": identifiers,
            "details": details,
        }))
    }

    async fn execute_close_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "close_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "close_pty_session requires a 'session_id' string")?;

        let metadata = self
            .pty_manager()
            .close_session(session_id.as_str())
            .with_context(|| format!("failed to close PTY session '{session_id}'"))?;
        self.end_pty_session();

        let mut response = snapshot_to_map(metadata, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        Ok(Value::Object(response))
    }

    async fn execute_send_pty_input(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "send_pty_input expects an object payload")?;
        let input = PtyInputPayload::from_map(payload)?;

        let written = self
            .pty_manager()
            .send_input_to_session(
                input.session_id.as_str(),
                &input.buffer,
                input.append_newline,
            )
            .with_context(|| format!("failed to write to PTY session '{}'", input.session_id))?;

        if input.wait_ms > 0 {
            sleep(Duration::from_millis(input.wait_ms)).await;
        }

        let output = self
            .pty_manager()
            .read_session_output(input.session_id.as_str(), input.drain_output)
            .with_context(|| format!("failed to read PTY session '{}' output", input.session_id))?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(input.session_id.as_str())
            .with_context(|| format!("failed to snapshot PTY session '{}'", input.session_id))?;

        let mut response = snapshot_to_map(snapshot, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));
        response.insert("written_bytes".to_string(), Value::from(written));
        response.insert(
            "appended_newline".to_string(),
            Value::Bool(input.append_newline),
        );
        if let Some(output) = output {
            response.insert("output".to_string(), Value::String(output));
        }

        Ok(Value::Object(response))
    }

    async fn execute_read_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "read_pty_session expects an object payload")?;
        let view_args = PtySessionViewArgs::from_map(payload)?;

        let output = self
            .pty_manager()
            .read_session_output(view_args.session_id.as_str(), view_args.drain_output)
            .with_context(|| {
                format!(
                    "failed to read PTY session '{}' output",
                    view_args.session_id
                )
            })?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(view_args.session_id.as_str())
            .with_context(|| {
                format!("failed to snapshot PTY session '{}'", view_args.session_id)
            })?;

        let mut response = snapshot_to_map(snapshot, view_args.view);
        response.insert("success".to_string(), Value::Bool(true));
        if let Some(output) = output {
            response.insert("output".to_string(), Value::String(output));
        }

        Ok(Value::Object(response))
    }

    async fn execute_resize_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = value_as_object(&args, "resize_pty_session expects an object payload")?;
        let session_id =
            parse_session_id(payload, "resize_pty_session requires a 'session_id' string")?;

        let current = self
            .pty_manager()
            .snapshot_session(session_id.as_str())
            .with_context(|| format!("failed to snapshot PTY session '{session_id}'"))?;

        let rows = parse_pty_dimension("rows", payload.get("rows"), current.rows)?;
        let cols = parse_pty_dimension("cols", payload.get("cols"), current.cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let snapshot = self
            .pty_manager()
            .resize_session(session_id.as_str(), size)
            .with_context(|| format!("failed to resize PTY session '{session_id}'"))?;

        let mut response = snapshot_to_map(snapshot, PtySnapshotViewOptions::default());
        response.insert("success".to_string(), Value::Bool(true));

        Ok(Value::Object(response))
    }
}

fn copy_value_if_absent(map: &mut Map<String, Value>, source_key: &str, target_key: &str) {
    if map.contains_key(target_key) {
        return;
    }

    if let Some(value) = map.get(source_key).cloned() {
        map.insert(target_key.to_string(), value);
    }
}

fn normalize_payload<'a>(
    args: &'a mut Value,
    context: &str,
    legacy_error: &str,
) -> Result<&'a mut Map<String, Value>> {
    let map = value_as_object_mut(args, context)?;
    if map.contains_key("bash_command") {
        return Err(anyhow!(legacy_error.to_string()));
    }
    copy_value_if_absent(map, "cwd", "working_dir");
    Ok(map)
}

fn normalize_run_command_payload(args: &mut Value) -> Result<()> {
    normalize_payload(
        args,
        "run_command expects an object payload",
        "bash_command is no longer supported. Use run_command instead.",
    )?;
    Ok(())
}

fn normalize_terminal_payload(args: &mut Value) -> Result<()> {
    {
        let map = normalize_payload(
            args,
            "run_terminal_cmd expects an object payload",
            "bash_command is no longer supported. Use run_terminal_cmd or run_pty_cmd instead.",
        )?;
        if !map.contains_key("mode") {
            match map.get("tty").and_then(|value| value.as_bool()) {
                Some(true) => {
                    map.insert("mode".to_string(), Value::String("pty".to_string()));
                }
                Some(false) => {
                    map.insert("mode".to_string(), Value::String("terminal".to_string()));
                }
                None => {}
            }
        }
        copy_value_if_absent(map, "timeout", "timeout_secs");
    }
    Ok(())
}

fn ensure_default_timeout(args: &mut Value, context: &str, default: u64) -> Result<()> {
    let map = value_as_object_mut(args, context)?;
    if !map.contains_key("timeout_secs") {
        map.insert("timeout_secs".to_string(), Value::Number(default.into()));
    }
    Ok(())
}

fn parse_timeout_secs(value: Option<&Value>, fallback: u64) -> Result<u64> {
    let parsed = value
        .map(|raw| {
            raw.as_u64()
                .ok_or_else(|| anyhow!("timeout_secs must be a positive integer"))
        })
        .transpose()?;
    validated_timeout_secs(parsed, fallback)
}

fn validated_timeout_secs(raw: Option<u64>, fallback: u64) -> Result<u64> {
    let timeout_secs = raw.unwrap_or(fallback);
    if timeout_secs == 0 {
        return Err(anyhow!("timeout_secs must be greater than zero"));
    }
    Ok(timeout_secs)
}

fn assemble_command_segments(program: &str, args: &[String]) -> Vec<String> {
    let mut command = Vec::with_capacity(1 + args.len());
    command.push(program.to_string());
    command.extend(args.iter().cloned());
    command
}

fn default_pty_size(default_rows: u16, default_cols: u16) -> PtySize {
    PtySize {
        rows: default_rows,
        cols: default_cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn run_mode_label(args: &Value) -> String {
    args.get("mode")
        .and_then(|value| value.as_str())
        .unwrap_or("terminal")
        .to_string()
}

fn build_terminal_command_response(
    result: &PtyCommandResult,
    mode_label: &str,
    command_display: &str,
    working_directory: String,
    timeout_secs: u64,
) -> Value {
    json!({
        "success": result.exit_code == 0,
        "exit_code": result.exit_code,
        "stdout": result.output,
        "stderr": "",
        "output": result.output,
        "mode": mode_label,
        "pty_enabled": true,
        "command": command_display,
        "working_directory": working_directory,
        "timeout_secs": timeout_secs,
        "duration_ms": result.duration.as_millis(),
        "pty": {
            "rows": result.size.rows,
            "cols": result.size.cols,
        },
    })
}

fn convert_command_string_to_array(
    args: &mut Value,
    shell_hint: Option<&str>,
    context: &str,
) -> Result<Option<String>> {
    let map = value_as_object_mut(args, context)?;
    let maybe_command = map
        .get("command")
        .and_then(|value| value.as_str().map(|value| value.to_string()));
    let Some(command_str) = maybe_command else {
        return Ok(None);
    };

    let sanitized = sanitize_command_string(&command_str);
    let segments = tokenize_command_string(sanitized.as_ref(), shell_hint)
        .map_err(|err| anyhow!("failed to parse command string: {}", err))?;
    if segments.is_empty() {
        return Err(anyhow!("command string cannot be empty"));
    }

    let command_array = segments.into_iter().map(Value::String).collect::<Vec<_>>();
    map.insert("command".to_string(), Value::Array(command_array));

    Ok(Some(command_str))
}

fn collect_command_vector(
    args: &Value,
    missing_error: &str,
    type_error: &str,
) -> Result<Vec<String>> {
    let map = value_as_object(args, missing_error)?;
    let array = map
        .get("command")
        .ok_or_else(|| anyhow!(missing_error.to_string()))?
        .as_array()
        .ok_or_else(|| anyhow!(missing_error.to_string()))?;

    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(|part| part.to_string())
                .ok_or_else(|| anyhow!(type_error.to_string()))
        })
        .collect()
}

fn determine_terminal_run_mode(args: &Value) -> Result<RunMode> {
    if matches!(
        args.get("mode").and_then(|value| value.as_str()),
        Some("streaming")
    ) {
        return Err(anyhow!("run_terminal_cmd does not support streaming mode"));
    }

    Ok(resolve_run_mode(args))
}

fn build_terminal_command_payload(
    args: &Value,
    command_vec: &[String],
    raw_command: Option<&str>,
) -> Map<String, Value> {
    let mut sanitized = serde_json::Map::new();
    let command_array = command_vec
        .iter()
        .cloned()
        .map(Value::String)
        .collect::<Vec<Value>>();
    sanitized.insert("command".to_string(), Value::Array(command_array));

    if let Some(working_dir) = args.get("working_dir").cloned() {
        sanitized.insert("working_dir".to_string(), working_dir);
    }
    if let Some(timeout) = args.get("timeout_secs").cloned() {
        sanitized.insert("timeout_secs".to_string(), timeout);
    }
    if let Some(response_format) = args.get("response_format").cloned() {
        sanitized.insert("response_format".to_string(), response_format);
    }
    if let Some(raw) = raw_command {
        sanitized.insert("raw_command".to_string(), Value::String(raw.to_string()));
    }
    if let Some(shell) = args.get("shell").cloned() {
        sanitized.insert("shell".to_string(), shell);
    }
    if let Some(login) = args.get("login").cloned() {
        sanitized.insert("login".to_string(), login);
    }

    sanitized
}

fn build_pty_args_from_terminal(args: &Value, command_vec: &[String]) -> Map<String, Value> {
    let mut pty_args = serde_json::Map::new();
    if let Some(program) = command_vec.first() {
        pty_args.insert("command".to_string(), Value::String(program.clone()));
    }
    if command_vec.len() > 1 {
        let rest = command_vec[1..]
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<Value>>();
        pty_args.insert("args".to_string(), Value::Array(rest));
    }
    if let Some(timeout) = args.get("timeout_secs").cloned() {
        pty_args.insert("timeout_secs".to_string(), timeout);
    }
    if let Some(working_dir) = args.get("working_dir").cloned() {
        pty_args.insert("working_dir".to_string(), working_dir);
    }
    if let Some(response_format) = args.get("response_format").cloned() {
        pty_args.insert("response_format".to_string(), response_format);
    }
    if let Some(rows) = args.get("rows").cloned() {
        pty_args.insert("rows".to_string(), rows);
    }
    if let Some(cols) = args.get("cols").cloned() {
        pty_args.insert("cols".to_string(), cols);
    }

    pty_args
}

struct TerminalExecutionInput {
    input: EnhancedTerminalInput,
    mode_label: String,
}

enum TerminalExecution {
    Terminal(TerminalExecutionInput),
    Pty { args: Value },
}

struct TerminalCommandPayload {
    sanitized: Map<String, Value>,
    run_mode: RunMode,
    mode_label: String,
    pty_args: Option<Map<String, Value>>,
}

impl TerminalCommandPayload {
    fn parse(args: &mut Value) -> Result<Self> {
        normalize_terminal_payload(args)?;

        let mode_label = run_mode_label(args);
        let shell_hint = args
            .get("shell")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let raw_command = convert_command_string_to_array(
            args,
            shell_hint.as_deref(),
            "run_terminal_cmd expects an object payload",
        )?;
        let command_vec = collect_command_vector(
            args,
            "run_terminal_cmd requires a 'command' array",
            "command array must contain only strings",
        )?;
        if command_vec.is_empty() {
            return Err(anyhow!("command array cannot be empty"));
        }

        let run_mode = determine_terminal_run_mode(args)?;
        let sanitized = build_terminal_command_payload(args, &command_vec, raw_command.as_deref());
        let pty_args = run_mode
            .is_pty()
            .then(|| build_pty_args_from_terminal(args, &command_vec));

        Ok(Self {
            sanitized,
            run_mode,
            mode_label,
            pty_args,
        })
    }

    fn into_execution(self) -> Result<TerminalExecution> {
        let TerminalCommandPayload {
            sanitized,
            run_mode,
            mode_label,
            pty_args,
        } = self;

        match run_mode {
            RunMode::Pty => {
                let args = pty_args
                    .ok_or_else(|| anyhow!("failed to prepare PTY payload for terminal command"))?;
                Ok(TerminalExecution::Pty {
                    args: Value::Object(args),
                })
            }
            RunMode::Terminal => {
                let sanitized_value = Value::Object(sanitized);
                let input: EnhancedTerminalInput = serde_json::from_value(sanitized_value)
                    .context("failed to parse terminal command input")?;
                Ok(TerminalExecution::Terminal(TerminalExecutionInput {
                    input,
                    mode_label,
                }))
            }
        }
    }
}

struct TerminalCommandPlan {
    request: PtyCommandRequest,
    command_display: String,
    working_directory: String,
    mode_label: String,
    timeout_secs: u64,
}

impl TerminalCommandPlan {
    async fn execute(self, manager: &PtyManager) -> Result<Value> {
        let TerminalCommandPlan {
            request,
            command_display,
            working_directory,
            mode_label,
            timeout_secs,
        } = self;

        let result = manager.run_command(request).await?;
        Ok(build_terminal_command_response(
            &result,
            &mode_label,
            &command_display,
            working_directory,
            timeout_secs,
        ))
    }
}

fn prepare_terminal_execution(args: &mut Value) -> Result<TerminalExecution> {
    let payload = TerminalCommandPayload::parse(args)?;
    payload.into_execution()
}

fn value_as_object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| anyhow!("{}", context))
}

fn value_as_object_mut<'a>(
    value: &'a mut Value,
    context: &str,
) -> Result<&'a mut Map<String, Value>> {
    value.as_object_mut().ok_or_else(|| anyhow!("{}", context))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunMode {
    Terminal,
    Pty,
}

impl RunMode {
    fn default_timeout(self) -> u64 {
        match self {
            RunMode::Terminal => DEFAULT_TERMINAL_TIMEOUT_SECS,
            RunMode::Pty => DEFAULT_PTY_TIMEOUT_SECS,
        }
    }

    fn is_pty(self) -> bool {
        matches!(self, RunMode::Pty)
    }
}

fn resolve_run_mode(args: &Value) -> RunMode {
    if let Some(mode_value) = args.get("mode").and_then(|value| value.as_str()) {
        return match mode_value {
            "pty" => RunMode::Pty,
            "terminal" => RunMode::Terminal,
            "auto" => detect_auto_mode(args),
            _ => RunMode::Terminal,
        };
    }

    if args
        .get("tty")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return RunMode::Pty;
    }

    detect_auto_mode(args)
}

fn detect_auto_mode(args: &Value) -> RunMode {
    if let Some(command) = primary_command(args) {
        if should_use_pty_for_command(&command) {
            return RunMode::Pty;
        }
    }

    RunMode::Terminal
}

fn primary_command(args: &Value) -> Option<String> {
    let command_value = args.get("command")?;

    if let Some(command) = command_value.as_str() {
        return Some(command.to_string());
    }

    command_value
        .as_array()
        .and_then(|values| values.get(0))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn should_use_pty_for_command(command: &str) -> bool {
    INTERACTIVE_COMMANDS
        .iter()
        .any(|candidate| command.contains(candidate))
}

fn parse_command_parts(
    payload: &Map<String, Value>,
    missing_error: &str,
    empty_error: &str,
) -> Result<Vec<String>> {
    let mut parts = match payload.get("command") {
        Some(Value::String(command)) => vec![command.to_string()],
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|part| part.to_string())
                    .ok_or_else(|| anyhow!("command array must contain only strings"))
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => {
            return Err(anyhow!("command must be a string or string array"));
        }
        None => {
            return Err(anyhow!("{}", missing_error));
        }
    };

    if let Some(args_value) = payload.get("args") {
        let args_array = args_value
            .as_array()
            .ok_or_else(|| anyhow!("args must be an array of strings"))?;
        for value in args_array {
            let Some(part) = value.as_str() else {
                return Err(anyhow!("args array must contain only strings"));
            };
            parts.push(part.to_string());
        }
    }

    if parts.is_empty() {
        return Err(anyhow!("{}", empty_error));
    }

    Ok(parts)
}

fn parse_pty_dimension(name: &str, value: Option<&Value>, default: u16) -> Result<u16> {
    let Some(raw) = value else {
        return Ok(default);
    };

    let numeric = raw
        .as_u64()
        .ok_or_else(|| anyhow!("{name} must be an integer"))?;
    if numeric == 0 {
        return Err(anyhow!("{name} must be greater than zero"));
    }
    if numeric > u16::MAX as u64 {
        return Err(anyhow!("{name} exceeds maximum value {}", u16::MAX));
    }

    Ok(numeric as u16)
}

fn bool_from_map(map: &Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn parse_session_id(payload: &Map<String, Value>, missing_error: &str) -> Result<String> {
    let raw_id = payload
        .get("session_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!(missing_error.to_string()))?;
    let trimmed = raw_id.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("session_id cannot be empty"));
    }

    Ok(trimmed.to_string())
}

struct PtyCommandSetup {
    command: Vec<String>,
    working_dir_path: PathBuf,
    working_dir_display: String,
    session_id: String,
    rows: u16,
    cols: u16,
    timeout_secs: u64,
}

impl PtyCommandSetup {
    fn size(&self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PtySnapshotViewOptions {
    include_screen: bool,
    include_scrollback: bool,
}

impl PtySnapshotViewOptions {
    fn new(include_screen: bool, include_scrollback: bool) -> Self {
        Self {
            include_screen,
            include_scrollback,
        }
    }
}

impl Default for PtySnapshotViewOptions {
    fn default() -> Self {
        Self {
            include_screen: true,
            include_scrollback: true,
        }
    }
}

fn snapshot_to_map(
    snapshot: VTCodePtySession,
    options: PtySnapshotViewOptions,
) -> Map<String, Value> {
    let VTCodePtySession {
        id,
        command,
        args,
        working_dir,
        rows,
        cols,
        screen_contents,
        scrollback,
    } = snapshot;

    let mut response = Map::new();
    response.insert("session_id".to_string(), Value::String(id));
    response.insert("command".to_string(), Value::String(command));
    response.insert(
        "args".to_string(),
        Value::Array(args.into_iter().map(Value::String).collect()),
    );
    let working_directory = working_dir.unwrap_or_else(|| ".".to_string());
    response.insert(
        "working_directory".to_string(),
        Value::String(working_directory),
    );
    response.insert("rows".to_string(), Value::from(rows));
    response.insert("cols".to_string(), Value::from(cols));

    if options.include_screen {
        if let Some(screen) = screen_contents {
            response.insert("screen_contents".to_string(), Value::String(screen));
        }
    }

    if options.include_scrollback {
        if let Some(scrollback) = scrollback {
            response.insert("scrollback".to_string(), Value::String(scrollback));
        }
    }

    response
}

struct PtySessionViewArgs {
    session_id: String,
    drain_output: bool,
    view: PtySnapshotViewOptions,
}

impl PtySessionViewArgs {
    fn from_map(map: &Map<String, Value>) -> Result<Self> {
        let session_id = parse_session_id(map, "read_pty_session requires a 'session_id' string")?;
        let drain_output = bool_from_map(map, "drain", false);
        let include_screen = bool_from_map(map, "include_screen", true);
        let include_scrollback = bool_from_map(map, "include_scrollback", true);

        Ok(Self {
            session_id,
            drain_output,
            view: PtySnapshotViewOptions::new(include_screen, include_scrollback),
        })
    }
}

struct PtyInputPayload {
    session_id: String,
    buffer: Vec<u8>,
    append_newline: bool,
    wait_ms: u64,
    drain_output: bool,
}

impl PtyInputPayload {
    fn from_map(map: &Map<String, Value>) -> Result<Self> {
        let session_id = parse_session_id(map, "send_pty_input requires a 'session_id' string")?;
        let append_newline = bool_from_map(map, "append_newline", false);
        let wait_ms = map
            .get("wait_ms")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let drain_output = bool_from_map(map, "drain", true);

        let mut buffer = Vec::new();
        if let Some(text) = map.get("input").and_then(|value| value.as_str()) {
            buffer.extend_from_slice(text.as_bytes());
        }
        if let Some(encoded) = map
            .get("input_base64")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            let decoded = BASE64_STANDARD
                .decode(encoded.as_bytes())
                .context("input_base64 must be valid base64")?;
            buffer.extend_from_slice(&decoded);
        }

        if buffer.is_empty() && !append_newline {
            return Err(anyhow!(
                "send_pty_input requires 'input' or 'input_base64' unless append_newline is true"
            ));
        }

        Ok(Self {
            session_id,
            buffer,
            append_newline,
            wait_ms,
            drain_output,
        })
    }
}

struct PtyEphemeralCapture {
    output: String,
    exit_code: Option<i32>,
    completed: bool,
    duration: Duration,
}

async fn collect_ephemeral_session_output(
    manager: &PtyManager,
    session_id: &str,
    poll_timeout: Duration,
) -> PtyEphemeralCapture {
    let mut output = String::new();
    let start = Instant::now();
    let mut completed = false;
    let mut exit_code = None;

    loop {
        if let Ok(Some(new_output)) = manager.read_session_output(session_id, true) {
            output.push_str(&new_output);
        }

        if let Ok(Some(code)) = manager.is_session_completed(session_id) {
            completed = true;
            exit_code = Some(code);
            break;
        }

        if start.elapsed() > poll_timeout {
            break;
        }

        sleep(Duration::from_millis(100)).await;
    }

    PtyEphemeralCapture {
        output,
        exit_code,
        completed,
        duration: start.elapsed(),
    }
}

fn build_ephemeral_pty_response(
    setup: &PtyCommandSetup,
    capture: PtyEphemeralCapture,
    snapshot: VTCodePtySession,
) -> Value {
    let PtyEphemeralCapture {
        output,
        exit_code,
        completed,
        duration,
    } = capture;

    let session_reference = if completed {
        None
    } else {
        Some(setup.session_id.clone())
    };
    let code = if completed { exit_code } else { None };

    json!({
        "success": true,
        "command": setup.command.clone(),
        "output": output,
        "code": code,
        "mode": "pty",
        "session_id": session_reference,
        "pty": {
            "rows": snapshot.rows,
            "cols": snapshot.cols,
        },
        "working_directory": setup.working_dir_display.clone(),
        "timeout_secs": setup.timeout_secs,
        "duration_ms": if completed { duration.as_millis() } else { 0 },
    })
}

fn sanitize_command_string(command: &str) -> Cow<'_, str> {
    let trimmed = command.trim_end_matches(char::is_whitespace);

    for &quote in &['\'', '"'] {
        let quote_count = trimmed.matches(quote).count();
        if quote_count % 2 != 0 && trimmed.ends_with(quote) {
            let mut adjusted = trimmed.to_string();
            adjusted.pop();
            return Cow::Owned(adjusted);
        }
    }

    if trimmed.len() != command.len() {
        Cow::Owned(trimmed.to_string())
    } else {
        Cow::Borrowed(command)
    }
}

fn tokenize_command_string(command: &str, shell_hint: Option<&str>) -> Result<Vec<String>> {
    if should_use_windows_command_tokenizer(shell_hint) {
        return tokenize_windows_command(command);
    }

    split(command).map_err(|err| anyhow!(err))
}

fn should_use_windows_command_tokenizer(shell_hint: Option<&str>) -> bool {
    if let Some(shell) = shell_hint {
        if is_windows_shell(shell) {
            return true;
        }
    }

    cfg!(windows)
}

fn tokenize_windows_command(command: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut token_started = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_quotes {
                    if matches!(chars.peek(), Some('"')) {
                        current.push('"');
                        token_started = true;
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                } else {
                    in_quotes = true;
                    token_started = true;
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if token_started {
                    tokens.push(current);
                    current = String::new();
                    token_started = false;
                }
            }
            _ => {
                current.push(ch);
                token_started = true;
            }
        }
    }

    if in_quotes {
        return Err(anyhow!("unterminated quote in command string"));
    }

    if token_started {
        tokens.push(current);
    }

    Ok(tokens)
}

fn is_windows_shell(shell: &str) -> bool {
    matches!(
        normalized_shell_name(shell).as_str(),
        "cmd" | "cmd.exe" | "powershell" | "powershell.exe" | "pwsh"
    )
}

fn normalized_shell_name(shell: &str) -> String {
    Path::new(shell)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(shell)
        .to_ascii_lowercase()
}

fn generate_session_id(prefix: &str) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis();
    format!("{prefix}-{timestamp}")
}

struct PtySessionLifecycle<'a> {
    registry: &'a ToolRegistry,
    active: bool,
}

impl<'a> PtySessionLifecycle<'a> {
    fn start(registry: &'a ToolRegistry) -> Result<Self> {
        registry.start_pty_session()?;
        Ok(Self {
            registry,
            active: true,
        })
    }

    fn commit(&mut self) {
        self.active = false;
    }
}

impl Drop for PtySessionLifecycle<'_> {
    fn drop(&mut self) {
        if self.active {
            self.registry.end_pty_session();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalized_shell_name, should_use_windows_command_tokenizer, tokenize_command_string,
        tokenize_windows_command,
    };

    #[test]
    fn windows_tokenizer_preserves_paths_with_spaces() {
        let command = r#""C:\Program Files\Git\bin\bash.exe" -lc "echo hi""#;
        let tokens = tokenize_command_string(command, Some("cmd.exe")).expect("tokens");
        assert_eq!(
            tokens,
            vec![
                r"C:\Program Files\Git\bin\bash.exe".to_string(),
                "-lc".to_string(),
                "echo hi".to_string(),
            ]
        );
    }

    #[test]
    fn windows_tokenizer_handles_empty_arguments() {
        let tokens = tokenize_windows_command("\"\"").expect("tokens");
        assert_eq!(tokens, vec![String::new()]);
    }

    #[test]
    fn windows_tokenizer_errors_on_unterminated_quotes() {
        let err = tokenize_windows_command("\"unterminated").unwrap_err();
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn tokenizer_uses_posix_rules_for_posix_shells() {
        let tokens =
            tokenize_command_string("echo 'hello world'", Some("/bin/bash")).expect("tokens");
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn detects_windows_shell_name_variants() {
        assert!(should_use_windows_command_tokenizer(Some(
            "C:/Windows/System32/cmd.exe"
        )));
        assert!(should_use_windows_command_tokenizer(Some("pwsh")));
        assert_eq!(normalized_shell_name("/bin/bash"), "bash");
    }
}
