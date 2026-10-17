//! Shell command handler (from Codex pattern).
//!
//! Executes shell commands with sandbox support, timeout handling,
//! and environment policy management.

use hashbrown::HashMap;
use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use vtcode_utility_tool_specs::{DEFAULT_MAX_OUTPUT_TOKENS, MAX_MAX_OUTPUT_TOKENS, MIN_MAX_OUTPUT_TOKENS};

use super::sandboxing::{Sandboxable, SandboxablePreference};
use super::tool_handler::{
    ShellToolCallParams, ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};
use crate::config::constants::tools;
use crate::tools::output_limits::OUTPUT_PREVIEW_CHARS_PER_TOKEN;
use crate::tools::shell::{ShellOutput as CoreShellOutput, ShellRunner};
use crate::tools::validation::commands;

/// Default timeout for shell commands (30 seconds).
const DEFAULT_SHELL_TIMEOUT_MS: u64 = 30_000;

/// Maximum timeout allowed (5 minutes).
const MAX_SHELL_TIMEOUT_MS: u64 = 300_000;

/// Resolved shell invocation: shared command params plus the model-visible
/// preview budget for this call.
struct ResolvedShellCall {
    params: ShellToolCallParams,
    max_output_tokens: usize,
}

/// Handler for shell command execution.
pub struct ShellHandler {
    /// Default shell to use.
    pub default_shell: String,
    /// Environment variables to inherit.
    pub inherit_env: bool,
}

impl Default for ShellHandler {
    fn default() -> Self {
        Self {
            default_shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
            inherit_env: true,
        }
    }
}

impl ShellHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_shell(shell: impl Into<String>) -> Self {
        Self { default_shell: shell.into(), inherit_env: true }
    }

    /// Parse shell parameters from payload.
    fn parse_params(&self, invocation: &ToolInvocation) -> Result<ResolvedShellCall, ToolCallError> {
        match &invocation.payload {
            ToolPayload::Function { arguments } => {
                // Parse as simple shell command string and wrap in ShellToolCallParams
                #[derive(Deserialize)]
                struct SimpleShellArgs {
                    command: String,
                    workdir: Option<String>,
                    timeout_ms: Option<u64>,
                    max_output_tokens: Option<u64>,
                }
                let simple: SimpleShellArgs = serde_json::from_str(arguments)
                    .map_err(|e| ToolCallError::respond(format!("Invalid shell arguments: {e}")))?;
                let max_output_tokens = resolve_max_output_tokens(simple.max_output_tokens)?;
                Ok(ResolvedShellCall {
                    params: ShellToolCallParams {
                        command: vec![simple.command],
                        workdir: simple.workdir,
                        timeout_ms: simple.timeout_ms,
                        sandbox_permissions: None,
                        justification: None,
                    },
                    max_output_tokens,
                })
            }
            ToolPayload::LocalShell { params } => Ok(ResolvedShellCall {
                params: params.clone(),
                max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            }),
            _ => Err(ToolCallError::respond("Invalid payload type for shell handler")),
        }
    }

    /// Execute a shell command.
    async fn execute_command(
        &self,
        params: &ShellToolCallParams,
        cwd: &Path,
        env: Option<HashMap<String, String>>,
    ) -> Result<CoreShellOutput, ToolCallError> {
        let timeout_ms = params.timeout_ms.unwrap_or(DEFAULT_SHELL_TIMEOUT_MS).min(MAX_SHELL_TIMEOUT_MS);
        let result = if let Some((program, args)) = params.command.split_first().filter(|_| params.command.len() > 1) {
            commands::validate_command_argv(&params.command).map_err(ToolCallError::Internal)?;
            let canonical_cwd = vtcode_commons::paths::canonicalize_async(cwd)
                .await
                .map_err(|error| ToolCallError::Internal(error.into()))?;
            let directory = tokio::task::spawn_blocking({
                let canonical_cwd = canonical_cwd.clone();
                move || vtcode_commons::fs::bound_file::open_directory_handle(&canonical_cwd)
            })
            .await
            .map_err(|error| ToolCallError::Internal(error.into()))?
            .map_err(|error| ToolCallError::Internal(error.into()))?;
            let mut command = tokio::process::Command::new(program);
            command.args(args).current_dir(&canonical_cwd);
            vtcode_commons::fs::bound_file::set_command_working_directory(command.as_std_mut(), &directory)
                .map_err(|error| ToolCallError::Internal(error.into()))?;
            let current_env = env.unwrap_or_else(|| std::env::vars().collect());
            let sanitized_env = crate::sandboxing::build_sanitized_env(
                &current_env,
                true,
                false,
                "shell-handler",
                &[canonical_cwd.as_path()],
            );
            command.env_clear().envs(sanitized_env);
            let output = tokio::time::timeout(Duration::from_millis(timeout_ms), command.output())
                .await
                .map_err(|_error| ToolCallError::Timeout(timeout_ms))?
                .map_err(|error| ToolCallError::Internal(error.into()))?;
            CoreShellOutput {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            }
        } else {
            let command = params.command.first().map(String::as_str).unwrap_or_default();
            commands::validate_shell_script(command).map_err(ToolCallError::Internal)?;
            let runner = ShellRunner::new(cwd.to_path_buf());
            tokio::time::timeout(Duration::from_millis(timeout_ms), runner.exec(command))
                .await
                .map_err(|_error| ToolCallError::Timeout(timeout_ms))?
                .map_err(ToolCallError::Internal)?
        };

        Ok(result)
    }
}

