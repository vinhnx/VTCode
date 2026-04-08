use anyhow::Result;
use anyhow::{Context, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::task;
use toml::Value as TomlValue;
use vtcode_core::config::current_config_defaults;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::loader::layers::ConfigLayerSource;
use vtcode_core::config::{DEFAULT_TERMINAL_TITLE_ITEMS, StatusLineConfig, StatusLineMode};
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::ui::inline_theme_from_core_styles;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSelection, TransientSubmission, WizardModalMode, WizardStep,
};

use crate::agent::runloop::model_picker::{ModelPickerStart, ModelPickerState};
use crate::agent::runloop::slash_commands::{SessionPaletteMode, StatuslineTargetMode};
use crate::agent::runloop::unified::display::{
    persist_theme_preference, sync_runtime_theme_selection,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::overlay_prompt::{
    OverlayWaitOutcome, wait_for_overlay_submission,
};
use crate::agent::runloop::unified::palettes::{
    ActivePalette, apply_prompt_style, build_lightweight_palette_view,
    refresh_runtime_config_from_manager, show_lightweight_model_palette, show_model_target_palette,
    show_sessions_palette, show_theme_palette,
};
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::config_toml::{
    ensure_child_table, load_toml_value, preferred_workspace_config_path, save_toml_value,
};
use super::{SlashCommandContext, SlashCommandControl};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalTitleItemSpec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
}

const TERMINAL_TITLE_ITEM_SPECS: [TerminalTitleItemSpec; 8] = [
    TerminalTitleItemSpec {
        id: "app-name",
        title: "App name",
        description: "VT Code branding",
    },
    TerminalTitleItemSpec {
        id: "project",
        title: "Project",
        description: "Workspace folder name",
    },
    TerminalTitleItemSpec {
        id: "spinner",
        title: "Spinner",
        description: "Activity indicator",
    },
    TerminalTitleItemSpec {
        id: "status",
        title: "Status",
        description: "Ready, Thinking, Working, Waiting, Undoing, or Action Required",
    },
    TerminalTitleItemSpec {
        id: "thread",
        title: "Thread",
        description: "Current thread label",
    },
    TerminalTitleItemSpec {
        id: "git-branch",
        title: "Git branch",
        description: "Active branch name",
    },
    TerminalTitleItemSpec {
        id: "model",
        title: "Model",
        description: "Current model id",
    },
    TerminalTitleItemSpec {
        id: "task-progress",
        title: "Task progress",
        description: "Latest task tracker progress summary",
    },
];

const STATUSLINE_INPUT_ID: &str = "statusline.input";
const STATUSLINE_SCRIPT_FILE_NAME: &str = "statusline.sh";

const STATUSLINE_SCRIPT_TEMPLATE: &str = r#"#!/bin/sh
payload="$(cat)"
branch="$(printf '%s' "$payload" | jq -r '.git.branch // ""' 2>/dev/null)"
dirty="$(printf '%s' "$payload" | jq -r '.git.dirty // false' 2>/dev/null)"
model="$(printf '%s' "$payload" | jq -r '.model.display_name // .model.id // ""' 2>/dev/null)"
reasoning="$(printf '%s' "$payload" | jq -r '.runtime.reasoning_effort // ""' 2>/dev/null)"

git_part=""
if [ -n "$branch" ]; then
  git_part="$branch"
  if [ "$dirty" = "true" ]; then
    git_part="$git_part*"
  fi
fi

model_part="$model"
if [ -n "$reasoning" ]; then
  model_part="$model_part ($reasoning)"
fi

if [ -n "$git_part" ] && [ -n "$model_part" ]; then
  printf '%s | %s\n' "$git_part" "$model_part"
elif [ -n "$git_part" ]; then
  printf '%s\n' "$git_part"
elif [ -n "$model_part" ]; then
  printf '%s\n' "$model_part"
fi
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatuslineSetupAction {
    Continue,
    Save,
    Cancel,
    EditCommand,
    UseScriptPath,
    ClearCommand,
    EditRefreshInterval,
    EditTimeout,
    ScaffoldScript { replace_existing: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScriptScaffoldResult {
    Created,
    Replaced,
    SkippedExisting,
}

pub(super) fn ensure_selection_ui_available(
    ctx: &mut SlashCommandContext<'_>,
    activity: &str,
) -> Result<bool> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Close the active model picker before {}.", activity),
        )?;
        return Ok(false);
    }
    if ctx.palette_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Another selection modal is already open. Press Esc to dismiss it before starting a new one.",
        )?;
        return Ok(false);
    }
    Ok(true)
}

pub(super) async fn wait_for_list_modal_selection(
    ctx: &mut SlashCommandContext<'_>,
) -> Option<InlineListSelection> {
    let outcome: OverlayWaitOutcome<InlineListSelection> = wait_for_overlay_submission(
        ctx.handle,
        ctx.session,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(selection) => Some(selection),
            _ => None,
        },
    )
    .await
    .ok()?;

    close_list_modal(ctx).await;

    match outcome {
        OverlayWaitOutcome::Submitted(selection) => Some(selection),
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => None,
    }
}

async fn close_list_modal(ctx: &mut SlashCommandContext<'_>) {
    ctx.handle.close_modal();
    ctx.handle.force_redraw();
    task::yield_now().await;
}

