use std::path::Path;

use anyhow::Result;
use anyhow::{Context, anyhow};
use vtcode_core::config::DEFAULT_TERMINAL_TITLE_ITEMS;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::turn::session::slash_commands::{SlashCommandContext, SlashCommandControl};

use super::{config_action_item, ensure_selection_ui_available, wait_for_list_modal_selection};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalTitleSetupAction {
    Continue,
    Save,
    Cancel,
}

pub(crate) async fn handle_start_terminal_title_setup(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer
            .line(MessageStyle::Info, "Terminal title setup is available in inline UI only.")?;
        return Ok(SlashCommandControl::Continue);
    }
    if !ensure_selection_ui_available(&mut ctx, "configuring the terminal title")? {
        return Ok(SlashCommandControl::Continue);
    }

    let original_items = ctx.vt_cfg.as_ref().and_then(|cfg| cfg.ui.terminal_title.items.clone());
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
            ctx.renderer.line(MessageStyle::Info, "Terminal title setup cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            ctx.renderer
                .line(MessageStyle::Error, "Unsupported terminal title setup selection.")?;
            continue;
        };

        match apply_terminal_title_action(&action, &mut draft_items)? {
            TerminalTitleSetupAction::Continue => {
                ctx.handle.set_terminal_title_items(Some(draft_items.clone()));
            }
            TerminalTitleSetupAction::Save => {
                persist_terminal_title_items(&ctx.config.workspace, ctx.vt_cfg, draft_items.clone())?;
                ctx.handle.set_terminal_title_items(Some(draft_items.clone()));
                ctx.renderer
                    .line(MessageStyle::Info, "Saved terminal title configuration to vtcode.toml.")?;
                return Ok(SlashCommandControl::Continue);
            }
            TerminalTitleSetupAction::Cancel => {
                ctx.handle.set_terminal_title_items(original_items.clone());
                ctx.renderer.line(MessageStyle::Info, "Terminal title setup cancelled.")?;
                return Ok(SlashCommandControl::Continue);
            }
        }
    }
}

fn build_terminal_title_setup_items(draft_items: &[String]) -> Vec<InlineListItem> {
    let mut items = Vec::new();

    for (index, item_id) in draft_items.iter().enumerate() {
        let Some(spec) = terminal_title_item_spec(item_id) else {
            continue;
        };
        items.push(config_action_item(
            &format!("Remove {}", spec.title),
            &format!("#{} in title. {}", index + 1, spec.description),
            "Enabled",
            0,
            format!("title:remove:{}", spec.id),
            format!("terminal title remove {}", spec.id),
        ));
        if index > 0 {
            items.push(config_action_item(
                &format!("Move {} up", spec.title),
                "Move earlier in the title",
                "Reorder",
                1,
                format!("title:move_up:{}", spec.id),
                format!("terminal title move up {}", spec.id),
            ));
        }
        if index + 1 < draft_items.len() {
            items.push(config_action_item(
                &format!("Move {} down", spec.title),
                "Move later in the title",
                "Reorder",
                1,
                format!("title:move_down:{}", spec.id),
                format!("terminal title move down {}", spec.id),
            ));
        }
    }

    for spec in TERMINAL_TITLE_ITEM_SPECS {
        if draft_items.iter().any(|item| item == spec.id) {
            continue;
        }
        items.push(config_action_item(
            &format!("Add {}", spec.title),
            spec.description,
            "Available",
            0,
            format!("title:add:{}", spec.id),
            format!("terminal title add {}", spec.id),
        ));
    }

    items.push(config_action_item(
        "Save changes",
        "Persist ui.terminal_title.items in vtcode.toml",
        "Save",
        0,
        "title:save",
        "terminal title save",
    ));
    items.push(config_action_item(
        "Cancel",
        "Discard changes and restore the original title",
        "Cancel",
        0,
        "title:cancel",
        "terminal title cancel",
    ));

    items
}

fn apply_terminal_title_action(action: &str, draft_items: &mut Vec<String>) -> Result<TerminalTitleSetupAction> {
    match action {
        "title:save" => return Ok(TerminalTitleSetupAction::Save),
        "title:cancel" => return Ok(TerminalTitleSetupAction::Cancel),
        _ => {}
    }

    let mut parts = action.splitn(3, ':');
    let Some("title") = parts.next() else {
        return Err(anyhow!("unsupported terminal title action: {action}"));
    };
    let operation = parts.next().ok_or_else(|| anyhow!("missing terminal title operation"))?;
    let item_id = parts.next().ok_or_else(|| anyhow!("missing terminal title item id"))?;

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
    vt_cfg: &mut Option<VTCodeConfig>,
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
    TERMINAL_TITLE_ITEM_SPECS.iter().copied().find(|spec| spec.id == item_id)
}

fn effective_terminal_title_items(raw_items: Option<Vec<String>>) -> Vec<String> {
    match raw_items {
        Some(items) => items
            .into_iter()
            .filter(|item| terminal_title_item_spec(item).is_some())
            .collect(),
        None => DEFAULT_TERMINAL_TITLE_ITEMS.iter().map(|item| (*item).to_string()).collect(),
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
            let key = text.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
            if !key.is_empty() && seen.insert(key) {
                parts.push((text, spinner_part));
            }
        }
    }

    let mut preview = String::new();
    for (index, (text, spinner_part)) in parts.iter().enumerate() {
        if index > 0 {
            let previous_spinner = parts[index - 1].1;
            preview.push_str(if previous_spinner || *spinner_part { " " } else { " | " });
        }
        preview.push_str(text);
    }
    preview
}

fn preview_status_label(status_left: Option<&str>) -> &'static str {
    let normalized = status_left.unwrap_or("").trim().to_ascii_lowercase();
    if normalized.contains("action required") || normalized.contains("approval") {
        "Action Required"
    } else if normalized.contains("undo") || normalized.contains("rewind") || normalized.contains("revert") {
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        TerminalTitleSetupAction, apply_terminal_title_action, build_terminal_title_preview,
        build_terminal_title_setup_items, effective_terminal_title_items,
    };

    #[test]
    fn effective_items_default_to_spinner_and_project() {
        assert_eq!(effective_terminal_title_items(None), vec!["spinner".to_string(), "project".to_string()]);
    }

    #[test]
    fn setup_items_preserve_current_order() {
        let items = build_terminal_title_setup_items(&["spinner".to_string(), "project".to_string()]);

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
            &["project".to_string(), "spinner".to_string(), "status".to_string()],
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

        let action = apply_terminal_title_action("title:cancel", &mut items).expect("cancel should parse");

        assert_eq!(action, TerminalTitleSetupAction::Cancel);
        assert_eq!(items, original);
    }

    #[test]
    fn save_action_is_supported() {
        let mut items = vec!["spinner".to_string(), "project".to_string()];

        let action = apply_terminal_title_action("title:save", &mut items).expect("save should parse");

        assert_eq!(action, TerminalTitleSetupAction::Save);
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