impl Sandboxable for ShellHandler {
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Require
    }

    fn escalate_on_failure(&self) -> bool {
        true // Shell commands may need escalation
    }
}

#[async_trait]
impl ToolHandler for ShellHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. } | ToolPayload::LocalShell { .. })
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        // Shell commands are considered mutating by default
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let resolved = self.parse_params(&invocation)?;
        let fallback = crate::tools::source_inspection::SourceInspection::default();
        let requested_cwd = invocation.turn.resolve_path_ref(resolved.params.workdir.as_deref());
        let cwd = vtcode_commons::paths::ensure_path_within_workspace_resolved(
            &requested_cwd,
            invocation.session.workspace_root(),
        )
        .await
        .map_err(|error| ToolCallError::respond(error.to_string()))?;
        let command = match resolved.params.command.as_slice() {
            [script] => serde_json::Value::String(script.clone()),
            argv => json!(argv),
        };
        let args = json!({"command": command});
        invocation
            .session
            .source_inspection()
            .unwrap_or(&fallback)
            .check_shell(&args, &cwd)
            .map_err(|error| ToolCallError::respond(error.to_string()))?;
        let output = self.execute_command(&resolved.params, &cwd, None).await?;

        // Sanitize output to remove any secrets before display/storage
        let sanitized = output.sanitize_secrets();
        let content_text = format_shell_output(&sanitized, resolved.max_output_tokens);

        Ok(ToolOutput::with_success(content_text, sanitized.exit_code == 0))
    }
}

/// Validate and resolve the caller-requested model-visible preview budget.
fn resolve_max_output_tokens(requested: Option<u64>) -> Result<usize, ToolCallError> {
    let Some(tokens) = requested else {
        return Ok(DEFAULT_MAX_OUTPUT_TOKENS);
    };
    // u64 -> usize fails only on 32-bit targets where the value exceeds usize::MAX;
    // the MAX_MAX_OUTPUT_TOKENS bound below is the real range check.
    let Ok(tokens) = usize::try_from(tokens) else {
        return Err(ToolCallError::respond(format!(
            "max_output_tokens must be an integer between {MIN_MAX_OUTPUT_TOKENS} and {MAX_MAX_OUTPUT_TOKENS}"
        )));
    };
    if !(MIN_MAX_OUTPUT_TOKENS..=MAX_MAX_OUTPUT_TOKENS).contains(&tokens) {
        return Err(ToolCallError::respond(format!(
            "max_output_tokens must be an integer between {MIN_MAX_OUTPUT_TOKENS} and {MAX_MAX_OUTPUT_TOKENS}"
        )));
    }
    Ok(tokens)
}

