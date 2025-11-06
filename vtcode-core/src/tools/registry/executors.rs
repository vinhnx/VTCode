use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures::future::BoxFuture;
use portable_pty::PtySize;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use shell_words::{join, split};
use std::{
    borrow::Cow,
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing::{debug, trace, warn};
use vte::{Parser, Perform};

use crate::config::PtyConfig;
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
                #[serde(default)]
                respect_ignore_files: Option<bool>,
                #[serde(default)]
                max_file_size: Option<usize>,
                #[serde(default)]
                search_hidden: Option<bool>,
                #[serde(default)]
                search_binary: Option<bool>,
                #[serde(default)]
                files_with_matches: Option<bool>,
                #[serde(default)]
                type_pattern: Option<String>,
                #[serde(default)]
                invert_match: Option<bool>,
                #[serde(default)]
                word_boundaries: Option<bool>,
                #[serde(default)]
                line_number: Option<bool>,
                #[serde(default)]
                column: Option<bool>,
                #[serde(default)]
                only_matching: Option<bool>,
                #[serde(default)]
                trim: Option<bool>,
            }

            fn default_grep_path() -> String {
                ".".to_string()
            }

            let payload: GrepArgs =
                serde_json::from_value(args).context("grep_file requires a 'pattern' field")?;

            // Validate the path parameter to avoid security issues
            if payload.path.contains("..") || payload.path.starts_with('/') {
                return Err(anyhow!(
                    "Path must be a relative path and cannot contain '..' or start with '/'"
                ));
            }

            // Validate and enforce hard limits
            if let Some(max_results) = payload.max_results {
                // Enforce a reasonable upper limit to prevent excessive resource usage
                const MAX_ALLOWED_RESULTS: usize = 1000;
                if max_results > MAX_ALLOWED_RESULTS {
                    return Err(anyhow!(
                        "max_results ({}) exceeds the maximum allowed value of {}",
                        max_results,
                        MAX_ALLOWED_RESULTS
                    ));
                }
                if max_results == 0 {
                    return Err(anyhow!("max_results must be greater than 0"));
                }
            }

            if let Some(max_file_size) = payload.max_file_size {
                // Enforce a reasonable upper limit for file size (100MB)
                const MAX_ALLOWED_FILE_SIZE: usize = 100 * 1024 * 1024; // 100MB in bytes
                if max_file_size > MAX_ALLOWED_FILE_SIZE {
                    return Err(anyhow!(
                        "max_file_size ({}) exceeds the maximum allowed value of {} bytes (100MB)",
                        max_file_size,
                        MAX_ALLOWED_FILE_SIZE
                    ));
                }
                if max_file_size == 0 {
                    return Err(anyhow!("max_file_size must be greater than 0"));
                }
            }

            // Validate context_lines to prevent excessive context
            if let Some(context_lines) = payload.context_lines {
                const MAX_ALLOWED_CONTEXT: usize = 20; // Increased from 10 to 20 for more flexibility
                if context_lines > MAX_ALLOWED_CONTEXT {
                    return Err(anyhow!(
                        "context_lines ({}) exceeds the maximum allowed value of {}",
                        context_lines,
                        MAX_ALLOWED_CONTEXT
                    ));
                }
                if (context_lines as i32) < 0 {
                    return Err(anyhow!("context_lines must not be negative"));
                }
            }

            // Validate glob_pattern for security
            if let Some(glob_pattern) = &payload.glob_pattern {
                if glob_pattern.contains("..") || glob_pattern.starts_with('/') {
                    return Err(anyhow!(
                        "glob_pattern must be a relative path and cannot contain '..' or start with '/'"
                    ));
                }
            }

            // Validate type_pattern for basic security (only allow alphanumeric, hyphens, underscores)
            if let Some(type_pattern) = &payload.type_pattern {
                if !type_pattern
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(anyhow!(
                        "type_pattern can only contain alphanumeric characters, hyphens, and underscores"
                    ));
                }
            }

            let input = GrepSearchInput {
                pattern: payload.pattern.clone(),
                path: payload.path.clone(),
                case_sensitive: payload.case_sensitive,
                literal: payload.literal,
                glob_pattern: payload.glob_pattern,
                context_lines: payload.context_lines,
                include_hidden: payload.include_hidden,
                max_results: payload.max_results,
                respect_ignore_files: payload.respect_ignore_files,
                max_file_size: payload.max_file_size,
                search_hidden: payload.search_hidden,
                search_binary: payload.search_binary,
                files_with_matches: payload.files_with_matches,
                type_pattern: payload.type_pattern,
                invert_match: payload.invert_match,
                word_boundaries: payload.word_boundaries,
                line_number: payload.line_number,
                column: payload.column,
                only_matching: payload.only_matching,
                trim: payload.trim,
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
        let delete_ops = patch
            .operations()
            .iter()
            .filter(|op| matches!(op, crate::tools::editing::PatchOperation::DeleteFile { .. }))
            .count();
        let add_ops = patch
            .operations()
            .iter()
            .filter(|op| matches!(op, crate::tools::editing::PatchOperation::AddFile { .. }))
            .count();

        if delete_ops > 0 && add_ops > 0 {
            warn!(
                delete_ops,
                add_ops,
                "apply_patch will delete and recreate files; ensure backups or incremental edits"
            );

            // Emit telemetry event for destructive operation detection
            // This addresses the Codex issue review recommendation to track
            // cascading delete/recreate sequences
            //
            // Reference: docs/research/codex_issue_review.md - apply_patch Tool Reliability
            let affected_files: Vec<String> = patch
                .operations()
                .iter()
                .filter_map(|op| match op {
                    crate::tools::editing::PatchOperation::DeleteFile { path } => {
                        Some(path.clone())
                    }
                    crate::tools::editing::PatchOperation::AddFile { path, .. } => {
                        Some(path.clone())
                    }
                    _ => None,
                })
                .collect();

            // Check if we're in a git repository (simple heuristic for backup detection)
            let has_git_backup = self.workspace_root().join(".git").exists();

            let event = crate::tools::registry::ToolTelemetryEvent::delete_and_recreate_warning(
                "apply_patch",
                affected_files.clone(),
                has_git_backup,
            );

            // Log the telemetry event (structured logging for observability)
            debug!(
                event = ?event,
                "Emitting destructive operation telemetry"
            );

            // TODO: Add confirmation prompt for destructive operations
            //
            // Implementation should:
            // 1. Check if running in interactive mode (not --skip-confirmations)
            // 2. Check if running in TUI mode vs CLI mode
            // 3. In CLI mode: Use dialoguer for confirmation prompt
            // 4. In TUI mode: Use modal confirmation (handled by runloop)
            // 5. Show affected files and backup status in prompt
            // 6. Allow user to abort operation
            //
            // Example implementation:
            // ```rust
            // if !self.skip_confirmations && !has_git_backup {
            //     let prompt_msg = format!(
            //         "apply_patch will delete and recreate {} file(s):\n{}\n\n\
            //          No git backup detected. Continue?",
            //         affected_files.len(),
            //         affected_files.join("\n")
            //     );
            //
            //     // CLI mode confirmation
            //     if !std::env::var("VTCODE_TUI_MODE").is_ok() {
            //         let confirmed = dialoguer::Confirm::new()
            //             .with_prompt(prompt_msg)
            //             .default(false)
            //             .interact()?;
            //         if !confirmed {
            //             return Ok(json!({
            //                 "success": false,
            //                 "error": "Operation cancelled by user"
            //             }));
            //         }
            //     }
            //     // TUI mode: Return special status code for runloop to handle
            //     else {
            //         return Err(anyhow!(
            //             "CONFIRMATION_REQUIRED: {}",
            //             prompt_msg
            //         ));
            //     }
            // }
            // ```
            //
            // Reference: docs/research/codex_issue_review.md - Confirmation prompts
            // Related: src/agent/runloop/unified/tool_routing.rs - ensure_tool_permission()
        }

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

        let results = match patch.apply(self.workspace_root()).await {
            Ok(results) => results,
            Err(err) => {
                warn!(
                    error = %err,
                    "apply_patch failed; consider falling back to incremental edits"
                );
                return Err(err);
            }
        };
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
        let mut command = parse_command_parts(
            payload,
            "run_pty_cmd requires a 'command' value",
            "PTY command cannot be empty",
        )?;

        let raw_command = payload
            .get("raw_command")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let shell = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        );
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if let Some(shell_program) = shell {
            let normalized_shell = normalized_shell_name(&shell_program);
            let existing_shell = command
                .first()
                .map(|existing| normalized_shell_name(existing));
            if existing_shell != Some(normalized_shell.clone()) {
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
        }

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

        let mut command_parts = parse_command_parts(
            payload,
            "create_pty_session requires a 'command' value",
            "PTY session command cannot be empty",
        )?;

        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if let Some(shell_program) = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        ) {
            let should_replace = payload.get("shell").is_some()
                || (command_parts.len() == 1 && is_default_shell_placeholder(&command_parts[0]));
            if should_replace {
                command_parts = vec![shell_program];
            }
        }

        if login_shell
            && !command_parts.is_empty()
            && !should_use_windows_command_tokenizer(Some(&command_parts[0]))
            && !command_parts.iter().skip(1).any(|arg| arg == "-l")
        {
            command_parts.push("-l".to_string());
        }

        // Check if this is a development toolchain command in sandbox mode
        if !command_parts.is_empty() {
            let program = &command_parts[0];
            if crate::tools::pty::is_development_toolchain_command(program) {
                if let Some(_profile) = self.pty_manager().sandbox_profile() {
                    return Err(anyhow!(
                        "{} could not be executed in the sandbox. This may be due to missing {} toolchain support in the current environment.\n\n\
                        Next steps:\n\
                        - Verify that the {} toolchain is installed and accessible.\n\
                        - Disable sandbox with `/sandbox disable` to run development tools with local toolchain access.\n\
                        - Alternatively, run the command directly in your terminal outside VT Code.",
                        program,
                        program,
                        program
                    ));
                }
            }
        }

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

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            command = ?command_parts,
            working_dir = %working_dir.display(),
            rows,
            cols,
            "creating PTY session"
        );

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
            response.insert("output".to_string(), Value::String(strip_ansi(&output)));
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
            response.insert("output".to_string(), Value::String(strip_ansi(&output)));
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
    if let Some(shell) = args.get("shell").cloned() {
        pty_args.insert("shell".to_string(), shell);
    }
    if let Some(login) = args.get("login").cloned() {
        pty_args.insert("login".to_string(), login);
    }
    if let Some(raw_command) = args.get("raw_command").cloned() {
        pty_args.insert("raw_command".to_string(), raw_command);
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
            response.insert(
                "screen_contents".to_string(),
                Value::String(strip_ansi(&screen)),
            );
        }
    }

    if options.include_scrollback {
        if let Some(scrollback) = scrollback {
            response.insert(
                "scrollback".to_string(),
                Value::String(strip_ansi(&scrollback)),
            );
        }
    }

    response
}

