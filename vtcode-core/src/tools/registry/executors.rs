use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

use crate::tools::apply_patch::Patch;
use crate::tools::traits::Tool;
use crate::tools::types::VTCodePtySession;
use crate::tools::{PlanUpdateResult, PtyCommandRequest, UpdatePlanArgs};

use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn grep_search_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.search_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn list_files_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn run_terminal_cmd_executor(
        &mut self,
        args: Value,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_terminal(args, false).await })
    }

    pub(super) fn run_pty_cmd_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_pty_command(args).await })
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

    pub(super) fn read_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn edit_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn ast_grep_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_ast_grep(args).await })
    }

    pub(super) fn simple_search_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.simple_search_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn bash_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_terminal(args, true).await })
    }

    pub(super) fn apply_patch_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    pub(super) fn srgn_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.srgn_tool().clone();
        Box::pin(async move { tool.execute(args).await })
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
        let input = args
            .get("input")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Error: Missing 'input' string with patch content. Example: apply_patch({{ input: '*** Begin Patch...*** End Patch' }})"))?;
        let patch = Patch::parse(input)?;
        let results = patch.apply(self.workspace_root()).await?;
        Ok(json!({
            "success": true,
            "applied": results,
        }))
    }

    async fn execute_run_terminal(
        &mut self,
        mut args: Value,
        invoked_from_bash: bool,
    ) -> Result<Value> {
        let bash_tool = self.inventory.bash_tool().clone();
        if invoked_from_bash {
            return bash_tool.execute(args).await;
        }

        // Support legacy bash_command payloads by routing through bash tool
        if args.get("bash_command").is_some() {
            return bash_tool.execute(args).await;
        }

        let raw_command = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Normalize string command to array
        if let Some(command_str) = raw_command.clone() {
            args.as_object_mut()
                .expect("run_terminal_cmd args must be an object")
                .insert(
                    "command".to_string(),
                    Value::Array(vec![Value::String(command_str)]),
                );
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

        if matches!(mode, "pty" | "streaming") {
            // Delegate to bash tool's "run" command for compatibility
            let mut bash_args = serde_json::Map::new();
            bash_args.insert("bash_command".to_string(), Value::String("run".to_string()));
            bash_args.insert("command".to_string(), Value::String(command_vec[0].clone()));
            if command_vec.len() > 1 {
                let rest = command_vec[1..]
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect();
                bash_args.insert("args".to_string(), Value::Array(rest));
            }
            if let Some(timeout) = args.get("timeout_secs").cloned() {
                bash_args.insert("timeout_secs".to_string(), timeout);
            }
            if let Some(working_dir) = args.get("working_dir").cloned() {
                bash_args.insert("working_dir".to_string(), working_dir);
            }
            if let Some(response_format) = args.get("response_format").cloned() {
                bash_args.insert("response_format".to_string(), response_format);
            }
            return bash_tool.execute(Value::Object(bash_args)).await;
        }

        // Build sanitized arguments for command tool
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

        let tool = self.inventory.command_tool().clone();
        tool.execute(Value::Object(sanitized)).await
    }

    async fn execute_run_pty_command(&self, args: Value) -> Result<Value> {
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
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))?;
        let working_dir_display = self.pty_manager().describe_working_dir(&working_dir);

        let request = PtyCommandRequest {
            command: command_parts.clone(),
            working_dir: working_dir.clone(),
            timeout: Duration::from_secs(timeout_secs),
            size: PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let result = self.pty_manager().run_command(request).await?;

        Ok(json!({
            "success": true,
            "command": command_parts,
            "output": result.output,
            "code": result.exit_code,
            "mode": "pty",
            "pty": {
                "rows": result.size.rows,
                "cols": result.size.cols,
            },
            "working_directory": working_dir_display,
            "timeout_secs": timeout_secs,
            "duration_ms": result.duration.as_millis(),
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
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))?;

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
