use anyhow::{Result, anyhow};
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_ui::tui::app::InlineListItem;

use super::super::config_action_item;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatuslineSetupAction {
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

pub(super) fn build_statusline_setup_items(draft: &StatusLineConfig, script_exists: bool) -> Vec<InlineListItem> {
    let mut items = Vec::new();

    for (mode, label, subtitle) in [
        (StatusLineMode::Auto, "Use auto permission review", "Show VT Code-built status components."),
        (StatusLineMode::Command, "Use command mode", "Run a shell command and render its first output line."),
        (StatusLineMode::Hidden, "Hide status line", "Disable the bottom status line."),
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
        if draft.show_clock {
            "Clock: shown"
        } else {
            "Clock: hidden"
        },
        "Show the current system time (HH:MM:SS) on the status line.",
        "Clock",
        0,
        "statusline:clock:toggle",
        "statusline clock time show hide toggle",
    ));
    items.push(config_action_item(
        "Use scaffold script path",
        "Point command to the target statusline.sh script.",
        "Command",
        0,
        "statusline:command:script",
        "statusline command script",
    ));
    if draft.command.as_deref().map(str::trim).is_some_and(|value| !value.is_empty()) {
        items.push(config_action_item(
            "Clear command",
            "Remove command so command mode falls back to auto.",
            "Command",
            0,
            "statusline:command:clear",
            "statusline command clear",
        ));
    }

    let (script_title, script_action, script_search) = if script_exists {
        ("Replace script template", "statusline:script:replace", "statusline script replace")
    } else {
        ("Create script template", "statusline:script:create", "statusline script create")
    };
    items.push(config_action_item(
        script_title,
        if script_exists {
            "Overwrite existing statusline.sh with the default template."
        } else {
            "Create statusline.sh using the default JSON payload template."
        },
        "Script",
        0,
        script_action,
        script_search,
    ));

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

pub(super) fn apply_statusline_action(action: &str, draft: &mut StatusLineConfig) -> Result<StatuslineSetupAction> {
    match action {
        "statusline:save" => return Ok(StatuslineSetupAction::Save),
        "statusline:cancel" => return Ok(StatuslineSetupAction::Cancel),
        "statusline:command:edit" => return Ok(StatuslineSetupAction::EditCommand),
        "statusline:command:script" => return Ok(StatuslineSetupAction::UseScriptPath),
        "statusline:command:clear" => return Ok(StatuslineSetupAction::ClearCommand),
        "statusline:refresh:edit" => return Ok(StatuslineSetupAction::EditRefreshInterval),
        "statusline:timeout:edit" => return Ok(StatuslineSetupAction::EditTimeout),
        "statusline:clock:toggle" => {
            draft.show_clock.toggle();
            return Ok(StatuslineSetupAction::Continue);
        }
        "statusline:script:create" => {
            return Ok(StatuslineSetupAction::ScaffoldScript { replace_existing: false });
        }
        "statusline:script:replace" => {
            return Ok(StatuslineSetupAction::ScaffoldScript { replace_existing: true });
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

pub(super) fn statusline_mode_id(mode: &StatusLineMode) -> &'static str {
    match mode {
        StatusLineMode::Auto => "auto",
        StatusLineMode::Command => "command",
        StatusLineMode::Hidden => "hidden",
        StatusLineMode::Unknown => "unknown",
    }
}
