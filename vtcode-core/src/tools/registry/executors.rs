use crate::exec::code_executor::Language;
use crate::exec::skill_manager::{Skill, SkillMetadata};
use crate::mcp::{DetailLevel, ToolDiscovery};
use crate::tools::file_tracker::FileTracker;
use crate::tools::registry::declarations::{
    UnifiedExecAction, UnifiedFileAction, UnifiedSearchAction,
};
use crate::tools::tool_intent;
use crate::tools::traits::Tool;
use crate::tools::types::VTCodePtySession;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use anyhow::{Context, Result, anyhow};
use chrono;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use super::ToolRegistry;

fn summarized_arg_keys(args: &Value) -> String {
    match args.as_object() {
        Some(map) => {
            if map.is_empty() {
                return "<none>".to_string();
            }
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            let mut preview = keys.into_iter().take(10).collect::<Vec<_>>().join(", ");
            if map.len() > 10 {
                preview.push_str(", ...");
            }
            preview
        }
        None => match args {
            Value::Null => "<null>".to_string(),
            Value::Array(_) => "<array>".to_string(),
            Value::String(_) => "<string>".to_string(),
            Value::Bool(_) => "<bool>".to_string(),
            Value::Number(_) => "<number>".to_string(),
            Value::Object(_) => "<object>".to_string(),
        },
    }
}

fn patch_source_from_args(args: &Value) -> Option<&str> {
    args.as_str()
        .or_else(|| args.get("input").and_then(|v| v.as_str()))
        .or_else(|| args.get("patch").and_then(|v| v.as_str()))
}

fn serialized_payload_size_bytes(args: &Value) -> usize {
    serde_json::to_vec(args)
        .map(|bytes| bytes.len())
        .unwrap_or_else(|_| args.to_string().len())
}

fn missing_unified_exec_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing action in unified_exec. Provide `action` or inferable fields: \
         `command|cmd|raw_command` (run), `input|chars|text` with `session_id` (write), \
         `session_id` (poll), or `action:\"list\"`/`action:\"close\"`. \
         Received keys: {}",
        summarized_arg_keys(args)
    )
}

fn missing_unified_file_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing action in unified_file. Provide `action` or file-operation fields such as \
         `path`, `content`, `old_str`, `patch`, or `destination`. Received keys: {}",
        summarized_arg_keys(args)
    )
}

fn missing_unified_search_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing action in unified_search. Provide `action` or inferable fields: \
         `pattern|query` (grep), `path` (list), `keyword` (tools), \
         `scope` (errors), `url` (web), `sub_action|name` (skill). Received keys: {}",
        summarized_arg_keys(args)
    )
}

fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.trim().is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn build_session_command_display(session: &VTCodePtySession) -> String {
    let args = &session.args;
    if let Some(flag_index) = args
        .iter()
        .position(|arg| matches!(arg.as_str(), "-c" | "/C" | "-Command"))
        && let Some(command) = args.get(flag_index + 1)
        && !command.trim().is_empty()
    {
        return command.clone();
    }

    let mut parts = Vec::with_capacity(1 + args.len());
    if !session.command.trim().is_empty() {
        parts.push(session.command.as_str());
    }
    for arg in args {
        if !arg.trim().is_empty() {
            parts.push(arg.as_str());
        }
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        shell_words::join(parts)
    }
}

fn attach_pty_response_context(
    response: &mut Value,
    session_id: &str,
    command: &str,
    working_directory: Option<&str>,
    rows: u16,
    cols: u16,
    is_exited: bool,
) {
    response["id"] = json!(session_id);
    response["session_id"] = json!(session_id);
    response["command"] = json!(command);
    response["working_directory"] = working_directory
        .map(|value| json!(value))
        .unwrap_or(Value::Null);
    response["rows"] = json!(rows);
    response["cols"] = json!(cols);
    response["is_exited"] = json!(is_exited);
}