pub(crate) async fn handle_theme_changed(
    ctx: SlashCommandContext<'_>,
    theme_id: String,
) -> Result<SlashCommandControl> {
    sync_runtime_theme_selection(ctx.config, ctx.vt_cfg.as_mut(), &theme_id);
    persist_theme_preference(ctx.renderer, &ctx.config.workspace, &theme_id).await?;
    let styles = theme::active_styles();
    ctx.handle.set_theme(inline_theme_from_core_styles(&styles));
    apply_prompt_style(ctx.handle);
    ctx.handle.force_redraw();
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_theme_palette(
    mut ctx: SlashCommandContext<'_>,
    mode: crate::agent::runloop::slash_commands::ThemePaletteMode,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "selecting a theme")? {
        return Ok(SlashCommandControl::Continue);
    }
    if show_theme_palette(ctx.renderer, mode)? {
        *ctx.palette_state = Some(ActivePalette::Theme {
            mode,
            original_theme_id: theme::active_theme_id(),
        });
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_session_palette(
    mut ctx: SlashCommandContext<'_>,
    mode: SessionPaletteMode,
    limit: usize,
    show_all: bool,
) -> Result<SlashCommandControl> {
    let activity = match mode {
        SessionPaletteMode::Resume => "browsing sessions",
        SessionPaletteMode::Fork => "selecting a session to fork",
    };
    if !ensure_selection_ui_available(&mut ctx, activity)? {
        return Ok(SlashCommandControl::Continue);
    }
    let scope = if show_all {
        SessionQueryScope::All
    } else {
        SessionQueryScope::CurrentWorkspace(ctx.config.workspace.clone())
    };

    match list_recent_sessions_in_scope(limit, &scope).await {
        Ok(listings) => {
            if show_sessions_palette(ctx.renderer, mode, &listings, limit, show_all)? {
                *ctx.palette_state = Some(ActivePalette::Sessions {
                    mode,
                    listings,
                    limit,
                    show_all,
                });
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to load session archives: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_history_picker(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Command history picker is available in inline UI only. Use /resume for archived sessions.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ensure_selection_ui_available(&mut ctx, "opening command history")? {
        return Ok(SlashCommandControl::Continue);
    }

    ctx.handle.show_history_picker();
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_file_browser(
    mut ctx: SlashCommandContext<'_>,
    initial_filter: Option<String>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "opening file browser")? {
        return Ok(SlashCommandControl::Continue);
    }
    // Ensure stale inline modal state cannot overlap with the file palette overlay.
    ctx.handle.close_modal();
    ctx.handle.force_redraw();
    if let Some(filter) = initial_filter {
        ctx.handle.set_input(format!("@{}", filter));
    } else {
        ctx.handle.set_input("@".to_string());
    }
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_statusline_setup(
    mut ctx: SlashCommandContext<'_>,
    _instructions: Option<String>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Status line setup is available in inline UI only.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !ensure_selection_ui_available(&mut ctx, "configuring the status line")? {
        return Ok(SlashCommandControl::Continue);
    }

    let Some(target) = select_statusline_target(&mut ctx).await? else {
        ctx.renderer
            .line(MessageStyle::Info, "Status line setup cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    };

    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let config_path = match target {
        StatuslineTargetMode::User => preferred_user_config_path(&manager)
            .context("Could not resolve user config path for status line setup")?,
        StatuslineTargetMode::Workspace => {
            preferred_workspace_config_path(&manager, &ctx.config.workspace)
        }
    };
    let script_path = statusline_script_path(target, &ctx.config.workspace, &config_path);
    let script_command = default_script_command(target, &script_path);

    let mut draft = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.status_line.clone())
        .unwrap_or_default();

    loop {
        let script_exists = script_path.exists();
        let preview = build_statusline_preview(
            &draft,
            ctx.input_status_state
                .git_summary
                .as_ref()
                .filter(|summary| !summary.branch.trim().is_empty())
                .map(|summary| (summary.branch.as_str(), summary.dirty)),
            ctx.input_status_state.thread_context.as_deref(),
            ctx.input_status_state.ide_context_source.as_deref(),
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
        let script_state = if script_exists { "present" } else { "missing" };

        ctx.handle.show_list_modal(
            "Status line setup".to_string(),
            vec![
                format!("Configure [ui.status_line] in the {config_label} config layer."),
                format!(
                    "Mode: {mode_label} | command: {command_label} | refresh: {}ms | timeout: {}ms",
                    draft.refresh_interval_ms, draft.command_timeout_ms
                ),
                format!("Script: {} ({script_state})", script_path.display()),
                format!("Preview: {preview}"),
            ],
            build_statusline_setup_items(&draft, script_exists),
            Some(InlineListSelection::ConfigAction(
                "statusline:save".to_string(),
            )),
            None,
        );

        let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
            ctx.renderer
                .line(MessageStyle::Info, "Status line setup cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported status line setup selection.",
            )?;
            continue;
        };

        match apply_statusline_action(&action, &mut draft)? {
            StatuslineSetupAction::Continue => {}
            StatuslineSetupAction::Save => {
                persist_statusline_config(
                    &mut ctx,
                    &config_path,
                    draft.clone(),
                    target,
                    script_path.as_path(),
                )
                .await?;
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Saved status line configuration to {}.",
                        config_path.display()
                    ),
                )?;
                return Ok(SlashCommandControl::Continue);
            }
            StatuslineSetupAction::Cancel => {
                ctx.renderer
                    .line(MessageStyle::Info, "Status line setup cancelled.")?;
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
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Command set to `{}`.", script_command),
                )?;
            }
            StatuslineSetupAction::ClearCommand => {
                draft.command = None;
                ctx.renderer
                    .line(MessageStyle::Info, "Cleared status line command.")?;
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
                match scaffold_statusline_script(&script_path, replace_existing)? {
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

async fn select_statusline_target(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<StatuslineTargetMode>> {
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
                selection: Some(InlineListSelection::ConfigAction(
                    "statusline:user".to_string(),
                )),
                search_value: Some("statusline user home personal".to_string()),
            },
            InlineListItem {
                title: "Workspace config".to_string(),
                subtitle: Some("Repo-local setup in the current workspace".to_string()),
                badge: Some("Workspace".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "statusline:workspace".to_string(),
                )),
                search_value: Some("statusline workspace repo local".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "statusline:user".to_string(),
        )),
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let target = match selection {
        InlineListSelection::ConfigAction(action) if action == "statusline:user" => {
            StatuslineTargetMode::User
        }
        InlineListSelection::ConfigAction(action) if action == "statusline:workspace" => {
            StatuslineTargetMode::Workspace
        }
        _ => {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported status line setup selection.",
            )?;
            return Ok(None);
        }
    };
    Ok(Some(target))
}

fn build_statusline_setup_items(
    draft: &StatusLineConfig,
    script_exists: bool,
) -> Vec<InlineListItem> {
    let mut items = Vec::new();

    for (mode, label, subtitle) in [
        (
            StatusLineMode::Auto,
            "Use auto mode",
            "Show VT Code-built status components.",
        ),
        (
            StatusLineMode::Command,
            "Use command mode",
            "Run a shell command and render its first output line.",
        ),
        (
            StatusLineMode::Hidden,
            "Hide status line",
            "Disable the bottom status line.",
        ),
    ] {
        let active = draft.mode == mode;
        items.push(InlineListItem {
            title: label.to_string(),
            subtitle: Some(subtitle.to_string()),
            badge: Some(if active {
                "Active".to_string()
            } else {
                "Mode".to_string()
            }),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "statusline:mode:{}",
                statusline_mode_id(&mode)
            ))),
            search_value: Some(format!("statusline mode {}", statusline_mode_id(&mode))),
        });
    }

    items.push(InlineListItem {
        title: "Edit command".to_string(),
        subtitle: Some("Set the shell command for command mode.".to_string()),
        badge: Some("Command".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:command:edit".to_string(),
        )),
        search_value: Some("statusline command edit".to_string()),
    });
    items.push(InlineListItem {
        title: "Use scaffold script path".to_string(),
        subtitle: Some("Point command to the target statusline.sh script.".to_string()),
        badge: Some("Command".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:command:script".to_string(),
        )),
        search_value: Some("statusline command script".to_string()),
    });
    if draft
        .command
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        items.push(InlineListItem {
            title: "Clear command".to_string(),
            subtitle: Some("Remove command so command mode falls back to auto.".to_string()),
            badge: Some("Command".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "statusline:command:clear".to_string(),
            )),
            search_value: Some("statusline command clear".to_string()),
        });
    }

    if script_exists {
        items.push(InlineListItem {
            title: "Replace script template".to_string(),
            subtitle: Some(
                "Overwrite existing statusline.sh with the default template.".to_string(),
            ),
            badge: Some("Script".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "statusline:script:replace".to_string(),
            )),
            search_value: Some("statusline script replace".to_string()),
        });
    } else {
        items.push(InlineListItem {
            title: "Create script template".to_string(),
            subtitle: Some(
                "Create statusline.sh using the default JSON payload template.".to_string(),
            ),
            badge: Some("Script".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "statusline:script:create".to_string(),
            )),
            search_value: Some("statusline script create".to_string()),
        });
    }

    items.push(InlineListItem {
        title: format!("Refresh interval: {}ms", draft.refresh_interval_ms),
        subtitle: Some("Set command refresh cadence.".to_string()),
        badge: Some("Timing".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:refresh:edit".to_string(),
        )),
        search_value: Some("statusline refresh interval".to_string()),
    });
    items.push(InlineListItem {
        title: format!("Command timeout: {}ms", draft.command_timeout_ms),
        subtitle: Some("Set command execution timeout.".to_string()),
        badge: Some("Timing".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:timeout:edit".to_string(),
        )),
        search_value: Some("statusline timeout".to_string()),
    });
    items.push(InlineListItem {
        title: "Save changes".to_string(),
        subtitle: Some("Persist [ui.status_line] changes.".to_string()),
        badge: Some("Save".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:save".to_string(),
        )),
        search_value: Some("statusline save".to_string()),
    });
    items.push(InlineListItem {
        title: "Cancel".to_string(),
        subtitle: Some("Discard changes.".to_string()),
        badge: Some("Cancel".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "statusline:cancel".to_string(),
        )),
        search_value: Some("statusline cancel".to_string()),
    });

    items
}

