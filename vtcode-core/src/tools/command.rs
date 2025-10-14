//! Command execution tool

use super::traits::{ModeTool, Tool};
use super::types::*;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

/// Command execution tool for non-PTY process handling with shell-aware quoting
#[derive(Clone)]
pub struct CommandTool {
    workspace_root: PathBuf,
}

impl CommandTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    async fn execute_terminal_command(
        &self,
        input: &EnhancedTerminalInput,
        invocation: CommandInvocation,
    ) -> Result<Value> {
        let mut cmd = Command::new(&invocation.program);
        cmd.args(&invocation.args);

        let work_dir = if let Some(ref working_dir) = input.working_dir {
            self.workspace_root.join(working_dir)
        } else {
            self.workspace_root.clone()
        };

        cmd.current_dir(work_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let duration = Duration::from_secs(input.timeout_secs.unwrap_or(30));
        let output = timeout(duration, cmd.output())
            .await
            .with_context(|| {
                format!(
                    "command '{}' timed out after {}s",
                    invocation.display,
                    duration.as_secs()
                )
            })?
            .with_context(|| format!("failed to run command: {}", invocation.display))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(json!({
            "success": output.status.success(),
            "exit_code": output.status.code().unwrap_or_default(),
            "stdout": stdout,
            "stderr": stderr,
            "mode": "terminal",
            "pty_enabled": false,
            "command": invocation.display,
            "used_shell": invocation.used_shell
        }))
    }

    fn prepare_invocation(&self, input: &EnhancedTerminalInput) -> Result<CommandInvocation> {
        if input.command.is_empty() {
            return Err(anyhow!("Command cannot be empty"));
        }

        self.validate_command_segments(&input.command)?;

        if let Some(invocation) = detect_explicit_shell(&input.command, &input.raw_command) {
            self.validate_script(&invocation.display)?;
            return Ok(invocation);
        }

        let shell = input
            .shell
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(default_shell);
        let login = input.login.unwrap_or(true);
        let script = if let Some(raw) = &input.raw_command {
            raw.clone()
        } else {
            join_command_for_shell(&input.command, Some(shell.as_str()))
        };

        self.validate_script(&script)?;

        let args = build_shell_arguments(&shell, login, &script);

        Ok(CommandInvocation {
            program: shell,
            args,
            display: script,
            used_shell: true,
        })
    }

    fn validate_command_segments(&self, command: &[String]) -> Result<()> {
        let program = &command[0];
        if program.chars().any(char::is_whitespace) {
            return Ok(());
        }

        let dangerous_commands = ["rm", "rmdir", "del", "format", "fdisk", "mkfs", "dd"];
        if dangerous_commands.contains(&program.as_str()) {
            return Err(anyhow!("Dangerous command not allowed: {}", program));
        }

        let full_command = command.join(" ");
        if full_command.contains("rm -rf /") || full_command.contains("sudo rm") {
            return Err(anyhow!("Potentially dangerous command pattern detected"));
        }

        Ok(())
    }

    fn validate_script(&self, script: &str) -> Result<()> {
        if script.contains("rm -rf /")
            || script.contains("sudo rm")
            || script.contains("format")
            || script.contains("fdisk")
            || script.contains("mkfs")
        {
            return Err(anyhow!(
                "Potentially dangerous command pattern detected in shell command"
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for CommandTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let input: EnhancedTerminalInput = serde_json::from_value(args)?;
        let invocation = self.prepare_invocation(&input)?;
        self.execute_terminal_command(&input, invocation).await
    }

    fn name(&self) -> &'static str {
        tools::RUN_TERMINAL_CMD
    }

    fn description(&self) -> &'static str {
        "Execute terminal commands"
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        let input: EnhancedTerminalInput = serde_json::from_value(args.clone())?;
        self.prepare_invocation(&input).map(|_| ())
    }
}

#[async_trait]
impl ModeTool for CommandTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec!["terminal"]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        let input: EnhancedTerminalInput = serde_json::from_value(args)?;
        let invocation = self.prepare_invocation(&input)?;
        match mode {
            "terminal" => self.execute_terminal_command(&input, invocation).await,
            _ => Err(anyhow!("Unsupported command execution mode: {}", mode)),
        }
    }
}

