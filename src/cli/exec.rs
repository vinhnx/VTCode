use anyhow::{Context, Result, anyhow, bail};
use console::style;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::str::FromStr;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails};
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;

use crate::workspace_trust::workspace_trust_level;

const EXEC_SESSION_PREFIX: &str = "exec-task";
const EXEC_TASK_ID: &str = "exec-task";
const EXEC_TASK_TITLE: &str = "Exec Task";

#[derive(Debug, Clone)]
pub struct ExecCommandOptions {
    pub json: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
}

fn resolve_prompt(prompt_arg: Option<String>) -> Result<String> {
    match prompt_arg {
        Some(p) if p != "-" => Ok(p),
        maybe_dash => {
            let force_stdin = matches!(maybe_dash.as_deref(), Some("-"));
            if io::stdin().is_terminal() && !force_stdin {
                bail!(
                    "No prompt provided. Pass a prompt argument, pipe input, or use '-' to read from stdin."
                );
            }
            if !force_stdin {
                eprintln!("Reading prompt from stdin...");
            }
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .context("Failed to read prompt from stdin")?;
            if buffer.trim().is_empty() {
                bail!("No prompt provided via stdin.");
            }
            Ok(buffer)
        }
    }
}

fn last_agent_message(events: &[ThreadEvent]) -> Option<&str> {
    events.iter().rev().find_map(|event| match event {
        ThreadEvent::ItemCompleted(completed) => match &completed.item.details {
            ThreadItemDetails::AgentMessage(item) => Some(item.text.as_str()),
            _ => None,
        },
        _ => None,
    })
}

pub async fn handle_exec_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
    prompt_arg: Option<String>,
) -> Result<()> {
    let prompt = resolve_prompt(prompt_arg)?;

    let trust_level = workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level")?;

    match trust_level {
        Some(WorkspaceTrustLevel::FullAuto) => {}
        Some(level) => {
            bail!(
                "Workspace trust level '{level}' does not permit exec runs. Upgrade trust to full auto."
            );
        }
        None => {
            bail!(
                "Workspace is not trusted. Start vtcode interactively once and mark it as full auto before using exec."
            );
        }
    }

    let automation_cfg = &vt_cfg.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(
            "Automation is disabled in configuration. Enable [automation.full_auto] to continue."
        );
    }

    let model_id = ModelId::from_str(&config.model).with_context(|| {
        format!(
            "Model '{}' is not recognized for exec command. Update vtcode.toml to a supported identifier.",
            config.model
        )
    })?;

    let session_id = format!(
        "{EXEC_SESSION_PREFIX}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| anyhow!("Failed to derive session identifier timestamp: {}", err))?
            .as_secs()
    );

    let mut runner = AgentRunner::new(
        AgentType::Single,
        model_id,
        config.api_key.clone(),
        config.workspace.clone(),
        session_id,
        Some(config.reasoning_effort),
    )?;

    runner
        .apply_workspace_configuration(vt_cfg)
        .await
        .context("Failed to apply workspace configuration to exec runner")?;
    runner.enable_full_auto(&automation_cfg.allowed_tools);
    runner.set_quiet(options.json);
    if options.json {
        runner.set_event_handler(|event| match serde_json::to_string(event) {
            Ok(line) => println!("{}", line),
            Err(err) => eprintln!("Failed to serialize exec event: {err}"),
        });
    }

    let task = Task {
        id: EXEC_TASK_ID.to_string(),
        title: EXEC_TASK_TITLE.to_string(),
        description: prompt.trim().to_string(),
        instructions: None,
    };

    let result = runner
        .execute_task(&task, &[] as &[ContextItem])
        .await
        .context("Failed to execute autonomous task")?;

    let mut event_lines = Vec::new();
    for event in &result.thread_events {
        let line =
            serde_json::to_string(event).context("Failed to serialize exec event to JSON")?;
        event_lines.push(line);
    }

    if !options.json {
        if !result.summary.trim().is_empty() {
            println!(
                "{} {}",
                style("[SUMMARY]").green().bold(),
                result.summary.trim()
            );
        }
        if !result.modified_files.is_empty() {
            println!(
                "{} {}",
                style("[FILES]").cyan().bold(),
                result.modified_files.join(", ")
            );
        }
        if !result.executed_commands.is_empty() {
            println!(
                "{} {}",
                style("[COMMANDS]").cyan().bold(),
                result.executed_commands.join(", ")
            );
        }
        if !result.warnings.is_empty() {
            for warning in &result.warnings {
                println!("{} {}", style("[WARNING]").yellow().bold(), warning);
            }
        }
    }

    if let Some(path) = &options.events_path {
        let mut body = event_lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        fs::write(path, body)
            .with_context(|| format!("Failed to write exec events to {}", path.display()))?;
    }

    if let Some(path) = &options.last_message_file {
        let message = last_agent_message(&result.thread_events).unwrap_or_default();
        fs::write(path, message)
            .with_context(|| format!("Failed to write last message file {}", path.display()))?;
        if message.is_empty() {
            eprintln!(
                "Warning: no last agent message; wrote empty content to {}",
                path.display()
            );
        }
    }

    Ok(())
}