fn apply_statusline_action(
    action: &str,
    draft: &mut StatusLineConfig,
) -> Result<StatuslineSetupAction> {
    match action {
        "statusline:save" => return Ok(StatuslineSetupAction::Save),
        "statusline:cancel" => return Ok(StatuslineSetupAction::Cancel),
        "statusline:command:edit" => return Ok(StatuslineSetupAction::EditCommand),
        "statusline:command:script" => return Ok(StatuslineSetupAction::UseScriptPath),
        "statusline:command:clear" => return Ok(StatuslineSetupAction::ClearCommand),
        "statusline:refresh:edit" => return Ok(StatuslineSetupAction::EditRefreshInterval),
        "statusline:timeout:edit" => return Ok(StatuslineSetupAction::EditTimeout),
        "statusline:script:create" => {
            return Ok(StatuslineSetupAction::ScaffoldScript {
                replace_existing: false,
            });
        }
        "statusline:script:replace" => {
            return Ok(StatuslineSetupAction::ScaffoldScript {
                replace_existing: true,
            });
        }
        _ => {}
    }

    if let Some(mode) = action.strip_prefix("statusline:mode:") {
        draft.mode = match mode {
            "auto" => StatusLineMode::Auto,
            "command" => StatusLineMode::Command,
            "hidden" => StatusLineMode::Hidden,
            _ => return Err(anyhow!("unsupported status line mode action: {mode}")),
        };
        return Ok(StatuslineSetupAction::Continue);
    }

    Err(anyhow!("unsupported status line action: {action}"))
}

