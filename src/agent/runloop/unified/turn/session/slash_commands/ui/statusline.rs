#[path = "statusline/config.rs"]
mod config;
#[path = "statusline/input.rs"]
mod input;
#[path = "statusline/persistence.rs"]
mod persistence;
#[path = "statusline/preview.rs"]
mod preview;

use anyhow::{Context, Result};
use vtcode_commons::modal_hints::truncate_modal_text;
use vtcode_core::config::StatusLineMode;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::slash_commands::StatuslineTargetMode;
use crate::agent::runloop::unified::turn::session::slash_commands::{SlashCommandContext, SlashCommandControl};

use super::super::config_toml::preferred_workspace_config_path;
use super::{ensure_selection_ui_available, wait_for_list_modal_selection};

use config::{StatuslineSetupAction, apply_statusline_action, build_statusline_setup_items, statusline_mode_id};
use input::{parse_statusline_millis, prompt_statusline_input};
use persistence::{
    ScriptScaffoldResult, default_script_command, persist_statusline_config, scaffold_statusline_script,
    statusline_script_exists, statusline_script_path,
};
use preview::build_statusline_preview;

pub(crate) async fn handle_start_statusline_setup(
    mut ctx: SlashCommandContext<'_>,
    _instructions: Option<String>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer
            .line(MessageStyle::Info, "Status line setup is available in inline UI only.")?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ensure_selection_ui_available(&mut ctx, "configuring the status line")? {
        return Ok(SlashCommandControl::Continue);
    }

    let Some(target) = select_statusline_target(&mut ctx).await? else {
        ctx.renderer.line(MessageStyle::Info, "Status line setup cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };

    let manager =
        ConfigManager::load_from_workspace(&ctx.config.workspace).context("Failed to load VT Code configuration")?;
    let config_path = match target {
        StatuslineTargetMode::User => manager
            .preferred_user_config_path()
            .context("Could not resolve user config path for status line setup")?,
        StatuslineTargetMode::Workspace => preferred_workspace_config_path(&manager, &ctx.config.workspace),
    };
    let script_path = statusline_script_path(target, &ctx.config.workspace, &config_path);
    let script_command = default_script_command(target, &script_path);

    let mut draft = ctx.vt_cfg.as_ref().map(|cfg| cfg.ui.status_line.clone()).unwrap_or_default();

    loop {
        let script_exists = statusline_script_exists(&script_path);
        let preview = build_statusline_preview(
            &draft,
            ctx.input_status_state
                .git_summary
                .as_ref()
                .filter(|summary| !summary.branch.trim().is_empty())
                .map(|summary| (summary.branch.as_str(), summary.dirty)),
            ctx.input_status_state.thread_context.as_deref(),
            &ctx.config.model,
        );
        let config_label = match target {
            StatuslineTargetMode::User => "user",
            StatuslineTargetMode::Workspace => "workspace",
        };
        let command_label = draft
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| "(unset)".to_string());
        let mode_label = statusline_mode_id(&draft.mode);
        let clock_label = if draft.show_clock { "on" } else { "off" };

        ctx.handle.show_list_modal(
            "Status line setup".to_string(),
            vec![
                format!("Configure [ui.status_line] in the {config_label} config layer."),
                format!(
                    "Mode: {mode_label} • clock: {clock_label} • {command_label} • {preview}",
                    preview = truncate_modal_text(&preview, 48),
                ),
            ],
            build_statusline_setup_items(&draft, script_exists),
            Some(InlineListSelection::ConfigAction("statusline:save".to_string())),
            None,
        );

        let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
            ctx.renderer.line(MessageStyle::Info, "Status line setup cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            ctx.renderer
                .line(MessageStyle::Error, "Unsupported status line setup selection.")?;
            continue;
        };

        match apply_statusline_action(&action, &mut draft)? {
            StatuslineSetupAction::Continue => {}
            StatuslineSetupAction::Save => {
                persist_statusline_config(&mut ctx, &config_path, draft.clone(), target, script_path.as_path()).await?;
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Saved status line configuration to {}.", config_path.display()),
                )?;
                return Ok(SlashCommandControl::Continue);
            }
            StatuslineSetupAction::Cancel => {
                ctx.renderer.line(MessageStyle::Info, "Status line setup cancelled.")?;
                return Ok(SlashCommandControl::Continue);
            }
            StatuslineSetupAction::EditCommand => {
                let current = draft
                    .command
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
                let Some(value) = prompt_statusline_input(
                    &mut ctx,
                    "Status line command",
                    "Enter command to run with `sh -c`.",
                    "Command",
                    &script_command,
                    current,
                    false,
                )
                .await?
                else {
                    continue;
                };
                draft.command = Some(value);
                draft.mode = StatusLineMode::Command;
            }
            StatuslineSetupAction::UseScriptPath => {
                draft.command = Some(script_command.clone());
                draft.mode = StatusLineMode::Command;
                ctx.renderer
                    .line(MessageStyle::Info, &format!("Command set to `{script_command}`."))?;
            }
            StatuslineSetupAction::ClearCommand => {
                draft.command = None;
                ctx.renderer.line(MessageStyle::Info, "Cleared status line command.")?;
            }
            StatuslineSetupAction::EditRefreshInterval => {
                let default_value = draft.refresh_interval_ms.to_string();
                let Some(value) = prompt_statusline_input(
                    &mut ctx,
                    "Refresh interval",
                    "Enter status line refresh interval in milliseconds.",
                    "Refresh interval (ms)",
                    &default_value,
                    Some(default_value.clone()),
                    false,
                )
                .await?
                else {
                    continue;
                };
                draft.refresh_interval_ms = parse_statusline_millis(&value, "refresh interval")?;
            }
            StatuslineSetupAction::EditTimeout => {
                let default_value = draft.command_timeout_ms.to_string();
                let Some(value) = prompt_statusline_input(
                    &mut ctx,
                    "Command timeout",
                    "Enter command timeout in milliseconds.",
                    "Command timeout (ms)",
                    &default_value,
                    Some(default_value.clone()),
                    false,
                )
                .await?
                else {
                    continue;
                };
                draft.command_timeout_ms = parse_statusline_millis(&value, "command timeout")?;
            }
            StatuslineSetupAction::ScaffoldScript { replace_existing } => {
                match scaffold_statusline_script(target, &script_path, replace_existing)? {
                    ScriptScaffoldResult::Created => {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!("Created status line script at {}.", script_path.display()),
                        )?;
                    }
                    ScriptScaffoldResult::Replaced => {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!("Replaced status line script at {}.", script_path.display()),
                        )?;
                    }
                    ScriptScaffoldResult::SkippedExisting => {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            "Script already exists. Choose \"Replace script template\" to overwrite it.",
                        )?;
                        continue;
                    }
                }
                draft.command = Some(script_command.clone());
                draft.mode = StatusLineMode::Command;
            }
        }
    }
}