#[derive(Debug, Clone)]
struct CommandInvocation {
    program: String,
    args: Vec<String>,
    display: String,
    used_shell: bool,
}

fn detect_explicit_shell(
    command: &[String],
    raw_command: &Option<String>,
) -> Option<CommandInvocation> {
    if command.is_empty() {
        return None;
    }

    let program = &command[0];
    if !is_shell_program(program) {
        return None;
    }

    let args = command[1..].to_vec();
    let display = raw_command
        .clone()
        .or_else(|| extract_shell_script(program, &args))
        .unwrap_or_else(|| join_command_for_shell(command, Some(program.as_str())));

    Some(CommandInvocation {
        program: program.clone(),
        args,
        display,
        used_shell: true,
    })
}

fn join_command_for_shell(command: &[String], shell: Option<&str>) -> String {
    let quoting = shell
        .map(shell_quoting_style)
        .unwrap_or(ShellQuotingStyle::Posix);
    command
        .iter()
        .map(|part| quote_argument(part, quoting))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_argument(arg: &str, style: ShellQuotingStyle) -> String {
    match style {
        ShellQuotingStyle::Cmd => quote_argument_cmd(arg),
        ShellQuotingStyle::PowerShell => quote_argument_powershell(arg),
        ShellQuotingStyle::Posix => quote_argument_posix(arg),
    }
}

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

fn quote_argument_powershell(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }

    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@".contains(ch))
    {
        return arg.to_string();
    }

    let escaped = arg.replace('\'', "''");
    format!("'{}'", escaped)
}

fn quote_argument_cmd(arg: &str) -> String {
    if arg.is_empty() {
        return String::from("\"\"");
    }

    let mut escaped = String::new();
    let mut needs_quotes = false;

    for ch in arg.chars() {
        match ch {
            '"' => {
                needs_quotes = true;
                escaped.push('\\');
                escaped.push('"');
            }
            '^' => {
                needs_quotes = true;
                escaped.push('^');
                escaped.push('^');
            }
            '&' | '|' | '<' | '>' | '(' | ')' => {
                needs_quotes = true;
                escaped.push('^');
                escaped.push(ch);
            }
            '%' => {
                needs_quotes = true;
                escaped.push('%');
                escaped.push('%');
            }
            ' ' | '\t' => {
                needs_quotes = true;
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }

    if needs_quotes {
        format!("\"{}\"", escaped)
    } else {
        escaped
    }
}

fn shell_quoting_style(shell: &str) -> ShellQuotingStyle {
    match shell_program_name(shell).as_str() {
        "cmd" | "cmd.exe" => ShellQuotingStyle::Cmd,
        "pwsh" | "powershell" | "powershell.exe" => ShellQuotingStyle::PowerShell,
        _ => ShellQuotingStyle::Posix,
    }
}

#[derive(Copy, Clone)]
enum ShellQuotingStyle {
    Posix,
    Cmd,
    PowerShell,
}

fn extract_shell_script(program: &str, args: &[String]) -> Option<String> {
    let name = shell_program_name(program);
    match name.as_str() {
        "sh" | "bash" | "zsh" | "ksh" | "dash" | "fish" => {
            if args.len() >= 2 && matches!(args[0].as_str(), "-c" | "-lc") {
                Some(args[1].clone())
            } else {
                None
            }
        }
        "pwsh" | "powershell" | "powershell.exe" => {
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                if arg.eq_ignore_ascii_case("-command") || arg.eq_ignore_ascii_case("-c") {
                    return iter.next().cloned();
                }
            }
            None
        }
        "cmd" | "cmd.exe" => {
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                if arg.eq_ignore_ascii_case("/c") {
                    return iter.next().cloned();
                }
            }
            None
        }
        _ => None,
    }
}

fn build_shell_arguments(shell: &str, login: bool, script: &str) -> Vec<String> {
    let name = shell_program_name(shell);
    match name.as_str() {
        "cmd" | "cmd.exe" => vec!["/C".to_string(), script.to_string()],
        "pwsh" | "powershell" | "powershell.exe" => {
            let mut args = Vec::new();
            if login {
                args.push("-NoProfile".to_string());
            }
            args.push("-Command".to_string());
            args.push(script.to_string());
            args
        }
        _ => {
            let flag = if login { "-lc" } else { "-c" };
            vec![flag.to_string(), script.to_string()]
        }
    }
}

