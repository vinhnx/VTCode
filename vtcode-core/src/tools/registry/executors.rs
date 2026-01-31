use crate::exec::code_executor::Language;
use crate::exec::skill_manager::{Skill, SkillMetadata};
use crate::tools::file_tracker::FileTracker;
use crate::tools::registry::declarations::{
    UnifiedExecAction, UnifiedFileAction, UnifiedSearchAction,
};
use crate::tools::traits::Tool;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use anyhow::{Context, Result, anyhow};
use chrono;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use super::ToolRegistry;

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
        let action_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .or_else(|| {
                if args.get("command").is_some() {
                    Some("run")
                } else if args.get("code").is_some() {
                    Some("code")
                } else if args.get("input").is_some() {
                    Some("write")
                } else if args.get("session_id").is_some() {
                    Some("poll")
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("Missing action in unified_exec"))?;

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
        let action_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .or_else(|| {
                if args.get("old_str").is_some() {
                    Some("edit")
                } else if args.get("patch").is_some() {
                    Some("patch")
                } else if args.get("content").is_some() {
                    Some("write")
                } else if args.get("destination").is_some() {
                    Some("move")
                } else {
                    Some("read")
                }
            })
            .ok_or_else(|| anyhow!("Missing action in unified_file"))?;

        let action: UnifiedFileAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;

        match action {
            UnifiedFileAction::Read => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.read_file(args).await
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

        let action_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .or_else(|| {
                // Smart action inference based on parameters
                if args.get("pattern").is_some() || args.get("query").is_some() {
                    Some("grep")
                } else if args.get("operation").is_some() {
                    Some("intelligence")
                } else if args.get("url").is_some() {
                    Some("web")
                } else if args.get("sub_action").is_some() {
                    Some("skill")
                } else if args.get("scope").is_some() {
                    Some("errors")
                } else if args.get("path").is_some() {
                    Some("list")
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("Missing action in unified_search. Valid actions: grep, list, intelligence, tools, errors, agent, web, skill"))?;

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
            UnifiedSearchAction::Intelligence => self.execute_code_intelligence(args).await,
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
                if !changes.is_empty() {
                    response["file_changes"] = json!(changes);
                }
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
            .user_agent("VTCode/1.0")
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
        let patch_source = args
            .get("input")
            .or_else(|| args.get("patch"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing patch input"))?;

        let patch_content = if patch_source.starts_with("base64:") {
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

        self.execute_tool_ref("apply_patch", &patch_args).await
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

    pub(super) fn code_intelligence_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_code_intelligence(args).await })
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

        self.pty_manager().create_session(
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

        let capture = self.wait_for_pty_yield(&session_id, yield_duration).await;

        let mut output = filter_pty_output(&strip_ansi(&capture.output));
        let mut truncated = false;

        if max_tokens > 0 && output.len() > max_tokens * 4 {
            output.truncate(max_tokens * 4);
            output.push_str("\n[Output truncated]");
            truncated = true;
        }

        let wall_time = capture.duration.as_secs_f64();
        let mut response = json!({
            "output": output,
            "wall_time": wall_time,
        });

        if let Some(code) = capture.exit_code {
            response["exit_code"] = json!(code);
            self.decrement_active_pty_sessions();
        } else {
            response["process_id"] = json!(session_id);
        }

        if truncated {
            response["truncated"] = json!(true);
        }
        if truncated || capture.exit_code.is_none() {
            response["follow_up_prompt"] = json!(format!(
                "Command output incomplete. Read more with read_pty_session session_id=\"{}\" before rerunning the command.",
                session_id
            ));
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

        let sid = payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("session_id is required for 'read_pty_session'"))?;

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

    async fn execute_code_intelligence(&self, args: Value) -> Result<Value> {
        let tool = self.inventory.code_intelligence_tool();
        tool.execute(args).await
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
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let available_tools = self.available_tools().await;

        let filtered: Vec<String> = available_tools
            .into_iter()
            .filter(|t| t.contains(query))
            .collect();

        Ok(json!({ "tools": filtered }))
    }

    async fn execute_apply_patch_internal(&self, args: Value) -> Result<Value> {
        let patch_source = args
            .get("input")
            .or_else(|| args.get("patch"))
            .or_else(|| args.get("diff"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing patch input"))?;

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