fn build_statusline_preview(
    draft: &StatusLineConfig,
    git: Option<(&str, bool)>,
    thread_context: Option<&str>,
    ide_context_source: Option<&str>,
    model: &str,
) -> String {
    match draft.mode {
        StatusLineMode::Hidden => "hidden mode: status line disabled".to_string(),
        StatusLineMode::Command => {
            let command = draft
                .command
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("(unset)");
            format!("command mode (setup does not execute command): {command}")
        }
        StatusLineMode::Auto => {
            let mut left_parts = Vec::new();
            if let Some((branch, dirty)) = git {
                let trimmed = branch.trim();
                if !trimmed.is_empty() {
                    left_parts.push(if dirty {
                        format!("{trimmed}*")
                    } else {
                        trimmed.to_string()
                    });
                }
            }

            let mut right_parts = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for value in [thread_context, ide_context_source, Some(model)] {
                let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
                    continue;
                };
                let key = value.to_ascii_lowercase();
                if seen.insert(key) {
                    right_parts.push(value.to_string());
                }
            }

            if left_parts.is_empty() && right_parts.is_empty() {
                return "auto mode: waiting for runtime context".to_string();
            }
            if left_parts.is_empty() {
                return format!("auto mode: {}", right_parts.join(" | "));
            }
            if right_parts.is_empty() {
                return format!("auto mode: {}", left_parts.join(" | "));
            }
            format!(
                "auto mode: {} | {}",
                left_parts.join(" | "),
                right_parts.join(" | ")
            )
        }
    }
}

fn statusline_mode_id(mode: &StatusLineMode) -> &'static str {
    match mode {
        StatusLineMode::Auto => "auto",
        StatusLineMode::Command => "command",
        StatusLineMode::Hidden => "hidden",
    }
}

async fn prompt_statusline_input(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
    allow_empty: bool,
) -> Result<Option<String>> {
    let step = build_statusline_prompt_step(question, freeform_label, placeholder, default_value);

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;

    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == STATUSLINE_INPUT_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    };
    let Some(value) = value else {
        return Ok(None);
    };

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() && !allow_empty {
        ctx.renderer
            .line(MessageStyle::Info, "Input was empty. Nothing changed.")?;
        return Ok(None);
    }
    if trimmed.is_empty() {
        return Ok(Some(String::new()));
    }
    Ok(Some(trimmed))
}

fn build_statusline_prompt_step(
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    default_value: Option<String>,
) -> WizardStep {
    WizardStep {
        title: "Input".to_string(),
        question: question.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some(
                "Press Enter to accept the default, or Tab to type a custom value.".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: STATUSLINE_INPUT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit statusline input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
        freeform_default: default_value,
    }
}

fn parse_statusline_millis(value: &str, label: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .with_context(|| format!("Failed to parse {} as milliseconds", label))
}

async fn persist_statusline_config(
    ctx: &mut SlashCommandContext<'_>,
    config_path: &Path,
    draft: StatusLineConfig,
    target: StatuslineTargetMode,
    script_path: &Path,
) -> Result<()> {
    write_statusline_config(config_path, &draft)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await?;

    if target == StatuslineTargetMode::Workspace {
        let command = draft
            .command
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if command == default_script_command(target, script_path) && !script_path.exists() {
            ctx.renderer.line(
                MessageStyle::Warning,
                "Saved command path points to a missing script. Use \"Create script template\" to scaffold it.",
            )?;
        }
    }

    Ok(())
}

fn write_statusline_config(config_path: &Path, draft: &StatusLineConfig) -> Result<()> {
    let mut root = load_toml_value(config_path)?;
    let root_table = root
        .as_table_mut()
        .context("Status line config root is not a TOML table")?;
    let ui_table = ensure_child_table(root_table, "ui");
    let status_table = ensure_child_table(ui_table, "status_line");

    status_table.insert(
        "mode".to_string(),
        TomlValue::String(statusline_mode_id(&draft.mode).to_string()),
    );
    if let Some(command) = draft
        .command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        status_table.insert(
            "command".to_string(),
            TomlValue::String(command.to_string()),
        );
    } else {
        status_table.remove("command");
    }
    status_table.insert(
        "refresh_interval_ms".to_string(),
        TomlValue::Integer(u64_to_toml_integer(
            draft.refresh_interval_ms,
            "refresh_interval_ms",
        )?),
    );
    status_table.insert(
        "command_timeout_ms".to_string(),
        TomlValue::Integer(u64_to_toml_integer(
            draft.command_timeout_ms,
            "command_timeout_ms",
        )?),
    );

    save_toml_value(config_path, &root)
}

fn u64_to_toml_integer(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{label} is too large to persist"))
}

fn statusline_script_path(
    target: StatuslineTargetMode,
    workspace: &Path,
    config_path: &Path,
) -> PathBuf {
    match target {
        StatuslineTargetMode::Workspace => {
            workspace.join(".vtcode").join(STATUSLINE_SCRIPT_FILE_NAME)
        }
        StatuslineTargetMode::User => config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| workspace.to_path_buf())
            .join(STATUSLINE_SCRIPT_FILE_NAME),
    }
}

