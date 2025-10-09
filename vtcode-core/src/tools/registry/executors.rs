use anyhow::{Context, Result, anyhow};
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde_json::{Value, json};
use std::time::Duration;

use crate::tools::apply_patch::Patch;
use crate::tools::traits::Tool;
use crate::tools::{PlanUpdateResult, PtyCommandRequest, UpdatePlanArgs};

use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn grep_search_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.search_tool.clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn list_files_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.file_ops_tool.clone();
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

    pub(super) fn curl_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.curl_tool.clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn read_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.file_ops_tool.clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.file_ops_tool.clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn edit_file_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn ast_grep_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_ast_grep(args).await })
    }

    pub(super) fn simple_search_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.simple_search_tool.clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn bash_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_terminal(args, true).await })
    }

    pub(super) fn apply_patch_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    pub(super) fn srgn_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.srgn_tool.clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn update_plan_executor(&mut self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.plan_manager.clone();
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
        let results = patch.apply(&self.workspace_root).await?;
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
        if invoked_from_bash {
            return self.bash_tool.execute(args).await;
        }

        // Support legacy bash_command payloads by routing through bash tool
        if args.get("bash_command").is_some() {
            return self.bash_tool.execute(args).await;
        }

        // Normalize string command to array
        if let Some(command_str) = args
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        {
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
            return self.bash_tool.execute(Value::Object(bash_args)).await;
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

        let tool = self.command_tool.clone();
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
            .unwrap_or(self.pty_config.command_timeout_seconds);
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

        let rows = parse_dimension("rows", payload.get("rows"), self.pty_config.default_rows)?;
        let cols = parse_dimension("cols", payload.get("cols"), self.pty_config.default_cols)?;

        let working_dir = self
            .pty_manager
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))?;
        let working_dir_display = self.pty_manager.describe_working_dir(&working_dir);

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

        let result = self.pty_manager.run_command(request).await?;

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
            .pty_manager
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

        let rows = parse_dimension("rows", payload.get("rows"), self.pty_config.default_rows)?;
        let cols = parse_dimension("cols", payload.get("cols"), self.pty_config.default_cols)?;

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        self.start_pty_session()?;
        let result = match self.pty_manager.create_session(
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
        }))
    }

    async fn execute_list_pty_sessions(&self) -> Result<Value> {
        let sessions = self.pty_manager.list_sessions();
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
            .pty_manager
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
        }))
    }
}
