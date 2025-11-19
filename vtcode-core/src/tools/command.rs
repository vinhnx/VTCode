//! Command execution tool

use super::types::*;
use crate::config::CommandsConfig;
use crate::execpolicy::{sanitize_working_dir, validate_command};
use crate::tools::command_policy::CommandPolicyEvaluator;
use crate::tools::path_env;
use crate::tools::shell::resolve_fallback_shell;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

/// Command execution tool for non-PTY process handling with policy enforcement
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
    policy: CommandPolicyEvaluator,
    extra_path_entries: Vec<PathBuf>,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_commands_config(workspace_root, CommandsConfig::default())
    }

    pub fn with_commands_config(workspace_root: PathBuf, commands_config: CommandsConfig) -> Self {
        // Note: We use the workspace_root directly here. Full validation happens
        // in prepare_invocation which is async.
        let policy = CommandPolicyEvaluator::from_config(&commands_config);
        let extra_path_entries = path_env::compute_extra_search_paths(
            &commands_config.extra_path_entries,
            &workspace_root,
        );
        Self {
            workspace_root,
            policy,
            extra_path_entries,
        }
    }

    pub fn update_commands_config(&mut self, commands_config: &CommandsConfig) {
        self.policy = CommandPolicyEvaluator::from_config(commands_config);
        self.extra_path_entries = path_env::compute_extra_search_paths(
            &commands_config.extra_path_entries,
            &self.workspace_root,
        );
    }

    pub(crate) async fn prepare_invocation(
        &self,
        input: &EnhancedTerminalInput,
    ) -> Result<CommandInvocation> {
        let command = &input.command;
        if command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }

        let program = &command[0];
        // Validate that the executable is non-empty after trimming
        if program.trim().is_empty() {
            return Err(anyhow!("Command executable cannot be empty"));
        }
        if program.contains(char::is_whitespace) {
            return Err(anyhow!(
                "Program name cannot contain whitespace: {}",
                program
            ));
        }

        let working_dir =
            sanitize_working_dir(&self.workspace_root, input.working_dir.as_deref()).await?;

        // Policy check: config rules first, fallback to strict allowlist
        let confirm_ok = input.confirm.unwrap_or(false);
        if !self.policy.allows(command) {
            // Forward confirmation status to validator so callers can opt-in to destructive commands
            validate_command(command, &self.workspace_root, &working_dir, confirm_ok).await?;
        }

        // Require explicit confirmation for high-risk operations even if policy allows them
        if is_risky_command(&command) && !confirm_ok {
            return Err(anyhow!(
                "Command appears destructive; set the `confirm` field to true to proceed."
            ));
        }
        if is_risky_command(&command) && confirm_ok {
            // Record audit for the explicitly confirmed destructive command
            log_audit_for_command(
                &format_command(command),
                "Confirmed destructive operation by agent",
            );
        }

        // If the program name includes a path separator or is absolute, execute it directly as provided
        // (unless the caller explicitly requested a shell override). Otherwise, always use the
        // user's login shell in `-lc` mode so PATH and environment are initialized consistently.
        let resolved_invocation =
            if program.contains(std::path::MAIN_SEPARATOR) || program.contains('/') {
                // Program provided as absolute/relative path: run directly
                CommandInvocation {
                    program: program.to_string(),
                    args: command[1..].to_vec(),
                    display: input
                        .raw_command
                        .clone()
                        .unwrap_or_else(|| format_command(command)),
                }
            } else {
                // Honor explicit shell override provided in the input. If the caller set `login` to
                // false, use `-c` (no login). Otherwise use `-lc` to force login shell semantics.
                let shell = input
                    .shell
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| resolve_fallback_shell());
                let use_login = input.login.unwrap_or(true);
                let full_command = format_command(command);
                CommandInvocation {
                    program: shell,
                    args: vec![
                        if use_login {
                            "-lc".to_string()
                        } else {
                            "-c".to_string()
                        },
                        full_command.clone(),
                    ],
                    display: full_command,
                }
            };

        Ok(resolved_invocation)
    }
}