fn default_script_command(target: StatuslineTargetMode, script_path: &Path) -> String {
    match target {
        StatuslineTargetMode::Workspace => ".vtcode/statusline.sh".to_string(),
        StatuslineTargetMode::User => shell_quote(script_path),
    }
}

fn shell_quote(path: &Path) -> String {
    let path = path.to_string_lossy();
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn preferred_user_config_path(manager: &ConfigManager) -> Option<PathBuf> {
    manager
        .layer_stack()
        .layers()
        .iter()
        .rev()
        .find_map(|layer| match &layer.source {
            ConfigLayerSource::User { file } if layer.is_enabled() => Some(file.clone()),
            _ => None,
        })
        .or_else(|| {
            let defaults = current_config_defaults();
            defaults
                .home_config_paths(manager.config_file_name())
                .into_iter()
                .next()
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(manager.config_file_name())))
}

fn scaffold_statusline_script(path: &Path, replace_existing: bool) -> Result<ScriptScaffoldResult> {
    if path.exists() && !replace_existing {
        return Ok(ScriptScaffoldResult::SkippedExisting);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let result = if path.exists() {
        ScriptScaffoldResult::Replaced
    } else {
        ScriptScaffoldResult::Created
    };
    fs::write(path, STATUSLINE_SCRIPT_TEMPLATE)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    set_executable(path)?;
    Ok(result)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("Failed to set executable bit on {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

pub(crate) async fn handle_start_terminal_title_setup(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Terminal title setup is available in inline UI only.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if !ensure_selection_ui_available(&mut ctx, "configuring the terminal title")? {
        return Ok(SlashCommandControl::Continue);
    }

    let original_items = ctx
        .vt_cfg
        .as_ref()
        .and_then(|cfg| cfg.ui.terminal_title.items.clone());
    let mut draft_items = effective_terminal_title_items(original_items.clone());

    loop {
        let preview = build_terminal_title_preview(
            &ctx.config.workspace,
            ctx.active_thread_label,
            ctx.input_status_state
                .git_summary
                .as_ref()
                .map(|summary| summary.branch.as_str()),
            &ctx.config.model,
            ctx.input_status_state.left.as_deref(),
            &draft_items,
        );
        let current_items = if draft_items.is_empty() {
            "disabled".to_string()
        } else {
            draft_items.join(", ")
        };

        ctx.handle.show_list_modal(
            "Terminal title setup".to_string(),
            vec![
                "Choose the ordered items VT Code should manage in the terminal title.".to_string(),
                format!("Current items: {current_items}"),
                format!("Preview: {preview}"),
            ],
            build_terminal_title_setup_items(&draft_items),
            Some(InlineListSelection::ConfigAction("title:save".to_string())),
            None,
        );

        let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
            ctx.handle.set_terminal_title_items(original_items.clone());
            ctx.renderer
                .line(MessageStyle::Info, "Terminal title setup cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            ctx.renderer.line(
                MessageStyle::Error,
                "Unsupported terminal title setup selection.",
            )?;
            continue;
        };

        match apply_terminal_title_action(&action, &mut draft_items)? {
            TerminalTitleSetupAction::Continue => {
                ctx.handle
                    .set_terminal_title_items(Some(draft_items.clone()));
            }
            TerminalTitleSetupAction::Save => {
                persist_terminal_title_items(
                    &ctx.config.workspace,
                    ctx.vt_cfg,
                    draft_items.clone(),
                )?;
                ctx.handle
                    .set_terminal_title_items(Some(draft_items.clone()));
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Saved terminal title configuration to vtcode.toml.",
                )?;
                return Ok(SlashCommandControl::Continue);
            }
            TerminalTitleSetupAction::Cancel => {
                ctx.handle.set_terminal_title_items(original_items.clone());
                ctx.renderer
                    .line(MessageStyle::Info, "Terminal title setup cancelled.")?;
                return Ok(SlashCommandControl::Continue);
            }
        }
    }
}

pub(crate) async fn handle_start_model_selection(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "selecting a model target")? {
        return Ok(SlashCommandControl::Continue);
    }

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Info,
            "Inline UI is unavailable; opening the main model picker directly.",
        )?;
        return start_model_selection_target(ctx, ModelPickerTarget::Main).await;
    }

    if show_model_target_palette(ctx.renderer)? {
        *ctx.palette_state = Some(ActivePalette::ModelTarget);
    }
    Ok(SlashCommandControl::Continue)
}

