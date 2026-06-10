use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use anyhow::{Context, anyhow};
use toml::Value as TomlValue;
use vtcode_core::config::current_config_defaults;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::loader::layers::ConfigLayerSource;
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::slash_commands::StatuslineTargetMode;
use crate::agent::runloop::unified::palettes::refresh_runtime_config_from_manager;
use crate::agent::runloop::unified::turn::session::slash_commands::{
    SlashCommandContext, SlashCommandControl,
};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::super::config_toml::{
    ensure_child_table, load_toml_value, preferred_workspace_config_path, save_toml_value,
};
use super::{config_action_item, ensure_selection_ui_available, wait_for_list_modal_selection};

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
            "Use auto permission review",
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
        items.push(config_action_item(
            label,
            subtitle,
            if active { "Active" } else { "Mode" },
            0,
            format!("statusline:mode:{}", statusline_mode_id(&mode)),
            format!("statusline mode {}", statusline_mode_id(&mode)),
        ));
    }

    items.push(config_action_item(
        "Edit command",
        "Set the shell command for command mode.",
        "Command",
        0,
        "statusline:command:edit",
        "statusline command edit",
    ));
    items.push(config_action_item(
        "Use scaffold script path",
        "Point command to the target statusline.sh script.",
        "Command",
        0,
        "statusline:command:script",
        "statusline command script",
    ));
    if draft
        .command
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
    {
        items.push(config_action_item(
            "Clear command",
            "Remove command so command mode falls back to auto.",
            "Command",
            0,
            "statusline:command:clear",
            "statusline command clear",
        ));
    }

    if script_exists {
        items.push(config_action_item(
            "Replace script template",
            "Overwrite existing statusline.sh with the default template.",
            "Script",
            0,
            "statusline:script:replace",
            "statusline script replace",
        ));
    } else {
        items.push(config_action_item(
            "Create script template",
            "Create statusline.sh using the default JSON payload template.",
            "Script",
            0,
            "statusline:script:create",
            "statusline script create",
        ));
    }

    items.push(config_action_item(
        &format!("Refresh interval: {}ms", draft.refresh_interval_ms),
        "Set command refresh cadence.",
        "Timing",
        0,
        "statusline:refresh:edit",
        "statusline refresh interval",
    ));
    items.push(config_action_item(
        &format!("Command timeout: {}ms", draft.command_timeout_ms),
        "Set command execution timeout.",
        "Timing",
        0,
        "statusline:timeout:edit",
        "statusline timeout",
    ));
    items.push(config_action_item(
        "Save changes",
        "Persist [ui.status_line] changes.",
        "Save",
        0,
        "statusline:save",
        "statusline save",
    ));
    items.push(config_action_item(
        "Cancel",
        "Discard changes.",
        "Cancel",
        0,
        "statusline:cancel",
        "statusline cancel",
    ));

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
                return "auto permission review: waiting for runtime context".to_string();
            }
            if left_parts.is_empty() {
                return format!("auto permission review: {}", right_parts.join(" | "));
            }
            if right_parts.is_empty() {
                return format!("auto permission review: {}", left_parts.join(" | "));
            }
            format!(
                "auto permission review: {} | {}",
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use vtcode_core::config::{StatusLineConfig, StatusLineMode};

    use super::{
        StatuslineSetupAction, apply_statusline_action, build_statusline_preview,
        build_statusline_setup_items, default_script_command,
    };
    use crate::agent::runloop::slash_commands::StatuslineTargetMode;

    #[test]
    fn statusline_action_switches_mode() {
        let mut draft = StatusLineConfig::default();

        let action =
            apply_statusline_action("statusline:mode:hidden", &mut draft).expect("mode action");

        assert_eq!(action, StatuslineSetupAction::Continue);
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
            StatuslineTargetMode::User,
            Path::new("/tmp/status line's/script.sh"),
        );

        assert_eq!(command, "'/tmp/status line'\\''s/script.sh'");
    }
}