/// Format sanitized shell output for the model, condensing oversized previews
/// to a head/tail excerpt within the token budget.
fn format_shell_output(sanitized: &CoreShellOutput, max_output_tokens: usize) -> String {
    use std::fmt::Write as _;

    let mut content_text = String::with_capacity(sanitized.stdout.len() + sanitized.stderr.len() + 32);
    if !sanitized.stdout.is_empty() {
        content_text.push_str(&sanitized.stdout);
    }
    if !sanitized.stderr.is_empty() {
        if !content_text.is_empty() {
            content_text.push('\n');
        }
        content_text.push_str("[stderr]\n");
        content_text.push_str(&sanitized.stderr);
    }
    if sanitized.exit_code != 0 {
        if !content_text.is_empty() {
            content_text.push('\n');
        }
        let _ = write!(content_text, "[exit code: {}]", sanitized.exit_code);
    }

    if content_text.is_empty() {
        return "(no output)".to_string();
    }

    let budget_bytes = max_output_tokens.saturating_mul(OUTPUT_PREVIEW_CHARS_PER_TOKEN).max(1);
    if content_text.len() <= budget_bytes {
        return content_text;
    }

    let head = budget_bytes / 2;
    let tail = budget_bytes.saturating_sub(head);
    vtcode_commons::preview::condense_text_bytes(&content_text, head, tail)
}