pub(super) async fn start_model_selection_target(
    ctx: SlashCommandContext<'_>,
    target: ModelPickerTarget,
) -> Result<SlashCommandControl> {
    ctx.session_stats.model_picker_target = target;
    match target {
        ModelPickerTarget::Main => start_model_picker(ctx).await,
        ModelPickerTarget::Lightweight => {
            let vt_cfg = ctx.vt_cfg.clone();
            let restore_status_left = ctx.input_status_state.left.clone();
            let restore_status_right = ctx.input_status_state.right.clone();
            let view = {
                let loading_spinner = if ctx.renderer.supports_inline_ui() {
                    Some(PlaceholderSpinner::new(
                        ctx.handle,
                        restore_status_left,
                        restore_status_right,
                        "Loading lightweight model lists...",
                    ))
                } else {
                    ctx.renderer
                        .line(MessageStyle::Info, "Loading lightweight model lists...")?;
                    None
                };
                let result = build_lightweight_palette_view(ctx.config, vt_cfg.as_ref()).await;
                drop(loading_spinner);
                result
            };
            if show_lightweight_model_palette(ctx.renderer, &view, None)? {
                *ctx.palette_state = Some(ActivePalette::LightweightModel {
                    view: Box::new(view),
                });
            }
            ctx.session_stats.model_picker_target = ModelPickerTarget::Main;
            Ok(SlashCommandControl::Continue)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalTitleSetupAction {
    Continue,
    Save,
    Cancel,
}

fn build_terminal_title_setup_items(draft_items: &[String]) -> Vec<InlineListItem> {
    let mut items = Vec::new();

    for (index, item_id) in draft_items.iter().enumerate() {
        let Some(spec) = terminal_title_item_spec(item_id) else {
            continue;
        };
        items.push(InlineListItem {
            title: format!("Remove {}", spec.title),
            subtitle: Some(format!("#{} in title. {}", index + 1, spec.description)),
            badge: Some("Enabled".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "title:remove:{}",
                spec.id
            ))),
            search_value: Some(format!("terminal title remove {}", spec.id)),
        });
        if index > 0 {
            items.push(InlineListItem {
                title: format!("Move {} up", spec.title),
                subtitle: Some("Move earlier in the title".to_string()),
                badge: Some("Reorder".to_string()),
                indent: 1,
                selection: Some(InlineListSelection::ConfigAction(format!(
                    "title:move_up:{}",
                    spec.id
                ))),
                search_value: Some(format!("terminal title move up {}", spec.id)),
            });
        }
        if index + 1 < draft_items.len() {
            items.push(InlineListItem {
                title: format!("Move {} down", spec.title),
                subtitle: Some("Move later in the title".to_string()),
                badge: Some("Reorder".to_string()),
                indent: 1,
                selection: Some(InlineListSelection::ConfigAction(format!(
                    "title:move_down:{}",
                    spec.id
                ))),
                search_value: Some(format!("terminal title move down {}", spec.id)),
            });
        }
    }

    for spec in TERMINAL_TITLE_ITEM_SPECS {
        if draft_items.iter().any(|item| item == spec.id) {
            continue;
        }
        items.push(InlineListItem {
            title: format!("Add {}", spec.title),
            subtitle: Some(spec.description.to_string()),
            badge: Some("Available".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "title:add:{}",
                spec.id
            ))),
            search_value: Some(format!("terminal title add {}", spec.id)),
        });
    }

    items.push(InlineListItem {
        title: "Save changes".to_string(),
        subtitle: Some("Persist ui.terminal_title.items in vtcode.toml".to_string()),
        badge: Some("Save".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction("title:save".to_string())),
        search_value: Some("terminal title save".to_string()),
    });
    items.push(InlineListItem {
        title: "Cancel".to_string(),
        subtitle: Some("Discard changes and restore the original title".to_string()),
        badge: Some("Cancel".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            "title:cancel".to_string(),
        )),
        search_value: Some("terminal title cancel".to_string()),
    });

    items
}

fn apply_terminal_title_action(
    action: &str,
    draft_items: &mut Vec<String>,
) -> Result<TerminalTitleSetupAction> {
    match action {
        "title:save" => return Ok(TerminalTitleSetupAction::Save),
        "title:cancel" => return Ok(TerminalTitleSetupAction::Cancel),
        _ => {}
    }

    let mut parts = action.splitn(3, ':');
    let Some("title") = parts.next() else {
        return Err(anyhow!("unsupported terminal title action: {action}"));
    };
    let operation = parts
        .next()
        .ok_or_else(|| anyhow!("missing terminal title operation"))?;
    let item_id = parts
        .next()
        .ok_or_else(|| anyhow!("missing terminal title item id"))?;

    if terminal_title_item_spec(item_id).is_none() {
        return Err(anyhow!("unsupported terminal title item: {item_id}"));
    }

    match operation {
        "add" => {
            if !draft_items.iter().any(|item| item == item_id) {
                draft_items.push(item_id.to_string());
            }
        }
        "remove" => {
            draft_items.retain(|item| item != item_id);
        }
        "move_up" => {
            if let Some(index) = draft_items.iter().position(|item| item == item_id)
                && index > 0
            {
                draft_items.swap(index - 1, index);
            }
        }
        "move_down" => {
            if let Some(index) = draft_items.iter().position(|item| item == item_id)
                && index + 1 < draft_items.len()
            {
                draft_items.swap(index, index + 1);
            }
        }
        _ => return Err(anyhow!("unsupported terminal title operation: {operation}")),
    }

    Ok(TerminalTitleSetupAction::Continue)
}

fn persist_terminal_title_items(
    workspace: &Path,
    vt_cfg: &mut Option<vtcode_core::config::loader::VTCodeConfig>,
    draft_items: Vec<String>,
) -> Result<()> {
    let mut manager = ConfigManager::load_from_workspace(workspace)
        .context("Failed to load configuration for terminal title update")?;
    let mut config = manager.config().clone();
    config.ui.terminal_title.items = Some(draft_items.clone());
    manager
        .save_config(&config)
        .context("Failed to save terminal title configuration")?;

    match vt_cfg {
        Some(existing) => existing.ui.terminal_title.items = Some(draft_items),
        None => *vt_cfg = Some(config),
    }
    Ok(())
}

fn terminal_title_item_spec(item_id: &str) -> Option<TerminalTitleItemSpec> {
    TERMINAL_TITLE_ITEM_SPECS
        .iter()
        .copied()
        .find(|spec| spec.id == item_id)
}

