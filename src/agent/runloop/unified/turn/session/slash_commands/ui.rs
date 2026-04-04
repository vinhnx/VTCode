use anyhow::Result;
use anyhow::{Context, anyhow};
use std::path::Path;
use tokio::task;
use vtcode_core::config::DEFAULT_TERMINAL_TITLE_ITEMS;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::ui::inline_theme_from_core_styles;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection, TransientSubmission};

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
    show_lightweight_model_palette, show_model_target_palette, show_sessions_palette,
    show_theme_palette,
};
use crate::agent::runloop::unified::session_setup::{
    apply_ide_context_snapshot, ide_context_status_label_from_bridge,
};
use crate::agent::runloop::unified::state::ModelPickerTarget;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

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
    instructions: Option<String>,
) -> Result<SlashCommandControl> {
    if !ensure_selection_ui_available(&mut ctx, "configuring the status line")? {
        return Ok(SlashCommandControl::Continue);
    }

    let lines = vec![
        "Choose where VT Code should persist the status-line setup.".to_string(),
        "User writes to your home config and ~/.config/vtcode/statusline.sh.".to_string(),
        "Workspace writes to the current workspace and .vtcode/statusline.sh.".to_string(),
    ];
    let items = vec![
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
    ];

    ctx.handle.show_list_modal(
        "Status line setup".to_string(),
        lines,
        items,
        Some(InlineListSelection::ConfigAction(
            "statusline:user".to_string(),
        )),
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        ctx.renderer
            .line(MessageStyle::Info, "Status line setup cancelled.")?;
        return Ok(SlashCommandControl::Continue);
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
            return Ok(SlashCommandControl::Continue);
        }
    };

    let (target_label, config_target, script_path) = match target {
        StatuslineTargetMode::User => (
            "user",
            "the user-level VT Code config layer",
            "~/.config/vtcode/statusline.sh",
        ),
        StatuslineTargetMode::Workspace => (
            "workspace",
            "the current workspace VT Code config layer",
            ".vtcode/statusline.sh",
        ),
    };
    let extra = instructions
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!(" Additional requirements: {}.", value.trim()))
        .unwrap_or_default();
    let prompt = format!(
        "Set up a VT Code custom status line for the {target_label} target. Create or update the status-line script at `{script_path}` and configure `[ui.status_line]` in {config_target} so `mode = \"command\"` and `command` points to that script. Reuse VT Code config APIs and existing status-line payload behavior. Keep the script concise and shell-compatible.{extra}"
    );

    Ok(SlashCommandControl::SubmitPrompt(prompt))
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
    for item_id in draft_items {
        match item_id.as_str() {
            "app-name" => parts.push(("VT Code".to_string(), false)),
            "project" => parts.push((project.clone(), false)),
            "spinner" => {
                if let Some(spinner) = &spinner {
                    parts.push((spinner.clone(), true));
                }
            }
            "status" => parts.push((status.to_string(), false)),
            "thread" => {
                if !thread_label.trim().is_empty() {
                    parts.push((thread_label.trim().to_string(), false));
                }
            }
            "git-branch" => {
                if let Some(branch) = git_branch.filter(|branch| !branch.trim().is_empty()) {
                    parts.push((branch.trim().to_string(), false));
                }
            }
            "model" => {
                if !model.trim().is_empty() {
                    parts.push((model.trim().to_string(), false));
                }
            }
            "task-progress" => parts.push(("2/5".to_string(), false)),
            _ => {}
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

    use super::{
        apply_terminal_title_action, build_terminal_title_preview,
        build_terminal_title_setup_items, effective_terminal_title_items,
    };

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
}
