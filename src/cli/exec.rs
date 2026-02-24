use anyhow::{Context, Result, bail};
use std::io::{self, Read};
use std::path::PathBuf;
use std::str::FromStr;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, RunnerSettings};
use vtcode_core::core::agent::task::{ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;
use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails};
use vtcode_core::utils::colors::style;
use vtcode_core::utils::file_utils::write_file_with_context;
use vtcode_core::utils::tty::TtyExt;
use vtcode_core::utils::validation::validate_non_empty;

use crate::workspace_trust::workspace_trust_level;

const EXEC_SESSION_PREFIX: &str = "exec-task";
const EXEC_TASK_ID: &str = "exec-task";
const EXEC_TASK_TITLE: &str = "Exec Task";
const EXEC_TASK_INSTRUCTIONS: &str = "You are running vtcode in non-interactive exec mode. Complete the task autonomously using the configured full-auto tool allowlist. Do not request additional user input, confirmations, or allowances—operate solely with the provided information and available tools. Provide a concise summary of the outcome when finished.";

#[derive(Debug, Clone)]
pub struct ExecCommandOptions {
    pub json: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
}

fn resolve_prompt(prompt_arg: Option<String>, quiet: bool) -> Result<String> {
    match prompt_arg {
        Some(p) if p != "-" => Ok(p),
        maybe_dash => {
            let force_stdin = matches!(maybe_dash.as_deref(), Some("-"));
            if io::stdin().is_tty_ext() && !force_stdin {
                bail!(
                    "No prompt provided. Pass a prompt argument, pipe input, or use '-' to read from stdin."
                );
            }
            if !force_stdin && !quiet {
                eprintln!("Reading prompt from stdin...");
            }
            // OPTIMIZATION: Pre-allocate buffer with reasonable capacity
            let mut buffer = String::with_capacity(1024);
            io::stdin()
                .read_to_string(&mut buffer)
                .context("Failed to read prompt from stdin")?;
            validate_non_empty(&buffer, "Prompt via stdin")?;
            Ok(buffer)
        }
    }
}

// OPTIMIZATION: Use inline hint for hot path
#[inline]
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
    tokio::select! {
        res = handle_exec_command_impl(config, vt_cfg, options, prompt_arg) => res,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("{}", style("\nCancelled by user.").red());
            bail!("Operation cancelled");
        }
    }
}

async fn handle_exec_command_impl(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    options: ExecCommandOptions,
    prompt_arg: Option<String>,
) -> Result<()> {
    let prompt = resolve_prompt(prompt_arg, config.quiet)?;

    let trust_level = workspace_trust_level(&config.workspace)
        .await
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

    // OPTIMIZATION: Use context instead of map_err with anyhow!
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("Failed to derive session identifier timestamp")?
        .as_secs();
    let session_id = format!("{EXEC_SESSION_PREFIX}-{timestamp}");

    let mut runner = AgentRunner::new(
        AgentType::Single,
        model_id,
        config.api_key.clone(),
        config.workspace.clone(),
        session_id,
        RunnerSettings {
            reasoning_effort: Some(config.reasoning_effort),
            verbosity: None,
        },
        None,
    )
    .await?;

    runner
        .apply_workspace_configuration(vt_cfg)
        .await
        .context("Failed to apply workspace configuration to exec runner")?;
    runner.enable_full_auto(&automation_cfg.allowed_tools).await;
    runner.set_quiet(options.json);
    if options.json {
        runner.set_event_handler(|event| match serde_json::to_string(event) {
            Ok(line) => println!("{}", line),
            Err(err) => eprintln!("Failed to serialize exec event: {err}"),
        });
    }

    // OPTIMIZATION: Avoid unnecessary allocations for static strings
    let task = Task {
        id: EXEC_TASK_ID.into(),
        title: EXEC_TASK_TITLE.into(),
        description: prompt.trim().to_string(),
        instructions: Some(EXEC_TASK_INSTRUCTIONS.into()),
    };

    let max_retries = vt_cfg.agent.max_task_retries;
    let result = runner
        .execute_task_with_retry(&task, &[] as &[ContextItem], max_retries)
        .await
        .context("Failed to execute autonomous task after retries")?;

    // OPTIMIZATION: Pre-allocate with capacity hint
    let mut event_lines = Vec::with_capacity(result.thread_events.len());
    for event in &result.thread_events {
        let line =
            serde_json::to_string(event).context("Failed to serialize exec event to JSON")?;
        event_lines.push(line);
    }

    if !options.json && !config.quiet {
        eprintln!();

        if !result.summary.trim().is_empty() {
            eprintln!(
                "{} {}\n",
                style("[SUMMARY]").green().bold(),
                result.summary.trim()
            );
        }

        // OPTIMIZATION: Use static str instead of allocating "-"
        let avg_display = result
            .average_turn_duration_ms
            .map(|avg| format!("{avg:.1}"))
            .unwrap_or_else(|| "-".into());
        let max_display = result
            .max_turn_duration_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());

        eprintln!("{}", style("[OUTCOME]").magenta().bold());
        eprintln!("  {:16} {}", "outcome", result.outcome);
        eprintln!("  {:16} {}", "turns", result.turns_executed);
        eprintln!("  {:16} {}", "duration_ms", result.total_duration_ms);
        eprintln!("  {:16} {}", "avg_turn_ms", avg_display);
        eprintln!("  {:16} {}", "max_turn_ms", max_display);
        eprintln!("  {:16} {}\n", "warnings", result.warnings.len());

        // OPTIMIZATION: Extract common pattern to reduce code duplication
        if !result.modified_files.is_empty() {
            eprintln!("{}", style("[FILES]").cyan().bold());
            for (idx, file) in result.modified_files.iter().enumerate() {
                eprintln!("  {:>2}. {}", idx + 1, file);
            }
            eprintln!();
        }

        if !result.executed_commands.is_empty() {
            eprintln!("{}", style("[COMMANDS]").cyan().bold());
            for (idx, cmd) in result.executed_commands.iter().enumerate() {
                eprintln!("  {:>2}. {}", idx + 1, cmd);
            }
            eprintln!();
        }

        if !result.warnings.is_empty() {
            eprintln!("{}", style("[WARNINGS]").red().bold());
            for (idx, warning) in result.warnings.iter().enumerate() {
                eprintln!("  {:>2}. {}", idx + 1, warning);
            }
            eprintln!();
        }
    }

    if let Some(path) = &options.events_path {
        let mut body = event_lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        write_file_with_context(path, &body, "exec events").await?;
    }

    if let Some(path) = &options.last_message_file {
        let message = last_agent_message(&result.thread_events).unwrap_or_default();
        write_file_with_context(path, message, "last message file").await?;
        if message.is_empty() {
            eprintln!(
                "Warning: no last agent message; wrote empty content to {}",
                path.display()
            );
        }
    }

    Ok(())
}