fn is_shell_program(program: &str) -> bool {
    matches!(
        shell_program_name(program).as_str(),
        "sh" | "bash"
            | "zsh"
            | "dash"
            | "ksh"
            | "fish"
            | "pwsh"
            | "powershell"
            | "powershell.exe"
            | "cmd"
            | "cmd.exe"
    )
}

fn shell_program_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase()
}

fn default_shell() -> String {
    if let Ok(shell) = env::var("SHELL") {
        if !shell.trim().is_empty() {
            return shell;
        }
    }

    if let Ok(comspec) = env::var("COMSPEC") {
        if !comspec.trim().is_empty() {
            return comspec;
        }
    }

    if cfg!(windows) {
        return "cmd.exe".to_string();
    }

    let fallback_shells = ["/bin/bash", "/usr/bin/bash", "/bin/sh"];
    for candidate in fallback_shells {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    "sh".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> CommandTool {
        CommandTool::new(PathBuf::from("."))
    }

    #[test]
    fn quotes_arguments_for_posix_shell() {
        assert_eq!(quote_argument_posix("simple"), "simple");
        assert_eq!(quote_argument_posix("needs space"), "'needs space'");
        assert_eq!(quote_argument_posix("quote'inner"), r#"'quote'"'"'inner'"#);
    }

    #[test]
    fn quotes_arguments_for_powershell_shell() {
        assert_eq!(quote_argument_powershell("simple"), "simple");
        assert_eq!(
            quote_argument_powershell("value with space"),
            "'value with space'"
        );
        assert_eq!(quote_argument_powershell("O'Reilly"), "'O''Reilly'");
    }

    #[test]
    fn quotes_arguments_for_cmd_shell() {
        assert_eq!(quote_argument_cmd("simple"), "simple");
        assert_eq!(quote_argument_cmd("needs space"), r#""needs space""#);
        assert_eq!(
            quote_argument_cmd("x & del important"),
            r#""x ^& del important""#
        );
        assert_eq!(quote_argument_cmd("%TEMP%"), r#""%%TEMP%%""#);
    }

    #[test]
    fn joins_command_for_posix_shell_execution() {
        let parts = vec!["echo".to_string(), "hello world".to_string()];
        assert_eq!(
            join_command_for_shell(&parts, Some("/bin/bash")),
            "echo 'hello world'"
        );
    }

    #[test]
    fn joins_command_for_cmd_shell_execution() {
        let parts = vec!["echo".to_string(), "x & del important".to_string()];
        assert_eq!(
            join_command_for_shell(&parts, Some("cmd.exe")),
            r#"echo "x ^& del important""#
        );
    }

    #[test]
    fn joins_command_for_powershell_execution() {
        let parts = vec!["Write-Output".to_string(), "O'Reilly".to_string()];
        assert_eq!(
            join_command_for_shell(&parts, Some("pwsh")),
            "Write-Output 'O''Reilly'"
        );
    }

    #[test]
    fn detects_explicit_bash_script() {
        let args = vec!["bash".to_string(), "-lc".to_string(), "ls".to_string()];
        let invocation = detect_explicit_shell(&args, &None).expect("shell invocation");
        assert_eq!(invocation.program, "bash");
        assert_eq!(invocation.args, vec!["-lc".to_string(), "ls".to_string()]);
        assert_eq!(invocation.display, "ls");
    }

    #[test]
    fn prepare_invocation_wraps_non_shell_commands() {
        let tool = make_tool();
        let input = EnhancedTerminalInput {
            command: vec!["cargo".into(), "test".into()],
            working_dir: None,
            timeout_secs: None,
            mode: None,
            response_format: None,
            raw_command: None,
            shell: Some("/bin/bash".into()),
            login: Some(true),
        };
        let invocation = tool.prepare_invocation(&input).expect("invocation");
        assert_eq!(invocation.program, "/bin/bash");
        assert_eq!(invocation.args[0], "-lc");
        assert_eq!(invocation.display, "cargo test");
    }
}