fn effective_terminal_title_items(raw_items: Option<Vec<String>>) -> Vec<String> {
    match raw_items {
        Some(items) => items
            .into_iter()
            .filter(|item| terminal_title_item_spec(item).is_some())
            .collect(),
        None => DEFAULT_TERMINAL_TITLE_ITEMS
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
    }
}

fn build_terminal_title_preview(
    workspace: &Path,
    thread_label: &str,
    git_branch: Option<&str>,
    model: &str,
    status_left: Option<&str>,
    draft_items: &[String],
) -> String {
    if draft_items.is_empty() {
        return "terminal title updates disabled".to_string();
    }

    let status = preview_status_label(status_left);
    let spinner = match status {
        "Ready" => None,
        "Action Required" => Some("!".to_string()),
        _ => Some("...".to_string()),
    };
    let project = workspace
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "workspace".to_string());

    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item_id in draft_items {
        let candidate = match item_id.as_str() {
            "app-name" => Some(("VT Code".to_string(), false)),
            "project" => Some((project.clone(), false)),
            "spinner" => spinner.as_ref().map(|spinner| (spinner.clone(), true)),
            "status" => Some((status.to_string(), false)),
            "thread" => {
                if !thread_label.trim().is_empty() {
                    Some((thread_label.trim().to_string(), false))
                } else {
                    None
                }
            }
            "git-branch" => git_branch
                .filter(|branch| !branch.trim().is_empty())
                .map(|branch| (branch.trim().to_string(), false)),
            "model" => {
                if !model.trim().is_empty() {
                    Some((model.trim().to_string(), false))
                } else {
                    None
                }
            }
            "task-progress" => Some(("2/5".to_string(), false)),
            _ => None,
        };
        if let Some((text, spinner_part)) = candidate {
            let key = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            if !key.is_empty() && seen.insert(key) {
                parts.push((text, spinner_part));
            }
        }
    }

    let mut preview = String::new();
    for (index, (text, spinner_part)) in parts.iter().enumerate() {
        if index > 0 {
            let previous_spinner = parts[index - 1].1;
            preview.push_str(if previous_spinner || *spinner_part {
                " "
            } else {
                " | "
            });
        }
        preview.push_str(text);
    }
    preview
}

fn preview_status_label(status_left: Option<&str>) -> &'static str {
    let normalized = status_left.unwrap_or("").trim().to_ascii_lowercase();
    if normalized.contains("action required") || normalized.contains("approval") {
        "Action Required"
    } else if normalized.contains("undo")
        || normalized.contains("rewind")
        || normalized.contains("revert")
    {
        "Undoing"
    } else if normalized.contains("waiting") || normalized.contains("queued") {
        "Waiting"
    } else if normalized.contains("thinking") || normalized.contains("processing") {
        "Thinking"
    } else if normalized.contains("running") {
        "Working"
    } else {
        "Ready"
    }
}

pub(crate) async fn handle_toggle_ide_context(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let enabled = ctx.context_manager.toggle_session_ide_context();

    let latest_editor_snapshot = if let Some(bridge) = ctx.ide_context_bridge.as_mut() {
        match bridge.refresh() {
            Ok((snapshot, _)) => snapshot,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to refresh IDE context while toggling /ide");
                bridge.snapshot().cloned()
            }
        }
    } else {
        None
    };

    apply_ide_context_snapshot(
        ctx.context_manager,
        ctx.header_context,
        ctx.handle,
        ctx.config.workspace.as_path(),
        ctx.vt_cfg.as_ref(),
        latest_editor_snapshot.clone(),
    );

    crate::agent::runloop::unified::status_line::update_ide_context_source(
        ctx.input_status_state,
        ide_context_status_label_from_bridge(
            ctx.context_manager,
            ctx.config.workspace.as_path(),
            ctx.vt_cfg.as_ref(),
            ctx.ide_context_bridge.as_ref(),
        ),
    );

    let message = match (enabled, latest_editor_snapshot.is_some()) {
        (true, true) => "IDE context enabled for this session.",
        (true, false) => {
            "IDE context enabled for this session. No IDE snapshot is currently available."
        }
        (false, _) => "IDE context disabled for this session.",
    };
    ctx.renderer.line(MessageStyle::Info, message)?;

    Ok(SlashCommandControl::Continue)
}

