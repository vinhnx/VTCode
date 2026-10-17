//! Slash command handler for in-chat Agent Plugins management.
//!
//! Wires `/plugin` into the interactive session: renders text-mode outcomes,
//! and in TUI sessions offers an interactive manager modal for browsing,
//! installing, and removing portable Agent Plugins.

use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use anyhow::{Context, Result};
use std::sync::Arc;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode};

use crate::agent::runloop::unified::session_setup::session_mcp_config;
use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};
use crate::agent::runloop::{PluginCommandAction, PluginCommandOutcome, handle_plugin_command};

use super::mcp::apply_manual_mcp_refresh;
use super::{SlashCommandContext, SlashCommandControl};

const PLUGIN_ACTION_PREFIX: &str = "plugins.action.";
const PLUGIN_ACTION_BACK: &str = "plugins.action.back";
const PLUGIN_PICK_PREFIX: &str = "plugins.pick.";
const PLUGIN_PICK_BACK_ACTION: &str = "plugins.pick.back";
const PLUGIN_ADD_SOURCE_QUESTION_ID: &str = "plugins.add.source";

pub(crate) async fn handle_manage_plugins(
    mut ctx: SlashCommandContext<'_>,
    action: PluginCommandAction,
) -> Result<SlashCommandControl> {
    if matches!(action, PluginCommandAction::Interactive) {
        return run_interactive_plugins_manager(&mut ctx).await;
    }

    if matches!(action, PluginCommandAction::Refresh) {
        refresh_plugin_mcp_runtime(&mut ctx).await?;
        return Ok(SlashCommandControl::Continue);
    }

    let outcome = handle_plugin_command(action, ctx.config.workspace.clone()).await?;
    if matches!(outcome, PluginCommandOutcome::Installed { .. } | PluginCommandOutcome::Removed { .. }) {
        refresh_plugin_mcp_runtime(&mut ctx).await?;
    }
    apply_plugin_command_outcome(&mut ctx, outcome).await
}

/// Rebuild the live MCP configuration so newly installed or removed plugin
/// `mcp.json` providers take effect without restarting the session.
async fn refresh_plugin_mcp_runtime(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let Some(manager) = ctx.async_mcp_manager.map(Arc::clone) else {
        ctx.renderer.line(
            MessageStyle::Info,
            "MCP is not active in this session; plugin MCP providers will be discovered at the next session start.",
        )?;
        return Ok(());
    };

    let new_config =
        session_mcp_config(ctx.vt_cfg.as_ref(), Some(ctx.active_primary_agent.active()), &ctx.config.workspace);
    let restarted = manager.reconfigure_active_runtime(new_config).await?;

    if restarted {
        // Refresh the tool snapshot so any plugin MCP tools become visible.
        apply_manual_mcp_refresh(ctx, "plugin_refresh").await;
        ctx.renderer
            .line(MessageStyle::Info, "MCP runtime reconfigured; plugin MCP providers are reconnecting.")?;
    } else {
        ctx.renderer
            .line(MessageStyle::Info, "Plugin MCP providers will be picked up at the next MCP initialization.")?;
    }
    Ok(())
}

