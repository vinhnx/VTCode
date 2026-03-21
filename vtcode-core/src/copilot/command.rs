use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use tokio::process::{Child, Command};
use vtcode_config::auth::CopilotAuthConfig;

const DEFAULT_PROGRAM: &str = "copilot";
const ENV_OVERRIDE: &str = "VTCODE_COPILOT_COMMAND";
const ACP_FLAGS: &[&str] = &[
    "--acp",
    "--stdio",
    "--no-custom-instructions",
    "--no-experimental",
    "--disable-builtin-mcps",
];
const SERVER_FLAGS: &[&str] = &[
    "--headless",
    "--no-auto-update",
    "--log-level",
    "error",
    "--stdio",
];
const STRIPPED_RUNTIME_ENV_VARS: &[&str] = &[
    "COPILOT_ALLOW_ALL",
    "COPILOT_CUSTOM_INSTRUCTIONS_DIRS",
    "COPILOT_EXPERIMENTS",
    "COPILOT_FEATURE_FLAGS",
    "GITHUB_COPILOT_MCP_JSON_FROM_INPUT",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotModelSelectionMode {
    CliArgument,
    EnvironmentVariable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCopilotCommand {
    pub program: String,
    pub args: Vec<String>,
    pub startup_timeout: Duration,
    pub auth_timeout: Duration,
}

impl ResolvedCopilotCommand {
    #[must_use]
    pub fn display(&self) -> String {
        format_command(&self.program, &self.args)
    }

    pub fn command(&self, cwd: Option<&Path>, extra_args: &[String]) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).args(extra_args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command
    }
}

pub fn resolve_copilot_command(config: &CopilotAuthConfig) -> Result<ResolvedCopilotCommand> {
    let (program, args) = if let Some(override_command) = env_override_command()? {
        split_command(&override_command, ENV_OVERRIDE)?
    } else if let Some(program) = configured_command(config)? {
        program
    } else {
        (DEFAULT_PROGRAM.to_string(), Vec::new())
    };

    Ok(ResolvedCopilotCommand {
        program,
        args,
        startup_timeout: Duration::from_secs(config.startup_timeout_secs.max(1)),
        auth_timeout: Duration::from_secs(config.auth_timeout_secs.max(1)),
    })
}

#[must_use]
pub fn copilot_command_available(resolved: &ResolvedCopilotCommand) -> bool {
    if resolved.program.contains(std::path::MAIN_SEPARATOR)
        || resolved.program.contains('/')
        || resolved.program.contains('\\')
    {
        return Path::new(&resolved.program).exists();
    }

    which::which(&resolved.program).is_ok()
}

pub fn spawn_copilot_acp_process(
    resolved: &ResolvedCopilotCommand,
    config: &CopilotAuthConfig,
    cwd: &Path,
    raw_model: Option<&str>,
    model_selection_mode: CopilotModelSelectionMode,
) -> Result<Child> {
    let mut extra_args = Vec::new();
    if let Some(raw_model) = raw_model.filter(|value| !value.trim().is_empty())
        && matches!(model_selection_mode, CopilotModelSelectionMode::CliArgument)
    {
        extra_args.push("--model".to_string());
        extra_args.push(raw_model.to_string());
    }
    for tool in &config.available_tools {
        let trimmed = tool.trim();
        if trimmed.is_empty() {
            continue;
        }
        extra_args.push("--available-tools".to_string());
        extra_args.push(trimmed.to_string());
    }
    for tool in &config.excluded_tools {
        let trimmed = tool.trim();
        if trimmed.is_empty() {
            continue;
        }
        extra_args.push("--excluded-tools".to_string());
        extra_args.push(trimmed.to_string());
    }
    extra_args.extend(ACP_FLAGS.iter().map(|flag| (*flag).to_string()));
    let mut command = resolved.command(Some(cwd), &extra_args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(raw_model) = raw_model.filter(|value| !value.trim().is_empty())
        && matches!(
            model_selection_mode,
            CopilotModelSelectionMode::EnvironmentVariable
        )
    {
        command.env("COPILOT_MODEL", raw_model);
    } else {
        command.env_remove("COPILOT_MODEL");
    }
    for env_var in STRIPPED_RUNTIME_ENV_VARS {
        command.env_remove(env_var);
    }

    command.spawn().with_context(|| {
        format!(
            "failed to spawn GitHub Copilot ACP runtime using `{}`",
            resolved.display()
        )
    })
}

pub fn spawn_copilot_server_process(
    resolved: &ResolvedCopilotCommand,
    cwd: &Path,
) -> Result<Child> {
    let extra_args = SERVER_FLAGS
        .iter()
        .map(|flag| (*flag).to_string())
        .collect::<Vec<_>>();
    let mut command = resolved.command(Some(cwd), &extra_args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("COPILOT_MODEL");

    for env_var in STRIPPED_RUNTIME_ENV_VARS {
        command.env_remove(env_var);
    }

    command.spawn().with_context(|| {
        format!(
            "failed to spawn GitHub Copilot CLI server using `{}`",
            resolved.display()
        )
    })
}

fn env_override_command() -> Result<Option<String>> {
    let Some(value) = std::env::var_os(ENV_OVERRIDE) else {
        return Ok(None);
    };
    let value = value.to_string_lossy().trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn configured_command(config: &CopilotAuthConfig) -> Result<Option<(String, Vec<String>)>> {
    let Some(command) = config.command.as_deref() else {
        return Ok(None);
    };
    let trimmed = command.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        split_command(trimmed, "[auth.copilot].command").map(Some)
    }
}

fn split_command(command: &str, source: &str) -> Result<(String, Vec<String>)> {
    let mut parts =
        shell_words::split(command).with_context(|| format!("invalid {source} command"))?;
    if parts.is_empty() {
        return Err(anyhow!("{source} command cannot be empty"));
    }
    let program = parts.remove(0);
    Ok((program, parts))
}

fn format_command(program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_words::quote(program).to_string());
    parts.extend(args.iter().map(|arg| shell_words::quote(arg).to_string()));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> CopilotAuthConfig {
        CopilotAuthConfig::default()
    }

    #[test]
    fn resolve_command_uses_default_copilot_binary() {
        let resolved = resolve_copilot_command(&default_config()).unwrap();

        assert_eq!(resolved.program, "copilot");
        assert!(resolved.args.is_empty());
    }

    #[test]
    fn resolve_command_splits_custom_command_with_args() {
        let config = CopilotAuthConfig {
            command: Some("node /tmp/copilot.js".to_string()),
            ..CopilotAuthConfig::default()
        };

        let resolved = resolve_copilot_command(&config).unwrap();

        assert_eq!(resolved.program, "node");
        assert_eq!(resolved.args, vec!["/tmp/copilot.js".to_string()]);
    }

    #[test]
    fn display_quotes_arguments() {
        let resolved = ResolvedCopilotCommand {
            program: "copilot".to_string(),
            args: vec!["--config-dir".to_string(), "/tmp/copilot home".to_string()],
            startup_timeout: Duration::from_secs(1),
            auth_timeout: Duration::from_secs(1),
        };

        let display = resolved.display();

        assert!(display.starts_with("copilot --config-dir "));
        assert!(display.contains("/tmp/copilot home"));
    }
}