/// Create the shell tool specification.
pub fn create_shell_tool() -> super::tool_handler::ToolSpec {
    use super::tool_handler::{ResponsesApiTool, ToolSpec};

    let parameters = vtcode_utility_tool_specs::with_max_output_tokens_parameter(json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "The shell command to execute"
            },
            "workdir": {
                "type": "string",
                "description": "Working directory for the command (optional)"
            },
            "timeout_ms": {
                "type": "number",
                "description": "Timeout in milliseconds (default: 30000, max: 300000)"
            }
        },
        "required": ["command"],
        "additionalProperties": false
    }));

    ToolSpec::Function(ResponsesApiTool {
        name: tools::SHELL.to_string(),
        description: "Execute a shell command and return its output. Default timeout 30s (max 300s). All commands run through the active sandbox policy. Do NOT use for interactive/long-running processes (e.g., dev servers, watchers) — use spawn_background_subprocess instead.".to_string(),
        parameters,
        strict: false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tool_handler::ToolSpec;
    use super::*;

    #[test]
    fn shell_handler_keeps_sandbox_retry_enabled() {
        assert!(ShellHandler::new().escalate_on_failure());
    }

    #[tokio::test]
    async fn test_shell_handler_echo() {
        let handler = ShellHandler::new();

        // Test that handler kind is correct
        assert_eq!(handler.kind(), ToolKind::Function);
    }

    #[tokio::test]
    async fn direct_shell_handler_allows_reads_in_the_bound_cwd_and_rejects_authored_writes() {
        use super::super::adapter::DefaultToolSession;
        use super::super::tool_handler::{Constrained, ShellEnvironmentPolicy, ToolSession, TurnContext};
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("sample.txt"), "root-only\n").unwrap();
        std::fs::write(root.path().join("nested/sample.txt"), "nested-only\n").unwrap();
        let session = Arc::new(DefaultToolSession::new(root.path().to_path_buf()));
        session
            .source_inspection()
            .unwrap()
            .set_mode(vtcode_config::core::agent::InspectionMode::Enforce);
        let turn = Arc::new(TurnContext {
            cwd: root.path().to_path_buf(),
            turn_id: "direct".into(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::default(),
            approval_policy: Constrained::default(),
            linux_sandbox_launcher: None,
            sandbox_policy: Constrained::default(),
        });
        for (command, succeeds) in [("cat sample.txt", true), ("printf bad > sample.txt", false)] {
            let invocation = ToolInvocation {
                session: session.clone(),
                turn: turn.clone(),
                tracker: None,
                call_id: command.into(),
                tool_name: tools::SHELL.into(),
                payload: ToolPayload::Function {
                    arguments: json!({"command":command,"workdir":"nested"}).to_string(),
                },
            };
            let result = ShellHandler::new().handle(invocation).await;
            if succeeds {
                let result = result.unwrap();
                assert!(result.content().unwrap().contains("nested-only"));
                assert!(!result.content().unwrap().contains("root-only"));
            } else {
                assert!(result.unwrap_err().to_string().contains("source_inspection_shell_write"));
            }
        }
        assert_eq!(std::fs::read_to_string(root.path().join("nested/sample.txt")).unwrap(), "nested-only\n");
    }

    #[test]
    fn test_shell_handler_matches_kind() {
        let handler = ShellHandler::new();

        assert!(handler.matches_kind(&ToolPayload::Function { arguments: "{}".to_string() }));

        assert!(handler.matches_kind(&ToolPayload::LocalShell {
            params: ShellToolCallParams {
                command: vec!["echo".to_string(), "hello".to_string()],
                workdir: None,
                timeout_ms: None,
                sandbox_permissions: None,
                justification: None,
            }
        }));
    }

    #[tokio::test]
    async fn test_shell_handler_is_mutating() {
        // Shell commands are always mutating
    }

    #[test]
    fn test_create_shell_tool_spec() {
        let spec = create_shell_tool();

        assert_eq!(spec.name(), "shell");
    }

    #[test]
    fn shell_tool_schema_advertises_max_output_tokens() {
        let spec = create_shell_tool();
        let ToolSpec::Function(tool) = spec else {
            panic!("shell tool must be a function spec");
        };
        assert!(
            tool.parameters["properties"]
                .get("max_output_tokens")
                .is_some_and(|field| field["default"] == json!(DEFAULT_MAX_OUTPUT_TOKENS)),
            "shell schema must advertise max_output_tokens with the shared default"
        );
    }

    #[test]
    fn resolve_max_output_tokens_defaults_and_bounds() {
        assert_eq!(resolve_max_output_tokens(None).unwrap(), DEFAULT_MAX_OUTPUT_TOKENS);
        assert_eq!(resolve_max_output_tokens(Some(1)).unwrap(), 1);
        assert_eq!(resolve_max_output_tokens(Some(50_000)).unwrap(), 50_000);
        assert!(resolve_max_output_tokens(Some(0)).is_err());
        assert!(resolve_max_output_tokens(Some(50_001)).is_err());
    }

    #[test]
    fn format_shell_output_leaves_small_output_unchanged() {
        let output = CoreShellOutput {
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert_eq!(format_shell_output(&output, DEFAULT_MAX_OUTPUT_TOKENS), "hello\n");
    }

    #[test]
    fn format_shell_output_preserves_exit_code_on_small_failure() {
        let output = CoreShellOutput {
            stdout: "partial\n".to_string(),
            stderr: "boom\n".to_string(),
            exit_code: 2,
        };
        let formatted = format_shell_output(&output, DEFAULT_MAX_OUTPUT_TOKENS);
        assert!(formatted.contains("[stderr]\n"));
        assert!(formatted.contains("[exit code: 2]"));
    }

    #[test]
    fn format_shell_output_condenses_oversized_content() {
        let stdout = "x".repeat(10_000);
        let output = CoreShellOutput {
            stdout: stdout.clone(),
            stderr: String::new(),
            exit_code: 0,
        };
        // 10 tokens * 4 chars/token = 40 byte budget.
        let formatted = format_shell_output(&output, 10);
        assert!(formatted.len() < stdout.len());
        assert!(formatted.contains("bytes omitted"));
        assert!(formatted.starts_with("xxxx"));
        assert!(formatted.ends_with("xxxx"));
    }

    #[test]
    fn format_shell_output_empty_becomes_no_output_marker() {
        let output = CoreShellOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert_eq!(format_shell_output(&output, DEFAULT_MAX_OUTPUT_TOKENS), "(no output)");
    }
}
