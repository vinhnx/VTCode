use anyhow::{Result, Context};
use vtcode_core::commands::init::{GenerateAgentsFileStatus, generate_agents_file};
use vtcode_core::utils::ansi::MessageStyle;
use crate::agent::runloop::unified::workspace_links::{
    handle_workspace_directory_command, LinkedDirectoryCommand,
};
use super::super::super::workspace::{bootstrap_config_files, build_workspace_index};

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_initialize_workspace(
    ctx: &SlashCommandContext<'_>,
    force: bool,
) -> Result<SlashCommandControl> {
    let workspace_path = ctx.config.workspace.clone();
    let workspace_label = workspace_path.display().to_string();
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Initializing vtcode configuration in {}...",
            workspace_label
        ),
    )?;
    let created_files = match bootstrap_config_files(workspace_path.clone(), force).await {
        Ok(files) => files,
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to initialize configuration: {}", err),
            )?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    if created_files.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Existing configuration detected; no files were changed.",
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Created {}: {}",
                if created_files.len() == 1 {
                    "file"
                } else {
                    "files"
                },
                created_files.join(", "),
            ),
        )?;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        "Indexing workspace context (this may take a moment)...",
    )?;
    match build_workspace_index(workspace_path).await {
        Ok(()) => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Workspace indexing complete. Stored under .vtcode/index.",
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to index workspace: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_generate_agent_file(
    ctx: &SlashCommandContext<'_>,
    overwrite: bool,
) -> Result<SlashCommandControl> {
    let workspace_path = ctx.config.workspace.clone();
    ctx.renderer.line(
        MessageStyle::Info,
        "Generating AGENTS.md guidance. This may take a moment...",
    )?;
    match generate_agents_file(ctx.tool_registry, workspace_path.as_path(), overwrite).await
    {
        Ok(report) => match report.status {
            GenerateAgentsFileStatus::Created => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Created AGENTS.md at {}", report.path.display()),
                )?;
            }
            GenerateAgentsFileStatus::Overwritten => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Overwrote existing AGENTS.md at {}", report.path.display()),
                )?;
            }
            GenerateAgentsFileStatus::SkippedExisting => {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "AGENTS.md already exists at {}. Use /generate-agent-file --force to regenerate it.",
                        report.path.display()
                    ),
                )?;
            }
        },
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to generate AGENTS.md guidance: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_manage_workspace_directories(
    ctx: &SlashCommandContext<'_>,
    command: LinkedDirectoryCommand,
) -> Result<SlashCommandControl> {
    handle_workspace_directory_command(
        ctx.renderer,
        &ctx.config.workspace,
        command,
        ctx.linked_directories,
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_start_file_browser(
    ctx: &SlashCommandContext<'_>,
    initial_filter: Option<String>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Close the active model picker before opening file browser.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    ctx.handle.force_redraw();
    if let Some(filter) = initial_filter {
        ctx.handle.set_input(format!("@{}", filter));
    } else {
        ctx.handle.set_input("@".to_string());
    }
    ctx.renderer.line(
        MessageStyle::Info,
        "File browser activated. Use arrow keys to navigate, Enter to select, Esc to close.",
    )?;
    Ok(SlashCommandControl::Continue)
}