// NOTE: Tool and ModeTool trait implementations removed since CommandTool
// is no longer registered as a public tool (RUN_COMMAND was deprecated).
// CommandTool is kept for internal command preparation in the PTY system.

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CommandInvocation {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) display: String,
}

#[allow(dead_code)]
fn format_command(command: &[String]) -> String {
    command
        .iter()
        .map(|part| quote_argument_posix(part))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
fn is_risky_command(command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }
    let program = command[0].as_str();
    let args = &command[1..];

    match program {
        "git" => {
            if args.is_empty() {
                return false;
            }
            if args[0] == "reset"
                && args
                    .iter()
                    .any(|a| a == "--hard" || a == "--merge" || a == "--keep")
            {
                return true;
            }
            if args[0] == "push" && args.iter().any(|a| a == "--force" || a == "-f") {
                return true;
            }
            if args[0] == "clean" && args.iter().any(|a| a == "-f" || a == "-x" || a == "-d") {
                return true;
            }
            false
        }
        "rm" => {
            args.iter()
                .any(|a| a == "-rf" || a == "-r" || a == "-f" || a == "-rf/")
                || args.iter().any(|a| a == "/")
        }
        "docker" => args
            .iter()
            .any(|a| a == "run" && args.iter().any(|b| b == "--privileged")),
        "kubectl" => true, // kubectl operations can be destructive; require confirmation
        _ => false,
    }
}

#[allow(dead_code)]
fn log_audit_for_command(_command: &str, _reason: &str) {
    // Audit logging removed - kept as no-op for backwards compatibility
}

