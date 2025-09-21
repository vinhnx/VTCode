use anyhow::{Context, Result, anyhow};
use futures::future::BoxFuture;
use serde_json::{Value, json};

use crate::tools::apply_patch::Patch;
use crate::tools::traits::Tool;
use crate::tools::{PlanUpdateResult, UpdatePlanArgs};

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
}