fn strip_ansi(text: &str) -> String {
    struct AnsiStripper {
        output: String,
    }

    impl AnsiStripper {
        fn new(capacity: usize) -> Self {
            Self {
                output: String::with_capacity(capacity),
            }
        }
    }

    impl Perform for AnsiStripper {
        fn print(&mut self, c: char) {
            self.output.push(c);
        }

        fn execute(&mut self, byte: u8) {
            match byte {
                b'\n' => self.output.push('\n'),
                b'\r' => self.output.push('\r'),
                b'\t' => self.output.push('\t'),
                _ => {}
            }
        }

        fn hook(
            &mut self,
            _params: &vte::Params,
            _intermediates: &[u8],
            _ignore: bool,
            _action: char,
        ) {
        }

        fn put(&mut self, _byte: u8) {}

        fn unhook(&mut self) {}

        fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

        fn csi_dispatch(
            &mut self,
            _params: &vte::Params,
            _intermediates: &[u8],
            _ignore: bool,
            _action: char,
        ) {
        }

        fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _action: u8) {}
    }

    let mut performer = AnsiStripper::new(text.len());
    let mut parser = Parser::new();

    for byte in text.as_bytes() {
        parser.advance(&mut performer, std::slice::from_ref(byte));
    }

    performer.output
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::{Map, json};

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("hello"), "hello");
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
        assert_eq!(
            strip_ansi("Checking \x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m"),
            "Checking vtcode"
        );
    }

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
    fn windows_join_quotes_arguments_with_spaces() {
        let parts = vec![
            r"C:\Program Files\Git\bin\git.exe".to_string(),
            "--version".to_string(),
        ];
        let joined = join_windows_command(&parts);
        assert_eq!(
            joined,
            r#""C:\Program Files\Git\bin\git.exe" --version"#.to_string()
        );
    }

    #[test]
    fn windows_join_leaves_simple_arguments_unquoted() {
        let parts = vec!["cmd".to_string(), "/C".to_string(), "dir".to_string()];
        let joined = join_windows_command(&parts);
        assert_eq!(joined, "cmd /C dir");
    }

    #[test]
    fn pty_input_prefers_base64_over_plain_text() {
        let mut payload = Map::new();
        payload.insert(
            "session_id".to_string(),
            Value::String("test-session".into()),
        );
        payload.insert("append_newline".to_string(), Value::Bool(false));
        payload.insert("input".to_string(), Value::String("plain".into()));
        let encoded = BASE64.encode(b"decoded");
        payload.insert("input_base64".to_string(), Value::String(encoded));

        let parsed = PtyInputPayload::from_map(&payload).expect("pty payload");
        assert_eq!(parsed.buffer, b"decoded");
        assert!(!parsed.append_newline);
    }

    #[test]
    fn pty_input_rejects_empty_payload_without_newline() {
        let mut payload = Map::new();
        payload.insert(
            "session_id".to_string(),
            Value::String("empty-session".into()),
        );

        let err = PtyInputPayload::from_map(&payload).expect_err("expected failure");
        assert!(
            err.to_string()
                .contains("send_pty_input requires 'input' or 'input_base64'")
        );
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

    #[test]
    fn resolve_shell_preference_uses_explicit_value() {
        let mut config = PtyConfig::default();
        config.preferred_shell = Some("/bin/bash".to_string());
        let resolved = super::resolve_shell_preference(Some("/custom/zsh"), &config);
        assert_eq!(resolved.as_deref(), Some("/custom/zsh"));
    }

    #[test]
    fn resolve_shell_preference_uses_config_value() {
        let mut config = PtyConfig::default();
        config.preferred_shell = Some("/bin/zsh".to_string());
        let resolved = super::resolve_shell_preference(None, &config);
        assert_eq!(resolved.as_deref(), Some("/bin/zsh"));
    }

    #[test]
    fn pty_input_prefers_base64_over_plain_text() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-1",
            "input": "ls",
            "input_base64": BASE64.encode("pwd"),
            "append_newline": false,
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert_eq!(payload.buffer, b"pwd");
        assert!(!payload.append_newline);
    }

    #[test]
    fn pty_input_uses_plain_text_when_base64_missing() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-2",
            "input": "echo hello",
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert_eq!(payload.buffer, b"echo hello");
        assert!(!payload.append_newline);
    }

    #[test]
    fn pty_input_rejects_empty_without_newline() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-3",
            "input": "",
            "append_newline": false,
        })
        .as_object()
        .unwrap()
        .clone();

        let err = PtyInputPayload::from_map(&map).unwrap_err();
        assert!(
            err.to_string()
                .contains("send_pty_input requires 'input' or 'input_base64'")
        );
    }

    #[test]
    fn pty_input_allows_empty_when_newline_requested() {
        let map: Map<String, Value> = json!({
            "session_id": "pty-4",
            "input": "",
            "append_newline": true,
        })
        .as_object()
        .unwrap()
        .clone();

        let payload = PtyInputPayload::from_map(&map).expect("payload");
        assert!(payload.buffer.is_empty());
        assert!(payload.append_newline);
    }
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

        let input_text = map.get("input").and_then(Value::as_str);
        let input_base64_text = map.get("input_base64").and_then(Value::as_str);
        let input_preview = input_text.map(Self::preview_string);
        let input_base64_preview = input_base64_text.map(Self::preview_string);

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            append_newline,
            wait_ms,
            drain_output,
            input_len = input_text.map(|text| text.len()).unwrap_or(0),
            input_preview = input_preview.as_deref(),
            input_base64_len = input_base64_text.map(|text| text.len()).unwrap_or(0),
            input_base64_preview = input_base64_preview.as_deref(),
            "received send_pty_input payload"
        );

        let mut buffer = Vec::new();

        // Prefer input_base64 if present, else use input
        if let Some(encoded) = map
            .get("input_base64")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            let decoded = BASE64_STANDARD
                .decode(encoded.as_bytes())
                .context("input_base64 must be valid base64")?;
            buffer.extend_from_slice(&decoded);
        } else if let Some(text) = map.get("input").and_then(|value| value.as_str()) {
            buffer.extend_from_slice(text.as_bytes());
        }

        debug!(
            target: "vtcode::pty",
            session_id = %session_id,
            buffer_len = buffer.len(),
            buffer_preview = %Self::preview_bytes(&buffer),
            "prepared PTY input buffer"
        );

        if buffer.is_empty() && !append_newline {
            debug!(
                target: "vtcode::pty",
                session_id = %session_id,
                "rejecting empty PTY input without append_newline"
            );
            return Err(anyhow!(
                "send_pty_input requires 'input' or 'input_base64' unless append_newline is true"
            ));
        }

        trace!(
            target: "vtcode::pty",
            session_id = session_id.as_str(),
            append_newline,
            wait_ms,
            drain_output,
            has_input = map.contains_key("input"),
            has_input_base64 = map.contains_key("input_base64"),
            buffer_len = buffer.len(),
            "parsed PTY input payload"
        );

        Ok(Self {
            session_id,
            buffer,
            append_newline,
            wait_ms,
            drain_output,
        })
    }

    fn preview_string(text: &str) -> String {
        const MAX_PREVIEW: usize = 64;
        if text.len() <= MAX_PREVIEW {
            text.to_string()
        } else {
            format!("{}…", &text[..MAX_PREVIEW])
        }
    }

    fn preview_bytes(bytes: &[u8]) -> String {
        const MAX_BYTES: usize = 64;
        if let Ok(text) = std::str::from_utf8(bytes) {
            return Self::preview_string(text);
        }

        let mut hex = String::new();
        for byte in bytes.iter().take(MAX_BYTES / 2) {
            use std::fmt::Write as _;
            let _ = write!(hex, "{:02x}", byte);
        }
        if bytes.len() > MAX_BYTES / 2 {
            hex.push('…');
        }
        format!("hex:{}", hex)
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
    let poll_interval = Duration::from_millis(50);
    let min_wait = Duration::from_millis(200); // Wait at least 200ms for fast commands

    loop {
        if let Ok(Some(new_output)) = manager.read_session_output(session_id, true) {
            if !new_output.is_empty() {
                output.push_str(&new_output);
            }
        }

        if let Ok(Some(code)) = manager.is_session_completed(session_id) {
            completed = true;
            exit_code = Some(code);
            // Drain any remaining output
            if let Ok(Some(final_output)) = manager.read_session_output(session_id, true) {
                output.push_str(&final_output);
            }
            break;
        }

        let elapsed = start.elapsed();

        // For long-running commands, return partial output early
        if elapsed > poll_timeout {
            break;
        }

        // If we have output and minimum wait time passed, check if we should return early
        if !output.is_empty() && elapsed > min_wait {
            // Return early if command is still running and we have output
            // This allows the agent to show progress
            if elapsed > Duration::from_secs(2) {
                break;
            }
        }

        sleep(poll_interval).await;
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
        "output": strip_ansi(&output),
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

fn build_shell_command_string(
    raw_command: Option<&str>,
    parts: &[String],
    shell_hint: &str,
) -> String {
    if let Some(raw) = raw_command {
        return raw.to_string();
    }

    if should_use_windows_command_tokenizer(Some(shell_hint)) {
        return join_windows_command(parts);
    }

    join(parts.iter().map(|part| part.as_str()))
}

fn join_windows_command(parts: &[String]) -> String {
    parts
        .iter()
        .map(|part| quote_windows_argument(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_windows_argument(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }

    let requires_quotes = arg
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '\t');
    if !requires_quotes {
        return arg.to_string();
    }

    let mut result = String::with_capacity(arg.len() + 2);
    result.push('"');

    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
            }
            '"' => {
                result.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    result.extend(std::iter::repeat('\\').take(backslashes));
                    backslashes = 0;
                }
                result.push(ch);
            }
        }
    }

    if backslashes > 0 {
        result.extend(std::iter::repeat('\\').take(backslashes * 2));
    }

    result.push('"');
    result
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

fn resolve_shell_preference(explicit: Option<&str>, config: &PtyConfig) -> Option<String> {
    explicit
        .and_then(sanitize_shell_candidate)
        .or_else(|| {
            config
                .preferred_shell
                .as_deref()
                .and_then(sanitize_shell_candidate)
        })
        .or_else(|| {
            env::var("SHELL")
                .ok()
                .and_then(|value| sanitize_shell_candidate(&value))
        })
        .or_else(detect_posix_shell_candidate)
}

fn sanitize_shell_candidate(shell: &str) -> Option<String> {
    let trimmed = shell.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn detect_posix_shell_candidate() -> Option<String> {
    if cfg!(windows) {
        return None;
    }

    const CANDIDATES: [&str; 6] = [
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/sh",
        "/usr/bin/sh",
    ];

    for candidate in CANDIDATES {
        if Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }

    None
}

fn is_default_shell_placeholder(program: &str) -> bool {
    matches!(normalized_shell_name(program).as_str(), "bash" | "sh")
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
