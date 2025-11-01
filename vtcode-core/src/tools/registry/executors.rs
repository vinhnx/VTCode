use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde::Deserialize;
use serde_json::{Value, json};
use shell_words::split;
use std::{
    borrow::Cow,
    path::Path,
    time::{Duration, Instant},
};
use tokio::time::sleep;

use crate::tools::apply_patch::Patch;
use crate::tools::grep_file::GrepSearchInput;
use crate::tools::traits::Tool;
use crate::tools::types::{EnhancedTerminalInput, VTCodePtySession};
use crate::tools::{PlanUpdateResult, PtyCommandRequest, UpdatePlanArgs};

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
        // Legacy support for old tool names
        if args.get("bash_command").is_some() {
            return Err(anyhow!(
                "bash_command is no longer supported. Use run_command instead."
            ));
        }

        // Support legacy payloads that send cwd/tty/timeout fields
        if args.get("working_dir").is_none() {
            if let Some(cwd) = args.get("cwd").cloned() {
                if let Some(map) = args.as_object_mut() {
                    map.insert("working_dir".to_string(), cwd);
                }
            }
        }

        // Auto-detect mode if not specified
        let mode = if let Some(mode) = args.get("mode").and_then(|v| v.as_str()) {
            mode.to_string()
        } else if args.get("tty").and_then(|v| v.as_bool()).unwrap_or(false) {
            "pty".to_string()
        } else {
            // Auto-detect: use PTY for interactive programs
            "auto".to_string()
        };

        // Smart mode detection
        let final_mode = if mode == "auto" {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    args.get("command")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.get(0))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("");

            // Commands that typically need PTY or are long-running
            let interactive_commands = [
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

            if interactive_commands
                .iter()
                .any(|&cmd| command.contains(cmd))
            {
                "pty"
            } else {
                "terminal"
            }
        } else {
            &mode
        };

        // Set appropriate defaults based on mode
        if args.get("timeout_secs").is_none() {
            let timeout = if final_mode == "pty" { 300 } else { 30 };
            if let Some(map) = args.as_object_mut() {
                map.insert("timeout_secs".to_string(), Value::Number(timeout.into()));
            }
        }

        // Execute in the appropriate mode
        if final_mode == "pty" {
            self.execute_run_pty_command(args).await
        } else {
            self.execute_run_terminal_internal(args).await
        }
    }

    async fn execute_run_terminal_internal(&mut self, mut args: Value) -> Result<Value> {
        // Legacy bash_command payloads are no longer supported
        // Users should use run_terminal_cmd or run_pty_cmd instead
        if args.get("bash_command").is_some() {
            return Err(anyhow!(
                "bash_command is no longer supported. Use run_terminal_cmd or run_pty_cmd instead."
            ));
        }

        // Support legacy payloads that send cwd/tty/timeout fields instead of the
        // normalized variants used by the modular registry.
        if args.get("working_dir").is_none() {
            if let Some(cwd) = args.get("cwd").cloned() {
                if let Some(map) = args.as_object_mut() {
                    map.insert("working_dir".to_string(), cwd);
                }
            }
        }

        if args.get("mode").is_none() {
            if let Some(tty_requested) = args.get("tty").and_then(|value| value.as_bool()) {
                if let Some(map) = args.as_object_mut() {
                    let mode = if tty_requested { "pty" } else { "terminal" };
                    map.insert("mode".to_string(), Value::String(mode.to_string()));
                }
            }
        }

        if args.get("timeout_secs").is_none() {
            if let Some(timeout) = args.get("timeout").cloned() {
                if let Some(map) = args.as_object_mut() {
                    map.insert("timeout_secs".to_string(), timeout);
                }
            }
        }

        let raw_command = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let shell_hint = args
            .get("shell")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Normalize string command to array
        if let Some(command_str) = raw_command.clone() {
            let sanitized = sanitize_command_string(&command_str);
            let segments = tokenize_command_string(sanitized.as_ref(), shell_hint.as_deref())
                .map_err(|err| anyhow!("failed to parse command string: {}", err))?;
            if segments.is_empty() {
                return Err(anyhow!("command string cannot be empty"));
            }

            let command_array = segments
                .iter()
                .cloned()
                .map(Value::String)
                .collect::<Vec<_>>();

            args.as_object_mut()
                .expect("run_terminal_cmd args must be an object")
                .insert("command".to_string(), Value::Array(command_array));
        }

        let command_vec = args
            .get("command")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("run_terminal_cmd requires a 'command' array"))?
            .iter()
            .map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| anyhow!("command array must contain only strings"))?;

        if command_vec.is_empty() {
            return Err(anyhow!("command array cannot be empty"));
        }

        let mode = args
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("terminal");

        if mode == "streaming" {
            return Err(anyhow!("run_terminal_cmd does not support streaming mode"));
        }

        if mode == "pty" {
            // Delegate to run_pty_cmd for compatibility
            let mut pty_args = serde_json::Map::new();
            pty_args.insert("command".to_string(), Value::String(command_vec[0].clone()));
            if command_vec.len() > 1 {
                let rest = command_vec[1..]
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect();
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
            return self.execute_run_pty_command(Value::Object(pty_args)).await;
        }

        // Build sanitized arguments for command tool preparation
        let mut sanitized = serde_json::Map::new();
        let command_array = command_vec
            .into_iter()
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
            sanitized.insert("raw_command".to_string(), Value::String(raw));
        }

        if let Some(shell) = args.get("shell").cloned() {
            sanitized.insert("shell".to_string(), shell);
        }

        if let Some(login) = args.get("login").cloned() {
            sanitized.insert("login".to_string(), login);
        }

        let sanitized_value = Value::Object(sanitized);
        let input: EnhancedTerminalInput = serde_json::from_value(sanitized_value)
            .context("failed to parse terminal command input")?;
        let invocation = self
            .inventory
            .command_tool()
            .prepare_invocation(&input)
            .await?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(input.working_dir.as_deref())
            .await?;
        let timeout_secs = input
            .timeout_secs
            .unwrap_or(self.pty_config().command_timeout_seconds);
        if timeout_secs == 0 {
            return Err(anyhow!("timeout_secs must be greater than zero"));
        }

        let mut command = Vec::with_capacity(1 + invocation.args.len());
        command.push(invocation.program.clone());
        command.extend(invocation.args.iter().cloned());

        let size = PtySize {
            rows: self.pty_config().default_rows,
            cols: self.pty_config().default_cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let request = PtyCommandRequest {
            command,
            working_dir: working_dir_path.clone(),
            timeout: Duration::from_secs(timeout_secs),
            size,
        };

        let result = self.pty_manager().run_command(request).await?;
        let working_directory = self.pty_manager().describe_working_dir(&working_dir_path);

        Ok(json!({
            "success": result.exit_code == 0,
            "exit_code": result.exit_code,
            "stdout": result.output,
            "stderr": "",
            "output": result.output,
            "mode": mode,
            "pty_enabled": true,
            "command": invocation.display,
            "working_directory": working_directory,
            "timeout_secs": timeout_secs,
            "duration_ms": result.duration.as_millis(),
            "pty": {
                "rows": result.size.rows,
                "cols": result.size.cols,
            },
        }))
    }

    async fn execute_run_pty_command(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("run_pty_cmd expects an object payload"))?;

        let mut command_parts = match payload.get("command") {
            Some(Value::String(command)) => vec![command.to_string()],
            Some(Value::Array(parts)) => parts
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
                return Err(anyhow!("run_pty_cmd requires a 'command' value"));
            }
        };

        if let Some(args_value) = payload.get("args") {
            if let Some(array) = args_value.as_array() {
                for value in array {
                    let Some(part) = value.as_str() else {
                        return Err(anyhow!("args array must contain only strings"));
                    };
                    command_parts.push(part.to_string());
                }
            } else {
                return Err(anyhow!("args must be an array of strings"));
            }
        }

        if command_parts.is_empty() {
            return Err(anyhow!("PTY command cannot be empty"));
        }

        let timeout_secs = payload
            .get("timeout_secs")
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| anyhow!("timeout_secs must be a positive integer"))
            })
            .transpose()?
            .unwrap_or(self.pty_config().command_timeout_seconds);
        if timeout_secs == 0 {
            return Err(anyhow!("timeout_secs must be greater than zero"));
        }

        let parse_dimension = |name: &str, value: Option<&Value>, default: u16| -> Result<u16> {
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
        };

        let rows = parse_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols = parse_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let working_dir = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;
        let working_dir_display = self.pty_manager().describe_working_dir(&working_dir);

        let session_id = format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.start_pty_session()?;
        let _result = match self.pty_manager().create_session(
            session_id.clone(),
            command_parts.clone(),
            working_dir.clone(),
            size,
        ) {
            Ok(meta) => meta,
            Err(error) => {
                self.end_pty_session();
                return Err(error);
            }
        };

        // Collect output until completion or timeout
        let mut output = String::new();
        let poll_timeout = Duration::from_secs(5);
        let start = Instant::now();
        let mut completed = false;
        let mut exit_code = None;

        loop {
            if let Some(new_output) = self
                .pty_manager()
                .read_session_output(&session_id, true)
                .ok()
                .flatten()
            {
                output.push_str(&new_output);
            }

            // Check if command completed
            if let Ok(Some(code)) = self.pty_manager().is_session_completed(&session_id) {
                completed = true;
                exit_code = Some(code);
                break;
            }

            if start.elapsed() > poll_timeout {
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let snapshot = self
            .pty_manager()
            .snapshot_session(&session_id)
            .with_context(|| format!("failed to snapshot PTY session '{}'", session_id))?;

        let code = if completed { exit_code } else { None };

        let duration_ms = if completed {
            start.elapsed().as_millis()
        } else {
            0
        };

        Ok(json!({
        "success": true,
            "command": command_parts,
            "output": output,
            "code": code,
            "mode": "pty",
            "session_id": if completed { None } else { Some(session_id) },
            "pty": {
                "rows": snapshot.rows,
                "cols": snapshot.cols,
            },
            "working_directory": working_dir_display,
            "timeout_secs": timeout_secs,
            "duration_ms": duration_ms,
        }))
    }

    async fn execute_create_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("create_pty_session expects an object payload"))?;

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("create_pty_session requires a 'session_id' string"))?
            .trim();

        if session_id.is_empty() {
            return Err(anyhow!("session_id cannot be empty"));
        }

        let mut command_parts = match payload.get("command") {
            Some(Value::String(command)) => vec![command.to_string()],
            Some(Value::Array(parts)) => parts
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
                return Err(anyhow!("create_pty_session requires a 'command' value"));
            }
        };

        if let Some(args_value) = payload.get("args") {
            if let Some(array) = args_value.as_array() {
                for value in array {
                    let Some(part) = value.as_str() else {
                        return Err(anyhow!("args array must contain only strings"));
                    };
                    command_parts.push(part.to_string());
                }
            } else {
                return Err(anyhow!("args must be an array of strings"));
            }
        }

        if command_parts.is_empty() {
            return Err(anyhow!("PTY session command cannot be empty"));
        }

        let working_dir = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;

        let parse_dimension = |name: &str, value: Option<&Value>, default: u16| -> Result<u16> {
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
        };

        let rows = parse_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols = parse_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.start_pty_session()?;
        let result = match self.pty_manager().create_session(
            session_id.to_string(),
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

        Ok(json!({
            "success": true,
            "session_id": result.id,
            "command": result.command,
            "args": result.args,
            "rows": result.rows,
            "cols": result.cols,
            "working_directory": result.working_dir.unwrap_or_else(|| ".".to_string()),
            "screen_contents": result.screen_contents,
            "scrollback": result.scrollback,
        }))
    }

    async fn execute_list_pty_sessions(&self) -> Result<Value> {
        let sessions = self.pty_manager().list_sessions();
        let identifiers: Vec<String> = sessions.iter().map(|session| session.id.clone()).collect();
        let details: Vec<Value> = sessions
            .into_iter()
            .map(|session| {
                json!({
                    "session_id": session.id,
                    "command": session.command,
                    "args": session.args,
                    "working_directory": session.working_dir.unwrap_or_else(|| ".".to_string()),
                    "rows": session.rows,
                    "cols": session.cols,
                    "screen_contents": session.screen_contents,
                    "scrollback": session.scrollback,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "sessions": identifiers,
            "details": details,
        }))
    }

    async fn execute_close_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("close_pty_session expects an object payload"))?;

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("close_pty_session requires a 'session_id' string"))?
            .trim();

        if session_id.is_empty() {
            return Err(anyhow!("session_id cannot be empty"));
        }

        let metadata = self
            .pty_manager()
            .close_session(session_id)
            .with_context(|| format!("failed to close PTY session '{session_id}'"))?;
        self.end_pty_session();

        Ok(json!({
            "success": true,
            "session_id": metadata.id,
            "command": metadata.command,
            "args": metadata.args,
            "rows": metadata.rows,
            "cols": metadata.cols,
            "working_directory": metadata.working_dir.unwrap_or_else(|| ".".to_string()),
            "screen_contents": metadata.screen_contents,
            "scrollback": metadata.scrollback,
        }))
    }

    async fn execute_send_pty_input(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("send_pty_input expects an object payload"))?;

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("send_pty_input requires a 'session_id' string"))?
            .trim();

        if session_id.is_empty() {
            return Err(anyhow!("session_id cannot be empty"));
        }

        let append_newline = payload
            .get("append_newline")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let wait_ms = payload
            .get("wait_ms")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let drain_output = payload
            .get("drain")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);

        let mut buffer = Vec::new();
        if let Some(text) = payload.get("input").and_then(|value| value.as_str()) {
            buffer.extend_from_slice(text.as_bytes());
        }
        if let Some(encoded) = payload
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

        let written = self
            .pty_manager()
            .send_input_to_session(session_id, &buffer, append_newline)
            .with_context(|| format!("failed to write to PTY session '{session_id}'"))?;

        if wait_ms > 0 {
            sleep(Duration::from_millis(wait_ms)).await;
        }

        let output = self
            .pty_manager()
            .read_session_output(session_id, drain_output)
            .with_context(|| format!("failed to read PTY session '{session_id}' output"))?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(session_id)
            .with_context(|| format!("failed to snapshot PTY session '{session_id}'"))?;

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
        let working_directory = working_dir.unwrap_or_else(|| ".".to_string());

        let mut response = json!({
            "success": true,
            "session_id": id,
            "command": command,
            "args": args,
            "rows": rows,
            "cols": cols,
            "working_directory": working_directory,
            "written_bytes": written,
            "appended_newline": append_newline,
        });

        if let Some(screen) = screen_contents {
            response["screen_contents"] = Value::String(screen);
        }
        if let Some(scrollback) = scrollback {
            response["scrollback"] = Value::String(scrollback);
        }
        if let Some(output) = output {
            response["output"] = Value::String(output);
        }

        Ok(response)
    }

    async fn execute_read_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("read_pty_session expects an object payload"))?;

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("read_pty_session requires a 'session_id' string"))?
            .trim();

        if session_id.is_empty() {
            return Err(anyhow!("session_id cannot be empty"));
        }

        let drain_output = payload
            .get("drain")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let include_screen = payload
            .get("include_screen")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let include_scrollback = payload
            .get("include_scrollback")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);

        let output = self
            .pty_manager()
            .read_session_output(session_id, drain_output)
            .with_context(|| format!("failed to read PTY session '{session_id}' output"))?;
        let snapshot = self
            .pty_manager()
            .snapshot_session(session_id)
            .with_context(|| format!("failed to snapshot PTY session '{session_id}'"))?;

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
        let working_directory = working_dir.unwrap_or_else(|| ".".to_string());

        let mut response = json!({
            "success": true,
            "session_id": id,
            "command": command,
            "args": args,
            "rows": rows,
            "cols": cols,
            "working_directory": working_directory,
        });

        if include_screen {
            if let Some(screen) = screen_contents {
                response["screen_contents"] = Value::String(screen);
            }
        }
        if include_scrollback {
            if let Some(scrollback) = scrollback {
                response["scrollback"] = Value::String(scrollback);
            }
        }
        if let Some(output) = output {
            response["output"] = Value::String(output);
        }

        Ok(response)
    }

    async fn execute_resize_pty_session(&mut self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("resize_pty_session expects an object payload"))?;

        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("resize_pty_session requires a 'session_id' string"))?
            .trim();

        if session_id.is_empty() {
            return Err(anyhow!("session_id cannot be empty"));
        }

        let current = self
            .pty_manager()
            .snapshot_session(session_id)
            .with_context(|| format!("failed to snapshot PTY session '{session_id}'"))?;

        let parse_dimension = |name: &str, value: Option<&Value>, default: u16| -> Result<u16> {
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
        };

        let rows = parse_dimension("rows", payload.get("rows"), current.rows)?;
        let cols = parse_dimension("cols", payload.get("cols"), current.cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let snapshot = self
            .pty_manager()
            .resize_session(session_id, size)
            .with_context(|| format!("failed to resize PTY session '{session_id}'"))?;

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
        let working_directory = working_dir.unwrap_or_else(|| ".".to_string());

        let mut response = json!({
            "success": true,
            "session_id": id,
            "command": command,
            "args": args,
            "rows": rows,
            "cols": cols,
            "working_directory": working_directory,
        });

        if let Some(screen) = screen_contents {
            response["screen_contents"] = Value::String(screen);
        }
        if let Some(scrollback) = scrollback {
            response["scrollback"] = Value::String(scrollback);
        }

        Ok(response)
    }
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