async fn select_statusline_target(ctx: &mut SlashCommandContext<'_>) -> Result<Option<StatuslineTargetMode>> {
    ctx.handle.show_list_modal(
        "Status line setup".to_string(),
        vec![
            "Choose where VT Code should persist status line changes.".to_string(),
            "User writes to your home config and ~/.config/vtcode/statusline.sh.".to_string(),
            "Workspace writes to the current workspace and .vtcode/statusline.sh.".to_string(),
        ],
        vec![
            InlineListItem {
                title: "User config".to_string(),
                subtitle: Some("Personal VT Code setup in your user config layer".to_string()),
                badge: Some("User".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction("statusline:user".to_string())),
                search_value: Some("statusline user home personal".to_string()),
                ..Default::default()
            },
            InlineListItem {
                title: "Workspace config".to_string(),
                subtitle: Some("Repo-local setup in the current workspace".to_string()),
                badge: Some("Workspace".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction("statusline:workspace".to_string())),
                search_value: Some("statusline workspace repo local".to_string()),
                ..Default::default()
            },
        ],
        Some(InlineListSelection::ConfigAction("statusline:user".to_string())),
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let target = match selection {
        InlineListSelection::ConfigAction(action) if action == "statusline:user" => StatuslineTargetMode::User,
        InlineListSelection::ConfigAction(action) if action == "statusline:workspace" => {
            StatuslineTargetMode::Workspace
        }
        _ => {
            ctx.renderer
                .line(MessageStyle::Error, "Unsupported status line setup selection.")?;
            return Ok(None);
        }
    };
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use vtcode_core::config::{StatusLineConfig, StatusLineMode};

    use super::{
        StatuslineSetupAction, apply_statusline_action, build_statusline_preview, build_statusline_setup_items,
        default_script_command,
    };
    use crate::agent::runloop::slash_commands::StatuslineTargetMode;

    #[test]
    fn statusline_action_switches_mode() {
        let mut draft = StatusLineConfig::default();

        let action = apply_statusline_action("statusline:mode:hidden", &mut draft).expect("mode action");

        assert_eq!(action, StatuslineSetupAction::Continue);
        assert_eq!(draft.mode, StatusLineMode::Hidden);
    }

    #[test]
    fn statusline_action_toggles_clock() {
        let mut draft = StatusLineConfig::default();
        assert!(draft.show_clock, "defaults to showing the clock");

        let action = apply_statusline_action("statusline:clock:toggle", &mut draft).expect("clock action");

        assert_eq!(action, StatuslineSetupAction::Continue);
        assert!(!draft.show_clock);

        let action = apply_statusline_action("statusline:clock:toggle", &mut draft).expect("clock action");
        assert_eq!(action, StatuslineSetupAction::Continue);
        assert!(draft.show_clock);
    }

    #[test]
    fn statusline_preview_includes_clock_when_enabled() {
        let draft = StatusLineConfig::default();

        let preview = build_statusline_preview(&draft, None, Some("thread-1"), "gpt-5.6-sol");

        assert!(preview.contains("thread-1"), "preview: {preview}");
        assert!(preview.contains("14:30:05"), "preview must include the clock: {preview}");
    }

    #[test]
    fn statusline_preview_omits_clock_when_disabled() {
        let draft = StatusLineConfig { show_clock: false, ..StatusLineConfig::default() };

        let preview = build_statusline_preview(&draft, None, Some("thread-1"), "gpt-5.6-sol");

        assert!(!preview.contains("14:30:05"), "preview must omit the clock: {preview}");
    }

    #[test]
    fn statusline_preview_in_command_mode_never_executes() {
        let draft = StatusLineConfig {
            mode: StatusLineMode::Command,
            command: Some(".vtcode/statusline.sh".to_string()),
            ..StatusLineConfig::default()
        };

        let preview = build_statusline_preview(&draft, Some(("main", false)), Some("thread-1"), "gpt-5.6-sol");

        assert_eq!(preview, "command mode (setup does not execute command): .vtcode/statusline.sh");
    }

    #[test]
    fn statusline_items_offer_replace_when_script_exists() {
        let draft = StatusLineConfig::default();
        let items = build_statusline_setup_items(&draft, true);

        assert!(items.iter().any(|item| item.title == "Replace script template"));
        assert!(!items.iter().any(|item| item.title == "Create script template"));
    }

    #[test]
    fn user_script_command_is_shell_quoted() {
        let command = default_script_command(StatuslineTargetMode::User, Path::new("/tmp/status line's/script.sh"));

        assert_eq!(command, "'/tmp/status line'\\''s/script.sh'");
    }
}