fn extract_run_session_id_from_tool_output_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    let session_id = file_name.strip_suffix(".txt")?;
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn extract_run_session_id_from_read_file_error(error_message: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_message.find(marker)? + marker.len();
    let rest = &error_message[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn build_read_pty_fallback_args(args: &Value, error_message: &str) -> Option<Value> {
    let session_id = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("target_path"))
        .and_then(Value::as_str)
        .and_then(extract_run_session_id_from_tool_output_path)
        .or_else(|| extract_run_session_id_from_read_file_error(error_message))?;

    let mut payload = serde_json::Map::new();
    payload.insert("session_id".to_string(), json!(session_id));

    if let Some(yield_time_ms) = args.get("yield_time_ms").cloned() {
        payload.insert("yield_time_ms".to_string(), yield_time_ms);
    }

    Some(Value::Object(payload))
}

impl ToolRegistry {
    pub(super) fn unified_exec_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_exec(args).await })
    }

    pub(super) fn unified_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_file(args).await })
    }

    pub(super) fn unified_search_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_search(args).await })
    }

    pub(super) async fn execute_unified_exec(&self, args: Value) -> Result<Value> {
        let mut args = args;
        if let Some(payload) = args.as_object_mut() {
            if payload.get("command").is_none() {
                if let Some(cmd) = payload.get("cmd").cloned() {
                    payload.insert("command".to_string(), cmd);
                } else if let Some(raw) = payload.get("raw_command").cloned() {
                    payload.insert("command".to_string(), raw);
                }
            }
            if payload.get("input").is_none() {
                if let Some(chars) = payload.get("chars").cloned() {
                    payload.insert("input".to_string(), chars);
                } else if let Some(text) = payload.get("text").cloned() {
                    payload.insert("input".to_string(), text);
                }
            }
        }

        let action_str = tool_intent::unified_exec_action(&args)
            .ok_or_else(|| missing_unified_exec_action_error(&args))?;
        let action: UnifiedExecAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;

        match action {
            UnifiedExecAction::Run => self.execute_run_pty_cmd(args).await,
            UnifiedExecAction::Write => self.execute_send_pty_input(args).await,
            UnifiedExecAction::Poll => self.execute_read_pty_session(args).await,
            UnifiedExecAction::List => self.execute_list_pty_sessions().await,
            UnifiedExecAction::Close => self.execute_close_pty_session(args).await,
            UnifiedExecAction::Code => self.execute_code(args).await,
        }
    }

    pub(super) async fn execute_unified_file(&self, args: Value) -> Result<Value> {
        let action_str = tool_intent::unified_file_action(&args)
            .ok_or_else(|| missing_unified_file_action_error(&args))?;

        let action: UnifiedFileAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;
        self.log_unified_file_payload_diagnostics(action_str, &args);

        match action {
            UnifiedFileAction::Read => {
                let tool = self.inventory.file_ops_tool().clone();
                match tool.read_file(args.clone()).await {
                    Ok(response) => Ok(response),
                    Err(read_err) => {
                        let read_err_text = read_err.to_string();
                        if let Some(fallback_args) =
                            build_read_pty_fallback_args(&args, &read_err_text)
                        {
                            let session_id = fallback_args
                                .get("session_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            tracing::info!(
                                session_id = %session_id,
                                "Auto-recovering unified_file read via read_pty_session"
                            );
                            match self.execute_read_pty_session(fallback_args).await {
                                Ok(mut recovered) => {
                                    if let Some(obj) = recovered.as_object_mut() {
                                        obj.insert("auto_recovered".to_string(), json!(true));
                                        obj.insert(
                                            "recovery_tool".to_string(),
                                            json!("read_pty_session"),
                                        );
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
                                        "Failed auto-recovery via read_pty_session"
                                    );
                                }
                            }
                        }
                        Err(read_err)
                    }
                }
            }
            UnifiedFileAction::Write => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.write_file(args).await
            }
            UnifiedFileAction::Edit => self.edit_file(args).await,
            UnifiedFileAction::Patch => self.execute_apply_patch(args).await,
            UnifiedFileAction::Delete => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.delete_file(args).await
            }
            UnifiedFileAction::Move => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.move_file(args).await
            }
            UnifiedFileAction::Copy => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.copy_file(args).await
            }
        }
    }

    pub(super) async fn execute_unified_search(&self, args: Value) -> Result<Value> {
        let mut args = args;

        let action_str = tool_intent::unified_search_action(&args)
            .ok_or_else(|| missing_unified_search_action_error(&args))?;

        let action: UnifiedSearchAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;

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
            UnifiedSearchAction::Intelligence => Ok(
                serde_json::json!({"error": "Code intelligence (tree-sitter) has been removed. Use grep/search tools instead."}),
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

        let language_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("python3");

        let language = match language_str {
            "python3" | "python" => Language::Python3,
            "javascript" | "js" => Language::JavaScript,
            _ => Language::Python3,
        };

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
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing url in web_fetch"))?;

        let raw = args.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("VT Code/1.0")
            .build()?;

        let response = client.get(url).send().await?;
        let status = response.status();

        if !status.is_success() {
            return Err(anyhow!("Web fetch failed with status: {}", status));
        }

        if raw {
            let body = response.text().await?;
            Ok(json!({ "success": true, "content": body, "url": url }))
        } else {
            let body = response.text().await?;
            // Fallback to raw content if html2md is not available
            Ok(json!({ "success": true, "content": body, "url": url }))
        }
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
        let patch_source =
            patch_source_from_args(&args).ok_or_else(|| anyhow!("Missing patch input"))?;
        let patch_input_bytes = patch_source.len();
        let patch_base64 = patch_source.starts_with("base64:");

        let patch_content = if patch_base64 {
            let b64 = &patch_source[7..];
            let decoded = BASE64
                .decode(b64)
                .with_context(|| "Failed to decode base64 patch")?;
            String::from_utf8(decoded).with_context(|| "Decoded patch is not valid UTF-8")?
        } else {
            patch_source.to_string()
        };

        let mut patch_args = args.clone();
        patch_args["input"] = json!(patch_content);
        let context = self.harness_context_snapshot();
        tracing::debug!(
            tool = "unified_file",
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

    fn log_unified_file_payload_diagnostics(&self, action: &str, args: &Value) {
        let context = self.harness_context_snapshot();
        let (patch_source_bytes, patch_base64) = patch_source_from_args(args)
            .map(|source| (source.len(), source.starts_with("base64:")))
            .unwrap_or((0, false));

        tracing::debug!(
            tool = "unified_file",
            action,
            payload_bytes = serialized_payload_size_bytes(args),
            patch_source_bytes,
            patch_base64,
            session_id = %context.session_id,
            task_id = %context.task_id.as_deref().unwrap_or(""),
            "Captured unified_file payload diagnostics"
        );
    }

    // ============================================================
    // SPECIALIZED EXECUTORS (Hidden from LLM, used by unified tools)
    // ============================================================

    pub(super) fn read_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn edit_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn grep_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let manager = self.inventory.grep_file_manager();
        Box::pin(async move {
            manager
                .perform_search(serde_json::from_value(args)?)
                .await
                .map(|r| json!(r))
        })
    }

    pub(super) fn list_files_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.execute(args).await })
    }

    pub(super) fn run_pty_cmd_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_run_pty_cmd(args).await })
    }

    pub(super) fn send_pty_input_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_send_pty_input(args).await })
    }

    pub(super) fn read_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_read_pty_session(args).await })
    }

    pub(super) fn list_pty_sessions_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_list_pty_sessions().await })
    }

    pub(super) fn close_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_close_pty_session(args).await })
    }

    pub(super) fn get_errors_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_get_errors(args).await })
    }

    pub(super) fn agent_info_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_agent_info().await })
    }

    pub(super) fn search_tools_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_search_tools(args).await })
    }

    pub(super) fn apply_patch_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch_internal(args).await })
    }

    // ============================================================
    // INTERNAL IMPLEMENTATIONS
    // ============================================================

    async fn execute_run_pty_cmd(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("run_pty_cmd requires a JSON object"))?;

        let (mut command, auto_raw_command) = parse_command_parts(
            payload,
            "run_pty_cmd requires a 'command' value",
            "PTY command cannot be empty",
        )?;
        let requested_command = command.clone();
        let is_git_diff = is_git_diff_command(&command);

        let shell_program = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        );
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let confirm = payload
            .get("confirm")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let normalized_shell = normalized_shell_name(&shell_program);
        let existing_shell = command
            .first()
            .map(|existing| normalized_shell_name(existing));

        if existing_shell != Some(normalized_shell.clone()) {
            // Prefer explicit raw_command, fallback to auto-detected from string command
            let raw_command = payload
                .get("raw_command")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or(auto_raw_command);

            let command_string =
                build_shell_command_string(raw_command.as_deref(), &command, &shell_program);

            let mut shell_invocation = Vec::with_capacity(4);
            shell_invocation.push(shell_program.clone());

            if login_shell && !should_use_windows_command_tokenizer(Some(&shell_program)) {
                shell_invocation.push("-l".to_string());
            }

            let command_flag = if should_use_windows_command_tokenizer(Some(&shell_program)) {
                match normalized_shell.as_str() {
                    "cmd" | "cmd.exe" => "/C".to_string(),
                    "powershell" | "powershell.exe" | "pwsh" => "-Command".to_string(),
                    _ => "-c".to_string(),
                }
            } else {
                "-c".to_string()
            };

            shell_invocation.push(command_flag);
            shell_invocation.push(command_string);
            command = shell_invocation;
        }

        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(payload.get("working_dir").and_then(|value| value.as_str()))
            .await?;

        let display_command = if should_use_windows_command_tokenizer(Some(&shell_program)) {
            join_windows_command(&command)
        } else {
            shell_words::join(command.iter().map(|part| part.as_str()))
        };
        let requested_command_display =
            if should_use_windows_command_tokenizer(Some(&shell_program)) {
                join_windows_command(&requested_command)
            } else {
                shell_words::join(requested_command.iter().map(|part| part.as_str()))
            };

        // Use explicit max_tokens if provided, otherwise check if command suggests a limit
        let max_tokens = payload
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .or_else(|| suggest_max_tokens_for_command(&display_command))
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS);

        enforce_pty_command_policy(&display_command, confirm)?;

        let yield_duration = payload
            .get("yield_time_ms")
            .and_then(|v| v.as_u64())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_secs(10));

        let _session_guard = self
            .start_pty_session()
            .context("Maximum PTY sessions reached; cannot start new session")?;

        self.increment_active_pty_sessions();

        let session_id = generate_session_id("run");

        let session_metadata = self.pty_manager().create_session(
            session_id.clone(),
            command,
            working_dir_path,
            portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            },
        )?;

        let mut output = String::new();
        let mut truncated = false;
        let max_output_len = max_tokens.saturating_mul(4);
        let timeout_seconds = self.pty_config().command_timeout_seconds;
        let mut exit_code = None;
        let start = Instant::now();
        let allow_polling = !yield_duration.is_zero();

        loop {
            if timeout_seconds > 0 && start.elapsed() >= Duration::from_secs(timeout_seconds) {
                break;
            }

            let remaining = if timeout_seconds > 0 {
                Duration::from_secs(timeout_seconds).saturating_sub(start.elapsed())
            } else {
                yield_duration
            };
            let wait_duration = if allow_polling {
                yield_duration.min(remaining)
            } else {
                Duration::ZERO
            };

            let capture = self.wait_for_pty_yield(&session_id, wait_duration).await;
            if !truncated {
                let cleaned_output = filter_pty_output(&strip_ansi(&capture.output));
                output.push_str(&cleaned_output);
                if max_tokens > 0 && output.len() > max_output_len {
                    output.truncate(max_output_len);
                    output.push_str("\n[Output truncated]");
                    truncated = true;
                }
            }

            if let Some(code) = capture.exit_code {
                exit_code = Some(code);
                break;
            }

            if !allow_polling {
                break;
            }
        }

        let wall_time = start.elapsed().as_secs_f64();
        let mut response = json!({
            "output": output,
            "wall_time": wall_time,
        });
        attach_pty_response_context(
            &mut response,
            &session_id,
            &requested_command_display,
            session_metadata.working_dir.as_deref(),
            session_metadata.rows,
            session_metadata.cols,
            exit_code.is_some(),
        );

        if let Some(code) = exit_code {
            response["exit_code"] = json!(code);
            self.decrement_active_pty_sessions();
        } else {
            response["process_id"] = json!(session_id);
        }

        if truncated {
            response["truncated"] = json!(true);
        }
        if truncated || exit_code.is_none() {
            response["follow_up_prompt"] = json!(format!(
                "Command output incomplete. Read more with read_pty_session session_id=\"{}\" before rerunning the command.",
                session_id
            ));
        }
        if is_git_diff {
            response["no_spool"] = json!(true);
            response["content_type"] = json!("git_diff");
        }

        Ok(response)
    }

    async fn execute_send_pty_input(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("send_pty_input requires a JSON object"))?;

        let sid = payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session_id is required for 'send_pty_input'"))?;

        let input = payload
            .get("input")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("input is required for 'send_pty_input'"))?;

        let yield_time_ms = payload
            .get("yield_time_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(250);

        let max_tokens = payload
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS);
        let session_metadata = self.pty_manager().snapshot_session(sid)?;
        let session_command = build_session_command_display(&session_metadata);

        self.pty_manager()
            .send_input_to_session(sid, input.as_bytes(), false)?;

        let capture = self
            .wait_for_pty_yield(sid, Duration::from_millis(yield_time_ms))
            .await;

        let mut output = filter_pty_output(&strip_ansi(&capture.output));
        let mut truncated = false;

        if max_tokens > 0 && output.len() > max_tokens * 4 {
            output.truncate(max_tokens * 4);
            output.push_str("\n[Output truncated]");
            truncated = true;
        }

        let mut response = json!({
            "output": output,
            "wall_time": capture.duration.as_secs_f64(),
        });
        attach_pty_response_context(
            &mut response,
            sid,
            &session_command,
            session_metadata.working_dir.as_deref(),
            session_metadata.rows,
            session_metadata.cols,
            capture.exit_code.is_some(),
        );

        if let Some(code) = capture.exit_code {
            response["exit_code"] = json!(code);
            self.decrement_active_pty_sessions();
        } else {
            response["session_id"] = json!(sid);
        }

        if truncated {
            response["truncated"] = json!(true);
        }
        if truncated || capture.exit_code.is_none() {
            response["follow_up_prompt"] = json!(format!(
                "Command output incomplete. Read more with read_pty_session session_id=\"{}\" before rerunning the command.",
                sid
            ));
        }

        Ok(response)
    }

    async fn execute_read_pty_session(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("read_pty_session requires a JSON object"))?;

        let raw_sid = payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session_id is required for 'read_pty_session'"))?;
        let sid = raw_sid.trim();

        if !is_valid_pty_session_id(sid) {
            return Err(anyhow!(
                "Invalid session_id for 'read_pty_session': '{}'. Expected an ASCII token (letters, digits, '-', '_').",
                raw_sid
            ));
        }
        let session_metadata = self.pty_manager().snapshot_session(sid)?;
        let session_command = build_session_command_display(&session_metadata);

        let yield_time_ms = payload
            .get("yield_time_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);

        let capture = self
            .wait_for_pty_yield(sid, Duration::from_millis(yield_time_ms))
            .await;

        let output = filter_pty_output(&strip_ansi(&capture.output));

        let mut response = json!({
            "output": output,
            "wall_time": capture.duration.as_secs_f64(),
        });
        attach_pty_response_context(
            &mut response,
            sid,
            &session_command,
            session_metadata.working_dir.as_deref(),
            session_metadata.rows,
            session_metadata.cols,
            capture.exit_code.is_some(),
        );

        if let Some(code) = capture.exit_code {
            response["exit_code"] = json!(code);
            self.decrement_active_pty_sessions();
        } else {
            response["session_id"] = json!(sid);
        }

        Ok(response)
    }

    async fn execute_list_pty_sessions(&self) -> Result<Value> {
        let sessions = self.pty_manager().list_sessions();
        Ok(json!({ "sessions": sessions }))
    }

    async fn execute_close_pty_session(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("close_pty_session requires a JSON object"))?;

        let sid = payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session_id is required for 'close_pty_session'"))?;

        self.pty_manager().close_session(sid)?;
        self.decrement_active_pty_sessions();

        Ok(json!({ "success": true, "session_id": sid }))
    }

    async fn execute_get_errors(&self, args: Value) -> Result<Value> {
        // Simplified version of get_errors logic
        let scope = args
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("archive");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

        let mut error_report = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "scope": scope,
            "total_errors": 0,
            "recent_errors": Vec::<Value>::new(),
        });

        if scope == "archive" || scope == "all" {
            let sessions = crate::utils::session_archive::list_recent_sessions(limit).await?;
            let mut issues = Vec::new();
            let mut total_errors = 0usize;

            for listing in sessions {
                for message in listing.snapshot.messages {
                    if message.role == crate::llm::provider::MessageRole::Assistant {
                        let text = message.content.as_text();
                        let lower = text.to_lowercase();
                        let error_patterns = crate::tools::constants::ERROR_DETECTION_PATTERNS;

                        if error_patterns.iter().any(|&pat| lower.contains(pat)) {
                            total_errors += 1;
                            issues.push(json!({
                                "type": "session_error",
                                "message": text.trim(),
                                "timestamp": listing.snapshot.ended_at.to_rfc3339(),
                            }));
                        }
                    }
                }
            }

            error_report["recent_errors"] = json!(issues);
            error_report["total_errors"] = json!(total_errors);
        }

        Ok(error_report)
    }

    async fn execute_agent_info(&self) -> Result<Value> {
        let available_tools = self.available_tools().await;
        Ok(json!({
            "tools_registered": available_tools,
            "workspace_root": self.workspace_root_str(),
            "available_tools_count": available_tools.len(),
            "agent_type": self.agent_type,
        }))
    }

    async fn execute_search_tools(&self, args: Value) -> Result<Value> {
        let keyword = args
            .get("keyword")
            .or_else(|| args.get("query"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let detail_level_str = args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .unwrap_or("name-and-description");
        let detail_level = match detail_level_str {
            "name-only" => DetailLevel::NameOnly,
            "full" => DetailLevel::Full,
            _ => DetailLevel::NameAndDescription,
        };

        // 1. Search local tools and aliases
        let mut results = Vec::new();
        let available_tools = self.available_tools().await;

        for tool_name in available_tools {
            // Skip MCP tools as they will be handled by ToolDiscovery
            if tool_name.starts_with("mcp_") {
                continue;
            }

            // Get description from inventory if available
            let description = if let Some(reg) = self.inventory.get_registration(&tool_name) {
                reg.metadata().description().unwrap_or("").to_string()
            } else {
                "".to_string()
            };

            if keyword.is_empty()
                || tool_name.to_lowercase().contains(&keyword.to_lowercase())
                || description.to_lowercase().contains(&keyword.to_lowercase())
            {
                results.push(json!({
                    "name": tool_name,
                    "provider": "builtin",
                    "description": description,
                }));
            }
        }

        // 2. Search MCP tools using ToolDiscovery
        if let Some(mcp_client) = self.mcp_client() {
            let discovery = ToolDiscovery::new(mcp_client);
            if let Ok(mcp_results) = discovery.search_tools(keyword, detail_level).await {
                for r in mcp_results {
                    results.push(r.to_json(detail_level));
                }
            }
        }

        // 3. Search skills
        let skill_manager = self.inventory.skill_manager();
        if let Ok(skills) = skill_manager.list_skills().await {
            for skill in skills {
                if keyword.is_empty()
                    || skill.name.to_lowercase().contains(&keyword.to_lowercase())
                    || skill
                        .description
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                {
                    results.push(json!({
                        "name": skill.name,
                        "provider": "skill",
                        "description": skill.description,
                    }));
                }
            }
        }

        Ok(json!({ "tools": results }))
    }

    async fn execute_apply_patch_internal(&self, args: Value) -> Result<Value> {
        let patch_source = patch_source_from_args(&args)
            .ok_or_else(|| anyhow!("Missing patch input (use 'input' or 'patch' parameter)"))?;

        let patch = crate::tools::editing::Patch::parse(patch_source)?;
        let results = patch.apply(&self.workspace_root_owned()).await?;

        Ok(json!({
            "success": true,
            "applied": results,
        }))
    }

    async fn wait_for_pty_yield(
        &self,
        session_id: &str,
        yield_duration: Duration,
    ) -> PtyEphemeralCapture {
        let mut output = String::new();
        let start = Instant::now();
        let poll_interval = Duration::from_millis(50);

        // Get the progress callback for streaming output to the TUI
        let progress_callback = self.progress_callback();
        let tool_name = "run_pty_cmd";

        // Throttle TUI updates to prevent excessive redraws
        let mut last_ui_update = Instant::now();
        let ui_update_interval = Duration::from_millis(100);
        let mut pending_lines = String::new();

        loop {
            if let Ok(Some(code)) = self.pty_manager().is_session_completed(session_id) {
                if let Ok(Some(final_output)) =
                    self.pty_manager().read_session_output(session_id, true)
                {
                    output.push_str(&final_output);

                    // Stream final output to TUI
                    if let Some(ref callback) = progress_callback {
                        pending_lines.push_str(&final_output);
                        if !pending_lines.is_empty() {
                            callback(tool_name, &pending_lines);
                        }
                    }
                }
                return PtyEphemeralCapture {
                    output,
                    exit_code: Some(code),
                    duration: start.elapsed(),
                };
            }

            if let Ok(Some(new_output)) = self.pty_manager().read_session_output(session_id, true) {
                output.push_str(&new_output);
                pending_lines.push_str(&new_output);

                // Stream output to TUI with throttling
                if let Some(ref callback) = progress_callback {
                    let now = Instant::now();
                    // Flush pending lines if interval elapsed or if we have a complete line
                    if now.duration_since(last_ui_update) >= ui_update_interval
                        || pending_lines.contains('\n')
                    {
                        if !pending_lines.is_empty() {
                            callback(tool_name, &pending_lines);
                            pending_lines.clear();
                            last_ui_update = now;
                        }
                    }
                }
            }

            if start.elapsed() >= yield_duration {
                // Flush any remaining pending lines
                if let Some(ref callback) = progress_callback {
                    if !pending_lines.is_empty() {
                        callback(tool_name, &pending_lines);
                    }
                }
                return PtyEphemeralCapture {
                    output,
                    exit_code: None,
                    duration: start.elapsed(),
                };
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

// Helper functions and structs for PTY execution

struct PtyEphemeralCapture {
    output: String,
    exit_code: Option<i32>,
    duration: Duration,
}

fn parse_command_parts(
    payload: &serde_json::Map<String, Value>,
    missing_error: &str,
    empty_error: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let (mut parts, raw_command) = match payload.get("command") {
        Some(Value::String(command)) => {
            // Preserve the original command string to avoid splitting shell operators
            let parts = shell_words::split(command).context("Failed to parse command string")?;
            (parts, Some(command.to_string()))
        }
        Some(Value::Array(values)) => {
            let parts = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|part| part.to_string())
                        .ok_or_else(|| anyhow!("command array must contain only strings"))
                })
                .collect::<Result<Vec<_>>>()?;
            (parts, None)
        }
        _ => return Err(anyhow!("{}", missing_error)),
    };

    if let Some(args_value) = payload.get("args") {
        if let Some(args_array) = args_value.as_array() {
            for value in args_array {
                if let Some(part) = value.as_str() {
                    parts.push(part.to_string());
                }
            }
        }
    }

    if parts.is_empty() {
        return Err(anyhow!("{}", empty_error));
    }

    Ok((parts, raw_command))
}

fn is_git_diff_command(parts: &[String]) -> bool {
    let Some(first) = parts.first() else {
        return false;
    };
    let basename = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first.as_str())
        .to_ascii_lowercase();
    if basename != "git" && basename != "git.exe" {
        return false;
    }
    parts.iter().skip(1).any(|part| part == "diff")
}

fn resolve_shell_preference(pref: Option<&str>, config: &crate::config::PtyConfig) -> String {
    pref.map(|s| s.to_string()).unwrap_or_else(|| {
        config
            .preferred_shell
            .clone()
            .unwrap_or_else(|| "sh".to_string())
    })
}

fn normalized_shell_name(shell: &str) -> String {
    PathBuf::from(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(shell)
        .to_lowercase()
}

fn build_shell_command_string(raw: Option<&str>, parts: &[String], _shell: &str) -> String {
    raw.map(|s| s.to_string())
        .unwrap_or_else(|| shell_words::join(parts.iter().map(|s| s.as_str())))
}

/// Check if a command is a file display command that should have limited output.
/// Returns suggested max_tokens if the command is a file display command without explicit limits.
pub fn suggest_max_tokens_for_command(cmd: &str) -> Option<usize> {
    let trimmed = cmd.trim().to_lowercase();

    // Skip if command already has output limiting
    if trimmed.contains("head") || trimmed.contains("tail") || trimmed.contains("| ") {
        return None;
    }

    // File display commands that benefit from token limits
    let file_display_cmds = ["cat ", "bat ", "type "]; // type for Windows

    for prefix in &file_display_cmds {
        if trimmed.starts_with(prefix) {
            // Suggest 250 tokens (~1000 chars) for file preview
            return Some(250);
        }
    }

    None
}

fn should_use_windows_command_tokenizer(shell: Option<&str>) -> bool {
    if cfg!(windows) {
        if let Some(s) = shell {
            let lower = s.to_lowercase();
            return lower.contains("cmd") || lower.contains("powershell") || lower.contains("pwsh");
        }
        return true;
    }
    false
}

fn join_windows_command(parts: &[String]) -> String {
    parts.join(" ")
}

fn parse_pty_dimension(name: &str, value: Option<&Value>, default: u16) -> Result<u16> {
    match value {
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| anyhow!("{} must be a number", name))?;
            Ok(n as u16)
        }
        None => Ok(default),
    }
}