pub(super) async fn start_model_picker(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if ctx.model_picker_state.is_some() {
        ctx.renderer.line(
            MessageStyle::Error,
            "A model picker session is already active. Complete or type 'cancel' to exit it before starting another.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    let reasoning = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort)
        .unwrap_or(ctx.config.reasoning_effort);
    let service_tier = ctx
        .vt_cfg
        .as_ref()
        .and_then(|cfg| cfg.provider.openai.service_tier);
    let workspace_hint = Some(ctx.config.workspace.clone());
    let restore_status_left = ctx.input_status_state.left.clone();
    let restore_status_right = ctx.input_status_state.right.clone();
    let picker_start = {
        let loading_spinner = if ctx.renderer.supports_inline_ui() {
            Some(PlaceholderSpinner::new(
                ctx.handle,
                restore_status_left.clone(),
                restore_status_right.clone(),
                "Loading model lists...",
            ))
        } else {
            ctx.renderer
                .line(MessageStyle::Info, "Loading model lists...")?;
            None
        };
        let result = ModelPickerState::new(
            ctx.renderer,
            ctx.vt_cfg.clone(),
            reasoning,
            service_tier,
            workspace_hint,
            ctx.config.provider.clone(),
            ctx.config.model.clone(),
            Some(std::sync::Arc::clone(ctx.ctrl_c_state)),
            Some(std::sync::Arc::clone(ctx.ctrl_c_notify)),
        )
        .await;
        drop(loading_spinner);
        result
    };
    match picker_start {
        Ok(ModelPickerStart::InProgress(picker)) => {
            *ctx.model_picker_state = Some(picker);
        }
        Ok(ModelPickerStart::Completed { state, selection }) => {
            if let Err(err) = finalize_model_selection(
                ctx.renderer,
                &state,
                selection,
                ctx.config,
                ctx.vt_cfg,
                ctx.provider_client,
                ctx.session_bootstrap,
                ctx.handle,
                ctx.header_context,
                ctx.full_auto,
                ctx.conversation_history.len(),
            )
            .await
            {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to apply model selection: {}", err),
                )?;
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to start model picker: {}", err),
            )?;
        }
    }
    Ok(SlashCommandControl::Continue)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use vtcode_core::config::{StatusLineConfig, StatusLineMode};

    use super::{
        apply_statusline_action, apply_terminal_title_action, build_statusline_preview,
        build_statusline_setup_items, build_terminal_title_preview,
        build_terminal_title_setup_items, default_script_command, effective_terminal_title_items,
    };

    #[test]
    fn statusline_action_switches_mode() {
        let mut draft = StatusLineConfig::default();

        let action =
            apply_statusline_action("statusline:mode:hidden", &mut draft).expect("mode action");

        assert_eq!(action, super::StatuslineSetupAction::Continue);
        assert_eq!(draft.mode, StatusLineMode::Hidden);
    }

    #[test]
    fn statusline_preview_in_command_mode_never_executes() {
        let draft = StatusLineConfig {
            mode: StatusLineMode::Command,
            command: Some(".vtcode/statusline.sh".to_string()),
            ..StatusLineConfig::default()
        };

        let preview = build_statusline_preview(
            &draft,
            Some(("main", false)),
            Some("thread-1"),
            None,
            "gpt-5.4",
        );

        assert_eq!(
            preview,
            "command mode (setup does not execute command): .vtcode/statusline.sh"
        );
    }

    #[test]
    fn statusline_items_offer_replace_when_script_exists() {
        let draft = StatusLineConfig::default();
        let items = build_statusline_setup_items(&draft, true);

        assert!(
            items
                .iter()
                .any(|item| item.title == "Replace script template")
        );
        assert!(
            !items
                .iter()
                .any(|item| item.title == "Create script template")
        );
    }

    #[test]
    fn user_script_command_is_shell_quoted() {
        let command = default_script_command(
            super::StatuslineTargetMode::User,
            Path::new("/tmp/status line's/script.sh"),
        );

        assert_eq!(command, "'/tmp/status line'\\''s/script.sh'");
    }

    #[test]
    fn effective_items_default_to_spinner_and_project() {
        assert_eq!(
            effective_terminal_title_items(None),
            vec!["spinner".to_string(), "project".to_string()]
        );
    }

    #[test]
    fn setup_items_preserve_current_order() {
        let items =
            build_terminal_title_setup_items(&["spinner".to_string(), "project".to_string()]);

        assert_eq!(items[0].title, "Remove Spinner");
        assert_eq!(items[1].title, "Move Spinner down");
        assert_eq!(items[2].title, "Remove Project");
        assert_eq!(items[3].title, "Move Project up");
    }

    #[test]
    fn preview_text_uses_spinner_separator_rules() {
        let preview = build_terminal_title_preview(
            Path::new("/tmp/demo-project"),
            "main",
            Some("feature/title"),
            "gpt-5.4",
            Some("Thinking"),
            &[
                "project".to_string(),
                "spinner".to_string(),
                "status".to_string(),
            ],
        );

        assert_eq!(preview, "demo-project ... Thinking");
    }

    #[test]
    fn apply_actions_support_reorder_and_disable() {
        let mut items = vec!["spinner".to_string(), "project".to_string()];

        apply_terminal_title_action("title:move_down:spinner", &mut items).expect("move down");
        assert_eq!(items, vec!["project".to_string(), "spinner".to_string()]);

        apply_terminal_title_action("title:remove:project", &mut items).expect("remove");
        assert_eq!(items, vec!["spinner".to_string()]);

        apply_terminal_title_action("title:remove:spinner", &mut items).expect("disable");
        assert!(items.is_empty());
    }

    #[test]
    fn cancel_action_leaves_draft_unchanged_for_restore() {
        let mut items = vec!["spinner".to_string(), "project".to_string()];
        let original = items.clone();

        let action =
            apply_terminal_title_action("title:cancel", &mut items).expect("cancel should parse");

        assert_eq!(action, super::TerminalTitleSetupAction::Cancel);
        assert_eq!(items, original);
    }

    #[test]
    fn save_action_is_supported() {
        let mut items = vec!["spinner".to_string(), "project".to_string()];

        let action =
            apply_terminal_title_action("title:save", &mut items).expect("save should parse");

        assert_eq!(action, super::TerminalTitleSetupAction::Save);
        assert_eq!(items, vec!["spinner".to_string(), "project".to_string()]);
    }

    #[test]
    fn preview_deduplicates_thread_and_git_branch() {
        let preview = build_terminal_title_preview(
            Path::new("/tmp/demo-project"),
            "main",
            Some("main"),
            "gpt-5.4",
            Some("Ready"),
            &["thread".to_string(), "git-branch".to_string()],
        );

        assert_eq!(preview, "main");
    }
}