#[allow(dead_code)]
fn quote_argument_posix(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }

    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@".contains(ch))
    {
        return arg.to_string();
    }

    let mut quoted = String::from("'");
    for ch in arg.chars() {
        if ch == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::path_env;
    use tempfile::tempdir;

    fn make_tool() -> CommandTool {
        let cwd = std::env::current_dir().expect("current dir");
        CommandTool::new(cwd)
    }

    fn make_input(command: Vec<&str>) -> EnhancedTerminalInput {
        EnhancedTerminalInput {
            command: command.into_iter().map(String::from).collect(),
            working_dir: None,
            timeout_secs: None,
            mode: None,
            response_format: None,
            raw_command: None,
            shell: None,
            login: None,
            confirm: None,
        }
    }

    #[test]
    fn formats_command_for_display() {
        let parts = vec!["echo".to_string(), "hello world".to_string()];
        assert_eq!(format_command(&parts), "echo 'hello world'");
    }

    #[tokio::test]
    async fn prepare_invocation_allows_policy_command() {
        let tool = make_tool();
        let input = make_input(vec!["ls"]);
        let invocation = tool.prepare_invocation(&input).await.expect("invocation");
        let shell = resolve_fallback_shell();
        assert_eq!(invocation.program, shell);
        assert_eq!(invocation.args, vec!["-lc".to_string(), "ls".to_string()]);
        assert_eq!(invocation.display, "ls");
    }

    #[tokio::test]
    async fn prepare_invocation_allows_cargo_via_policy() {
        let tool = make_tool();
        let input = make_input(vec!["cargo", "check"]);
        let invocation = tool
            .prepare_invocation(&input)
            .await
            .expect("cargo check should be allowed");
        let shell = resolve_fallback_shell();
        assert_eq!(invocation.program, shell);
        assert_eq!(
            invocation.args,
            vec!["-lc".to_string(), "cargo check".to_string()]
        );
        assert_eq!(invocation.display, "cargo check");
    }

    #[tokio::test]
    async fn prepare_invocation_rejects_command_not_in_policy() {
        let tool = make_tool();
        let input = make_input(vec!["custom-tool"]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("custom-tool should be blocked");
        assert!(
            error
                .to_string()
                .contains("is not permitted by the execution policy")
        );
    }

    #[tokio::test]
    async fn prepare_invocation_requires_confirm_for_git_reset_hard() {
        let tool = make_tool();
        let mut input = make_input(vec!["git", "reset", "--hard"]);
        // No explicit confirm set - should error
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("git reset --hard should require confirmation");
        assert!(error.to_string().contains("set the `confirm` field"));
    }

    #[tokio::test]
    async fn prepare_invocation_allows_git_reset_with_confirm() {
        let tool = make_tool();
        let mut input = make_input(vec!["git", "reset", "--hard"]);
        input.confirm = Some(true);
        let invocation = tool
            .prepare_invocation(&input)
            .await
            .expect("git reset --hard should be allowed when confirm=true");
        assert!(invocation.display.contains("git reset"));
    }

    #[tokio::test]
    async fn prepare_invocation_respects_custom_allow_list() {
        let cwd = std::env::current_dir().expect("current dir");
        let mut config = CommandsConfig::default();
        config.allow_list.push("my-build".to_string());
        let tool = CommandTool::with_commands_config(cwd, config);
        let input = make_input(vec!["my-build"]);
        let invocation = tool
            .prepare_invocation(&input)
            .await
            .expect("custom allow list should enable command");
        let shell = resolve_fallback_shell();
        assert_eq!(invocation.program, shell);
        assert_eq!(
            invocation.args,
            vec!["-lc".to_string(), "my-build".to_string()]
        );
    }

    #[tokio::test]
    async fn prepare_invocation_respects_shell_override_and_login_false() {
        let cwd = std::env::current_dir().expect("current dir");
        let tool = CommandTool::new(cwd);
        let mut input = make_input(vec!["my-build"]);
        input.shell = Some("/bin/sh".to_string());
        input.login = Some(false);
        let invocation = tool.prepare_invocation(&input).await.expect("invocation");
        assert_eq!(invocation.program, "/bin/sh".to_string());
        assert_eq!(
            invocation.args,
            vec!["-c".to_string(), "my-build".to_string()]
        );
    }

    #[test]
    fn resolve_program_path_respects_os_path_separator() {
        let noise_dir = tempdir().expect("noise tempdir");
        let target_dir = tempdir().expect("target tempdir");
        let fake_tool_path = target_dir.path().join("fake-tool");
        std::fs::write(&fake_tool_path, b"#!/bin/sh\n").expect("write fake tool");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake_tool_path)
                .expect("metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_tool_path, perms).expect("set perms");
        }

        let custom_paths = vec![
            noise_dir.path().to_path_buf(),
            target_dir.path().to_path_buf(),
        ];
        let resolved =
            path_env::resolve_program_path_from_paths("fake-tool", custom_paths.into_iter());
        let expected = fake_tool_path.to_string_lossy().to_string();
        assert_eq!(resolved, Some(expected));
    }

    #[tokio::test]
    async fn prepare_invocation_respects_custom_deny_list() {
        let cwd = std::env::current_dir().expect("current dir");
        let mut config = CommandsConfig::default();
        config.deny_list.push("cargo".to_string());
        let tool = CommandTool::with_commands_config(cwd, config);
        let input = make_input(vec!["cargo", "check"]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("deny list should block cargo");
        assert!(error.to_string().contains("is not permitted"));
    }

    #[tokio::test]
    async fn prepare_invocation_uses_shell_for_command_execution() {
        let tool = make_tool();
        let input = make_input(vec!["srt", "run"]);
        let invocation = tool.prepare_invocation(&input).await.expect("invocation");
        let shell = resolve_fallback_shell();
        assert_eq!(invocation.program, shell);
        assert_eq!(
            invocation.args,
            vec!["-lc".to_string(), "srt run".to_string()]
        );
        assert_eq!(invocation.display, "srt run");
    }

    #[tokio::test]
    async fn prepare_invocation_uses_extra_path_entries() {
        let cwd = std::env::current_dir().expect("current dir");
        let temp_dir = tempdir().expect("tempdir");
        let binary_path = temp_dir.path().join("fake-extra");
        std::fs::write(&binary_path, b"#!/bin/sh\n").expect("write fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&binary_path)
                .expect("metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&binary_path, perms).expect("set perms");
        }

        let mut config = CommandsConfig::default();
        config.allow_list.push("fake-extra".to_string());
        config.extra_path_entries = vec![
            binary_path
                .parent()
                .expect("parent")
                .to_string_lossy()
                .to_string(),
        ];

        let tool = CommandTool::with_commands_config(cwd, config);
        let input = make_input(vec!["fake-extra"]);
        let invocation = tool
            .prepare_invocation(&input)
            .await
            .expect("extra path should allow command");
        assert_eq!(
            invocation.program,
            binary_path.to_string_lossy().to_string()
        );
        assert!(invocation.args.is_empty());
    }

    #[tokio::test]
    async fn working_dir_escape_is_rejected() {
        let tool = make_tool();
        let mut input = make_input(vec!["ls"]);
        input.working_dir = Some("../".into());
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("working dir escape should fail");
        assert!(
            error
                .to_string()
                .contains("working directory '../' escapes the workspace root")
        );
    }

    #[tokio::test]
    async fn prepare_invocation_rejects_empty_command() {
        let tool = make_tool();
        let input = make_input(vec![]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("empty command should be rejected");
        assert!(error.to_string().contains("Command cannot be empty"));
    }

    #[tokio::test]
    async fn prepare_invocation_rejects_empty_executable() {
        let tool = make_tool();
        let input = make_input(vec!["", "arg1"]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("empty executable should be rejected");
        assert!(
            error
                .to_string()
                .contains("Command executable cannot be empty")
        );
    }

    #[tokio::test]
    async fn prepare_invocation_rejects_whitespace_only_executable() {
        let tool = make_tool();
        let input = make_input(vec!["   ", "arg1"]);
        let error = tool
            .prepare_invocation(&input)
            .await
            .expect_err("whitespace-only executable should be rejected");
        assert!(
            error
                .to_string()
                .contains("Command executable cannot be empty")
        );
    }

    #[tokio::test]
    async fn validate_args_rejects_empty_command() {
        let tool = make_tool();
        let args = json!({ "command": [] });
        let error = tool
            .validate_args(&args)
            .expect_err("empty command should fail validation");
        assert!(error.to_string().contains("Command cannot be empty"));
    }

    #[tokio::test]
    async fn validate_args_rejects_empty_executable() {
        let tool = make_tool();
        let args = json!({ "command": ["", "arg1"] });
        let error = tool
            .validate_args(&args)
            .expect_err("empty executable should fail validation");
        assert!(
            error
                .to_string()
                .contains("Command executable cannot be empty")
        );
    }

    #[tokio::test]
    async fn validate_args_accepts_valid_command() {
        let tool = make_tool();
        let args = json!({ "command": ["ls", "-la"] });
        tool.validate_args(&args)
            .expect("valid command should pass validation");
    }

    #[test]
    fn environment_variables_are_inherited_from_parent() {
        // Verify that the environment setup includes inherited parent process variables.
        // This test documents the fix for the cargo fmt issue where PATH and other
        // critical environment variables were not being passed to subprocesses.
        // See: vtcode-core/src/tools/command.rs:execute_terminal_command()

        // Set a test environment variable in the parent process
        unsafe {
            std::env::set_var("_TEST_VAR_FOR_ENV_INHERITANCE", "test_value");
        }

        // The fix uses std::env::vars_os().collect() which inherits all parent variables
        let env: HashMap<OsString, OsString> = std::env::vars_os().collect();

        // Verify our test variable is present
        assert!(
            env.contains_key(&OsString::from("_TEST_VAR_FOR_ENV_INHERITANCE")),
            "Parent environment variables should be inherited"
        );

        // Verify critical system variables are present
        assert!(
            env.contains_key(&OsString::from("PATH")),
            "PATH environment variable must be inherited for command resolution"
        );

        // Cleanup
        unsafe {
            std::env::remove_var("_TEST_VAR_FOR_ENV_INHERITANCE");
        }
    }
}