fn generate_session_id(prefix: &str) -> String {
    format!(
        "{}-{}",
        prefix,
        uuid::Uuid::new_v4().to_string()[..8].to_string()
    )
}

fn strip_ansi(text: &str) -> String {
    crate::utils::ansi_parser::strip_ansi(text)
}

fn filter_pty_output(text: &str) -> String {
    text.to_string()
}

// Conservative PTY command policy inspired by bash allow/deny defaults.
const PTY_DENY_PREFIXES: &[&str] = &[
    "bash -i",
    "sh -i",
    "zsh -i",
    "fish -i",
    "python -i",
    "python3 -i",
    "ipython",
    "nano",
    "vim",
    "vi",
    "emacs",
    "top",
    "htop",
    "less",
    "more",
    "screen",
    "tmux",
];

const PTY_DENY_STANDALONE: &[&str] = &["python", "python3", "bash", "sh", "zsh", "fish"];

#[allow(dead_code)]
const PTY_ALLOW_PREFIXES: &[&str] = &[
    "pwd",
    "whoami",
    "ls",
    "git status",
    "git diff",
    "git log",
    "stat",
    "which",
    "echo",
    "cat",
];

fn enforce_pty_command_policy(display_command: &str, confirm: bool) -> Result<()> {
    let lower = display_command.to_ascii_lowercase();
    let trimmed = lower.trim();
    let is_standalone = trimmed.split_whitespace().count() == 1;

    let deny_match = PTY_DENY_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let standalone_denied = is_standalone && PTY_DENY_STANDALONE.contains(&trimmed);

    if deny_match || standalone_denied {
        if confirm {
            return Ok(());
        }
        return Err(anyhow!(
            "Command '{}' is blocked by PTY safety policy. Set confirm=true to force execution.",
            display_command
        ));
    }

    // Allowlisted commands are simply allowed; we rely on general policy for others.
    Ok(())
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
mod pty_context_tests {
    use super::{attach_pty_response_context, build_session_command_display};
    use crate::tools::types::VTCodePtySession;
    use serde_json::json;

    #[test]
    fn build_session_command_display_unwraps_shell_c_argument() {
        let session = VTCodePtySession {
            id: "run-123".to_string(),
            command: "zsh".to_string(),
            args: vec![
                "-l".to_string(),
                "-c".to_string(),
                "cargo check".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: 24,
            cols: 80,
            screen_contents: None,
            scrollback: None,
        };

        assert_eq!(build_session_command_display(&session), "cargo check");
    }

    #[test]
    fn attach_pty_response_context_sets_expected_keys() {
        let mut response = json!({ "output": "ok" });
        attach_pty_response_context(
            &mut response,
            "run-123",
            "cargo check",
            Some("."),
            30,
            120,
            false,
        );

        assert_eq!(response["id"], "run-123");
        assert_eq!(response["session_id"], "run-123");
        assert_eq!(response["command"], "cargo check");
        assert_eq!(response["working_directory"], ".");
        assert_eq!(response["rows"], 30);
        assert_eq!(response["cols"], 120);
        assert_eq!(response["is_exited"], false);
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
        extract_run_session_id_from_read_file_error, extract_run_session_id_from_tool_output_path,
        missing_unified_exec_action_error, missing_unified_search_action_error,
        patch_source_from_args, summarized_arg_keys,
    };
    use serde_json::json;

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
        assert!(text.contains("Missing action in unified_exec"));
        assert!(text.contains("foo"));
        assert!(text.contains("session_id"));
    }

    #[test]
    fn unified_search_missing_action_error_includes_received_keys() {
        let err = missing_unified_search_action_error(&json!({
            "unexpected": true
        }));
        let text = err.to_string();
        assert!(text.contains("Missing action in unified_search"));
        assert!(text.contains("unexpected"));
    }

    #[test]
    fn patch_source_accepts_raw_string_and_object_fields() {
        assert_eq!(
            patch_source_from_args(&json!("*** Begin Patch\n*** End Patch\n")),
            Some("*** Begin Patch\n*** End Patch\n")
        );
        assert_eq!(patch_source_from_args(&json!({"input": "x"})), Some("x"));
        assert_eq!(patch_source_from_args(&json!({"patch": "y"})), Some("y"));
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
        let error = "Use read_pty_session with session_id=\"run-zz9\" instead of read_file.";
        assert_eq!(
            extract_run_session_id_from_read_file_error(error),
            Some("run-zz9".to_string())
        );
        assert_eq!(
            extract_run_session_id_from_read_file_error("no session"),
            None
        );
    }
}