async fn apply_plugin_command_outcome(
    ctx: &mut SlashCommandContext<'_>,
    outcome: PluginCommandOutcome,
) -> Result<SlashCommandControl> {
    match outcome {
        PluginCommandOutcome::Handled { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        PluginCommandOutcome::Installed { message } => {
            ctx.renderer.line(MessageStyle::Output, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        PluginCommandOutcome::Removed { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        PluginCommandOutcome::Error { message } => {
            ctx.renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

async fn execute_plugin_action(ctx: &mut SlashCommandContext<'_>, action: PluginCommandAction) -> Result<()> {
    let outcome = handle_plugin_command(action, ctx.config.workspace.clone()).await?;
    let _ = apply_plugin_command_outcome(ctx, outcome).await?;
    Ok(())
}

async fn run_interactive_plugins_manager(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        execute_plugin_action(ctx, PluginCommandAction::Help).await?;
        return Ok(SlashCommandControl::Continue);
    }

    loop {
        show_plugins_manager_actions_modal(ctx);
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };

        if action == PLUGIN_ACTION_BACK {
            return Ok(SlashCommandControl::Continue);
        }

        let Some(action_key) = action.strip_prefix(PLUGIN_ACTION_PREFIX) else {
            continue;
        };

        match action_key {
            "list" => execute_plugin_action(ctx, PluginCommandAction::List).await?,
            "info" => {
                if let Some(name) = pick_installed_plugin(ctx, "Plugin Details", "Select a plugin to inspect.").await? {
                    execute_plugin_action(ctx, PluginCommandAction::Info { name }).await?;
                }
            }
            "add" => {
                if let Some(source) = prompt_source(ctx).await? {
                    execute_plugin_action(ctx, PluginCommandAction::Add { source, name: None }).await?;
                }
            }
            "remove" => {
                if let Some(name) = pick_installed_plugin(ctx, "Remove Plugin", "Select a plugin to uninstall.").await?
                {
                    execute_plugin_action(ctx, PluginCommandAction::Remove { name }).await?;
                }
            }
            "refresh" => execute_plugin_action(ctx, PluginCommandAction::Refresh).await?,
            "help" => execute_plugin_action(ctx, PluginCommandAction::Help).await?,
            _ => continue,
        }
    }
}

async fn prompt_source(ctx: &mut SlashCommandContext<'_>) -> Result<Option<String>> {
    let step = vtcode_ui::tui::app::WizardStep {
        title: "Source".to_string(),
        question: "Provide a git URL or local directory to install the plugin from.".to_string(),
        items: vec![
            ui_list::choice(
                "Submit",
                Some("Press Tab to type the source, then Enter to install.".to_string()),
                Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: PLUGIN_ADD_SOURCE_QUESTION_ID.to_string(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
            )
            .with_search_value("submit source".to_string()),
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some("Source".to_string()),
        freeform_placeholder: Some("https://github.com/org/plugin or /local/path".to_string()),
        freeform_default: None,
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        "Add Plugin".to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => selections.into_iter().find_map(|selection| match selection {
            InlineListSelection::RequestUserInputAnswer { question_id, selected, other }
                if question_id == PLUGIN_ADD_SOURCE_QUESTION_ID =>
            {
                other.or_else(|| selected.first().cloned())
            }
            _ => None,
        }),
        WizardModalOutcome::Cancelled { .. } => None,
    };

    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "Plugin source was empty. Nothing installed.")?;
        return Ok(None);
    }
    Ok(Some(trimmed))
}

async fn pick_installed_plugin(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    description: &str,
) -> Result<Option<String>> {
    let workspace = ctx.config.workspace.clone();
    // `discover_installed_plugins` does recursive `read_dir` + manifest loads;
    // run it off the async executor. See `# Blocking` docs in
    // `src/agent/runloop/git.rs`.
    let entries = tokio::task::spawn_blocking(move || {
        crate::agent::runloop::plugin_commands::discover_installed_plugins(&workspace)
    })
    .await
    .context("plugin discovery task panicked")?;
    if entries.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No installed plugins. Add one with 'Add plugin' or '/plugin add <source>'.")?;
        return Ok(None);
    }

    let mut items: Vec<InlineListItem> = entries
        .iter()
        .map(|entry| {
            let version = entry.version.as_deref().unwrap_or("unknown");
            ui_list::action(
                entry.name.clone(),
                format!(
                    "v{version} — {} skill(s), {} MCP server(s) — {}",
                    entry.skill_count,
                    entry.mcp_server_count,
                    entry.scope.label()
                ),
                None,
                Tone::Neutral,
                Some(InlineListSelection::ConfigAction(format!("{}{}", PLUGIN_PICK_PREFIX, entry.name))),
            )
            .with_search_value(format!("{} {} {}", entry.name, entry.description, entry.scope.label()))
        })
        .collect();

    items.push(
        ui_list::action(
            "Back",
            "Cancel and return".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(PLUGIN_PICK_BACK_ACTION.to_string())),
        )
        .with_search_value("back cancel".to_string()),
    );

    ctx.renderer.show_list_modal(
        title,
        vec![description.to_string()],
        items,
        entries
            .first()
            .map(|entry| InlineListSelection::ConfigAction(format!("{}{}", PLUGIN_PICK_PREFIX, entry.name))),
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("plugin name".to_string()),
            fuzzy: false,
        }),
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    if action == PLUGIN_PICK_BACK_ACTION {
        return Ok(None);
    }
    Ok(action.strip_prefix(PLUGIN_PICK_PREFIX).map(ToString::to_string))
}

fn show_plugins_manager_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let items = vec![
        InlineListItem::group_header("Actions"),
        ui_list::action(
            "List plugins",
            "Show all installed plugins".to_string(),
            Some("Recommended".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}list"))),
        )
        .with_search_value("list plugins".to_string()),
        ui_list::action(
            "Add plugin",
            "Install a plugin from a git URL or local directory".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}add"))),
        )
        .with_search_value("install add plugin".to_string()),
        ui_list::action(
            "Remove plugin",
            "Uninstall a plugin".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}remove"))),
        )
        .with_search_value("remove uninstall plugin".to_string()),
        ui_list::action(
            "Plugin details",
            "Show metadata and components for a plugin".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}info"))),
        )
        .with_search_value("details info metadata plugin".to_string()),
        ui_list::action(
            "Refresh MCP providers",
            "Re-discover plugin-provided MCP servers".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}refresh"))),
        )
        .with_search_value("refresh mcp providers".to_string()),
        ui_list::action(
            "Show help",
            "Display `/plugin` command help".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}help"))),
        )
        .with_search_value("help commands plugin".to_string()),
        ui_list::action(
            "Back",
            "Close plugin manager".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(PLUGIN_ACTION_BACK.to_string())),
        )
        .with_search_value("back close".to_string()),
    ];

    ctx.renderer.show_list_modal(
        "Plugin Manager",
        vec!["Manage portable Agent Plugins interactively.".to_string()],
        items,
        Some(InlineListSelection::ConfigAction(format!("{PLUGIN_ACTION_PREFIX}list"))),
        None,
    );
}
