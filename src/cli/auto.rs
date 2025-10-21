use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use console::style;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::models::ModelId;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runner::{AgentRunner, ContextItem, Task};
use vtcode_core::core::agent::types::AgentType;

use crate::workspace_trust::workspace_trust_level;

const AUTO_SESSION_PREFIX: &str = "auto-task";
const AUTO_TASK_ID: &str = "auto-task";
const AUTO_TASK_TITLE: &str = "Autonomous Task";

pub async fn handle_auto_task_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    prompt: &str,
) -> Result<()> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        bail!("Automation prompt is empty. Provide instructions after --auto/--full-auto.");
    }

    let trust_level = workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level")?;

    match trust_level {
        Some(WorkspaceTrustLevel::FullAuto) => {}
        Some(level) => {
            bail!(
                "Workspace trust level '{level}' does not permit autonomous runs. Start an interactive \
                 session and upgrade trust to full auto before using --auto/--full-auto."
            );
        }
        None => {
            bail!(
                "Workspace is not trusted. Start vtcode interactively once and mark the workspace \
                 as full auto before using --auto/--full-auto."
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
            "Model '{}' is not recognized for autonomous execution. Update vtcode.toml to a \
             supported identifier.",
            config.model
        )
    })?;

    let session_id = format!(
        "{AUTO_SESSION_PREFIX}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
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

    runner.enable_full_auto(&automation_cfg.allowed_tools);

    let task = Task {
        id: AUTO_TASK_ID.to_string(),
        title: AUTO_TASK_TITLE.to_string(),
        description: trimmed.to_string(),
        instructions: None,
    };

    let result = runner
        .execute_task(&task, &[] as &[ContextItem])
        .await
        .context("Failed to execute autonomous task")?;

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
        for warning in result.warnings {
            println!("{} {}", style("[WARNING]").yellow().bold(), warning);
        }
    }

    Ok(())
}
