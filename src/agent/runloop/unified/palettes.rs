use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use chrono::Local;

use vtcode_config::root::{ColorSchemeMode, NotificationDeliveryMode};
use vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode;
use vtcode_core::config::ReasoningDisplayMode;
use vtcode_core::config::SystemPromptMode;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::ToolPolicy;
use vtcode_core::config::UiDisplayMode;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::{ReasoningEffortLevel, VerbosityLevel};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionListing;
use vtcode_tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, convert_style,
};

use crate::agent::runloop::slash_commands::ThemePaletteMode;
use crate::agent::runloop::tui_compat::{inline_theme_from_core_styles, to_tui_appearance};
use crate::agent::runloop::ui::build_inline_header_context;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::display::persist_theme_preference;

const THEME_PALETTE_TITLE: &str = "Theme picker";
const THEME_ACTIVE_BADGE: &str = "Active";
const THEME_SELECT_HINT: &str = "Use ↑/↓ to choose a theme, Enter to apply, Esc to cancel.";
const THEME_SEARCH_LABEL: &str = "Search themes";
const THEME_SEARCH_PLACEHOLDER: &str = "Filter themes (fuzzy)";
const SESSIONS_PALETTE_TITLE: &str = "Archived sessions";
const SESSIONS_HINT_PRIMARY: &str = "Use ↑/↓ to browse sessions.";
const SESSIONS_HINT_SECONDARY: &str = "Enter to resume session • Esc to close.";
const SESSIONS_LATEST_BADGE: &str = "Latest";
const CONFIG_PALETTE_TITLE: &str = "VT Code Configuration";
const CONFIG_HINT: &str = "↑/↓ select • ←/→ change <value> • Enter apply • Esc close";

#[derive(Clone)]
pub(crate) enum ActivePalette {
    Theme {
        mode: ThemePaletteMode,
        original_theme_id: String,
    },
    Sessions {
        listings: Vec<SessionListing>,
        limit: usize,
    },
    Config {
        workspace: PathBuf,
        vt_snapshot: Box<Option<VTCodeConfig>>,
        selected: Option<InlineListSelection>,
    },
}

pub(crate) fn show_theme_palette(
    renderer: &mut AnsiRenderer,
    mode: ThemePaletteMode,
) -> Result<bool> {
    let title = match mode {
        ThemePaletteMode::Select => THEME_PALETTE_TITLE,
    };
    let hint = match mode {
        ThemePaletteMode::Select => THEME_SELECT_HINT,
    };

    let current_id = theme::active_theme_id();
    let current_label = theme::active_theme_label().to_string();
    let mut items = Vec::new();

    for id in theme::available_themes() {
        let label = theme::theme_label(id).unwrap_or(id);
        let badge = (id == current_id).then(|| THEME_ACTIVE_BADGE.to_string());
        items.push(InlineListItem {
            title: label.to_string(),
            subtitle: Some(format!("ID: {}", id)),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Theme(id.to_string())),
            search_value: Some(theme_search_value(id, label)),
        });
    }

    if items.is_empty() {
        renderer.line(MessageStyle::Info, "No themes available.")?;
        return Ok(false);
    }

    let lines = vec![
        format!("Current theme: {}", current_label),
        hint.to_string(),
    ];
    renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::Theme(current_id)),
        Some(InlineListSearchConfig {
            label: THEME_SEARCH_LABEL.to_string(),
            placeholder: Some(THEME_SEARCH_PLACEHOLDER.to_string()),
        }),
    );

    Ok(true)
}

fn theme_search_value(theme_id: &str, theme_label: &str) -> String {
    format!("{theme_label} {theme_id} theme appearance colors")
}

#[allow(clippy::vec_init_then_push)]
pub(crate) fn show_config_palette(
    renderer: &mut AnsiRenderer,
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
    selected: Option<InlineListSelection>,
) -> Result<bool> {
    let (source_label, config) = load_effective_config(workspace, vt_snapshot)?;
    let mut items = Vec::new();

    items.push(InlineListItem {
        title: "Reload from file".to_string(),
        subtitle: Some("Refresh values from vtcode.toml".to_string()),
        badge: Some("Action".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction("reload".to_string())),
        search_value: Some("reload refresh config settings vtcode.toml".to_string()),
    });

    items.push(config_toggle_item(
        "Autonomous mode",
        "agent.autonomous_mode",
        config.agent.autonomous_mode,
    ));
    items.push(config_toggle_item(
        "Planning mode",
        "agent.todo_planning_mode",
        config.agent.todo_planning_mode,
    ));
    items.push(config_toggle_item(
        "Checkpointing",
        "agent.checkpointing.enabled",
        config.agent.checkpointing.enabled,
    ));
    items.push(config_toggle_item(
        "Small model",
        "agent.small_model.enabled",
        config.agent.small_model.enabled,
    ));
    items.push(config_toggle_item(
        "Vibe coding",
        "agent.vibe_coding.enabled",
        config.agent.vibe_coding.enabled,
    ));
    items.push(config_toggle_item(
        "Prompt cache",
        "prompt_cache.enabled",
        config.prompt_cache.enabled,
    ));
    items.push(config_toggle_item(
        "MCP enabled",
        "mcp.enabled",
        config.mcp.enabled,
    ));
    items.push(config_toggle_item(
        "Full auto",
        "automation.full_auto.enabled",
        config.automation.full_auto.enabled,
    ));
    items.push(config_toggle_item(
        "Human in the loop",
        "security.human_in_the_loop",
        config.security.human_in_the_loop,
    ));
    items.push(config_toggle_item(
        "PTY enabled",
        "pty.enabled",
        config.pty.enabled,
    ));
    items.push(config_toggle_item(
        "Show sidebar",
        "ui.show_sidebar",
        config.ui.show_sidebar,
    ));
    items.push(config_toggle_item(
        "Dim completed todos",
        "ui.dim_completed_todos",
        config.ui.dim_completed_todos,
    ));
    items.push(config_toggle_item(
        "Message spacing",
        "ui.message_block_spacing",
        config.ui.message_block_spacing,
    ));
    items.push(config_toggle_item(
        "Show turn timer",
        "ui.show_turn_timer",
        config.ui.show_turn_timer,
    ));
    items.push(config_toggle_item(
        "Allow ANSI tool output",
        "ui.allow_tool_ansi",
        config.ui.allow_tool_ansi,
    ));
    items.push(config_toggle_item(
        "Notifications enabled",
        "ui.notifications.enabled",
        config.ui.notifications.enabled,
    ));
    items.push(config_toggle_item(
        "Notif: suppress when focused",
        "ui.notifications.suppress_when_focused",
        config.ui.notifications.suppress_when_focused,
    ));
    items.push(config_toggle_item(
        "Notif: tool failure",
        "ui.notifications.tool_failure",
        config.ui.notifications.tool_failure,
    ));
    items.push(config_toggle_item(
        "Notif: error",
        "ui.notifications.error",
        config.ui.notifications.error,
    ));
    items.push(config_toggle_item(
        "Notif: completion",
        "ui.notifications.completion",
        config.ui.notifications.completion,
    ));
    items.push(config_toggle_item(
        "Notif: HITL",
        "ui.notifications.hitl",
        config.ui.notifications.hitl,
    ));
    items.push(config_toggle_item(
        "Notif: tool success",
        "ui.notifications.tool_success",
        config.ui.notifications.tool_success,
    ));
    items.push(config_toggle_item(
        "Syntax highlighting",
        "syntax_highlighting.enabled",
        config.syntax_highlighting.enabled,
    ));
    items.push(config_toggle_item(
        "Keyboard protocol",
        "ui.keyboard_protocol.enabled",
        config.ui.keyboard_protocol.enabled,
    ));
    items.push(config_toggle_item(
        "Reasoning visible (toggle mode)",
        "ui.reasoning_visible_default",
        config.ui.reasoning_visible_default,
    ));
    items.push(config_toggle_item(
        "Screen reader mode",
        "ui.screen_reader_mode",
        config.ui.screen_reader_mode,
    ));
    items.push(config_toggle_item(
        "Reduce motion",
        "ui.reduce_motion_mode",
        config.ui.reduce_motion_mode,
    ));
    items.push(config_toggle_item(
        "Keep progress animation",
        "ui.reduce_motion_keep_progress_animation",
        config.ui.reduce_motion_keep_progress_animation,
    ));
    items.push(config_toggle_item(
        "Bold is bright",
        "ui.bold_is_bright",
        config.ui.bold_is_bright,
    ));
    items.push(config_toggle_item(
        "Safe colors only",
        "ui.safe_colors_only",
        config.ui.safe_colors_only,
    ));

    items.push(config_cycle_item(
        "Tool output mode",
        "ui.tool_output_mode",
        match config.ui.tool_output_mode {
            ToolOutputMode::Compact => "compact",
            ToolOutputMode::Full => "full",
        },
    ));
    items.push(config_cycle_item(
        "UI display mode",
        "ui.display_mode",
        match config.ui.display_mode {
            UiDisplayMode::Full => "full",
            UiDisplayMode::Minimal => "minimal",
            UiDisplayMode::Focused => "focused",
        },
    ));
    items.push(config_cycle_item(
        "Reasoning display mode",
        "ui.reasoning_display_mode",
        match config.ui.reasoning_display_mode {
            ReasoningDisplayMode::Always => "always",
            ReasoningDisplayMode::Toggle => "toggle",
            ReasoningDisplayMode::Hidden => "hidden",
        },
    ));
    items.push(config_cycle_item(
        "Color scheme mode",
        "ui.color_scheme_mode",
        match config.ui.color_scheme_mode {
            ColorSchemeMode::Auto => "auto",
            ColorSchemeMode::Light => "light",
            ColorSchemeMode::Dark => "dark",
        },
    ));
    items.push(config_cycle_item(
        "Notification delivery",
        "ui.notifications.delivery_mode",
        match config.ui.notifications.delivery_mode {
            NotificationDeliveryMode::Terminal => "terminal",
            NotificationDeliveryMode::Hybrid => "hybrid",
            NotificationDeliveryMode::Desktop => "desktop",
        },
    ));
    items.push(config_cycle_item(
        "Reasoning effort",
        "agent.reasoning_effort",
        config.agent.reasoning_effort.as_str(),
    ));
    items.push(config_cycle_item(
        "System prompt mode",
        "agent.system_prompt_mode",
        config.agent.system_prompt_mode.as_str(),
    ));
    items.push(config_cycle_item(
        "Tool docs mode",
        "agent.tool_documentation_mode",
        config.agent.tool_documentation_mode.as_str(),
    ));
    items.push(config_cycle_item(
        "Verbosity",
        "agent.verbosity",
        config.agent.verbosity.as_str(),
    ));
    items.push(config_cycle_item(
        "Tool policy",
        "tools.default_policy",
        match config.tools.default_policy {
            ToolPolicy::Allow => "allow",
            ToolPolicy::Prompt => "prompt",
            ToolPolicy::Deny => "deny",
        },
    ));
    items.push(config_cycle_item(
        "ACP trust",
        "acp.zed.workspace_trust",
        match config.acp.zed.workspace_trust {
            AgentClientProtocolZedWorkspaceTrustMode::FullAuto => "full_auto",
            AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => "tools_policy",
        },
    ));
    items.push(config_cycle_item(
        "Keyboard mode",
        "ui.keyboard_protocol.mode",
        &config.ui.keyboard_protocol.mode,
    ));
    items.push(config_cycle_item(
        "Theme",
        "agent.theme",
        &config.agent.theme,
    ));

    items.push(config_number_item(
        "Context tokens",
        "context.max_context_tokens",
        config.context.max_context_tokens as i64,
        "Enter to increase by 1024",
    ));
    items.push(config_number_item_dec(
        "Context tokens",
        "context.max_context_tokens",
        config.context.max_context_tokens as i64,
        "Enter to decrease by 1024",
    ));
    items.push(config_number_item(
        "Context trim %",
        "context.trim_to_percent",
        config.context.trim_to_percent as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "Context trim %",
        "context.trim_to_percent",
        config.context.trim_to_percent as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "Max conversation turns",
        "agent.max_conversation_turns",
        config.agent.max_conversation_turns as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "Max conversation turns",
        "agent.max_conversation_turns",
        config.agent.max_conversation_turns as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "Inline viewport rows",
        "ui.inline_viewport_rows",
        config.ui.inline_viewport_rows as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "Inline viewport rows",
        "ui.inline_viewport_rows",
        config.ui.inline_viewport_rows as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "PTY default rows",
        "pty.default_rows",
        config.pty.default_rows as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "PTY default rows",
        "pty.default_rows",
        config.pty.default_rows as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "PTY default cols",
        "pty.default_cols",
        config.pty.default_cols as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "PTY default cols",
        "pty.default_cols",
        config.pty.default_cols as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "PTY timeout (sec)",
        "pty.command_timeout_seconds",
        config.pty.command_timeout_seconds as i64,
        "Enter to increase by 1",
    ));
    items.push(config_number_item_dec(
        "PTY timeout (sec)",
        "pty.command_timeout_seconds",
        config.pty.command_timeout_seconds as i64,
        "Enter to decrease by 1",
    ));
    items.push(config_number_item(
        "Minimum contrast",
        "ui.minimum_contrast",
        (config.ui.minimum_contrast * 10.0).round() as i64,
        "Enter to cycle 1.0 / 3.0 / 4.5 / 7.0",
    ));

    items.push(InlineListItem {
        title: "Default model".to_string(),
        subtitle: Some(config.agent.default_model.clone()),
        badge: Some("Read-only".to_string()),
        indent: 0,
        selection: None,
        search_value: Some(config_search_value("Default model", "agent.default_model")),
    });

    let lines = vec![source_label, CONFIG_HINT.to_string()];
    let search = InlineListSearchConfig {
        label: "Search settings or keys".to_string(),
        placeholder: Some("Filter config options (fuzzy)".to_string()),
    };
    renderer.show_list_modal(CONFIG_PALETTE_TITLE, lines, items, selected, Some(search));

    Ok(true)
}

fn config_toggle_item(label: &str, key: &str, enabled: bool) -> InlineListItem {
    InlineListItem {
        title: label.to_string(),
        subtitle: Some(if enabled { "on" } else { "off" }.to_string()),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{}:toggle", key))),
        search_value: Some(config_search_value(label, key)),
    }
}

fn config_cycle_item(label: &str, key: &str, value: &str) -> InlineListItem {
    InlineListItem {
        title: label.to_string(),
        subtitle: Some(format!("← {} →", value)),
        badge: Some("Cycle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{}:cycle", key))),
        search_value: Some(config_search_value(label, key)),
    }
}

fn config_number_item(label: &str, key: &str, value: i64, hint: &str) -> InlineListItem {
    InlineListItem {
        title: label.to_string(),
        subtitle: Some(format!("{} ({})", value, hint)),
        badge: Some("Number".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{}:inc", key))),
        search_value: Some(config_search_value(label, key)),
    }
}

fn config_number_item_dec(label: &str, key: &str, value: i64, hint: &str) -> InlineListItem {
    InlineListItem {
        title: format!("{} (decrease)", label),
        subtitle: Some(format!("{} ({})", value, hint)),
        badge: Some("Number".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{}:dec", key))),
        search_value: Some(config_search_value(label, key)),
    }
}

fn config_search_value(label: &str, key: &str) -> String {
    let aliases = config_key_aliases(key);
    if aliases.is_empty() {
        format!("{} {}", label, key)
    } else {
        format!("{} {} {}", label, key, aliases)
    }
}

fn config_key_aliases(key: &str) -> &'static str {
    match key {
        "agent.autonomous_mode" => "auto autonomous mode",
        "agent.todo_planning_mode" => "plan planning todos",
        "agent.checkpointing.enabled" => "checkpoint snapshots restore",
        "agent.small_model.enabled" => "small model lightweight",
        "agent.vibe_coding.enabled" => "vibe coding relaxed",
        "prompt_cache.enabled" => "cache prompt reuse",
        "mcp.enabled" => "mcp model context protocol tools",
        "automation.full_auto.enabled" => "full auto automation",
        "security.human_in_the_loop" => "hitl approval confirm",
        "pty.enabled" => "terminal shell pty",
        "ui.show_sidebar" => "sidebar navigation panel",
        "ui.dim_completed_todos" => "todo dim completed",
        "ui.message_block_spacing" => "spacing compact message",
        "ui.show_turn_timer" => "timer turn duration",
        "ui.allow_tool_ansi" => "ansi color tools output",
        "ui.notifications.enabled" => "notifications alerts",
        "ui.notifications.suppress_when_focused" => "notifications focus suppress",
        "ui.notifications.tool_failure" => "notifications tool failure",
        "ui.notifications.error" => "notifications error",
        "ui.notifications.completion" => "notifications completion done",
        "ui.notifications.hitl" => "notifications hitl approval",
        "ui.notifications.tool_success" => "notifications tool success",
        "syntax_highlighting.enabled" => "syntax highlighting code colors",
        "ui.keyboard_protocol.enabled" => "keyboard protocol",
        "ui.reasoning_visible_default" => "reasoning visible default",
        "ui.screen_reader_mode" => "screen reader accessibility",
        "ui.reduce_motion_mode" => "reduce motion animation accessibility",
        "ui.reduce_motion_keep_progress_animation" => "progress animation reduce motion",
        "ui.bold_is_bright" => "bold bright terminal",
        "ui.safe_colors_only" => "safe colors ansi contrast",
        "ui.tool_output_mode" => "tool output compact full",
        "ui.display_mode" => "display ui mode focused minimal",
        "ui.reasoning_display_mode" => "reasoning display hidden toggle always",
        "ui.color_scheme_mode" => "theme color scheme dark light auto",
        "ui.notifications.delivery_mode" => "notification delivery desktop terminal",
        "agent.reasoning_effort" => "reasoning effort level",
        "agent.system_prompt_mode" => "system prompt mode",
        "agent.tool_documentation_mode" => "tool docs documentation mode",
        "agent.verbosity" => "verbosity concise detailed",
        "tools.default_policy" => "tool policy allow prompt deny",
        "acp.zed.workspace_trust" => "acp zed workspace trust",
        "ui.keyboard_protocol.mode" => "keyboard mode default full minimal custom",
        "agent.theme" => "theme appearance",
        "context.max_context_tokens" => "context tokens limit window",
        "context.trim_to_percent" => "context trim percent",
        "agent.max_conversation_turns" => "conversation turns max",
        "ui.inline_viewport_rows" => "inline viewport rows",
        "pty.default_rows" => "pty terminal rows",
        "pty.default_cols" => "pty terminal cols columns",
        "pty.command_timeout_seconds" => "pty timeout seconds",
        "ui.minimum_contrast" => "contrast accessibility wcag",
        "agent.default_model" => "default model",
        _ => "",
    }
}

fn load_effective_config(
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
) -> Result<(String, VTCodeConfig)> {
    let manager = ConfigManager::load()?;
    let config_path = manager.config_path().map(std::path::Path::to_path_buf);
    let config = if config_path.is_some() {
        manager.config().clone()
    } else if let Some(snapshot) = vt_snapshot {
        snapshot.clone()
    } else {
        manager.config().clone()
    };

    let source_label = if let Some(path) = config_path {
        format!("Configuration source: {}", path.display())
    } else {
        format!(
            "No vtcode.toml found for {}. Showing runtime defaults.",
            workspace.display()
        )
    };

    Ok((source_label, config))
}

fn apply_config_action(action: &str) -> Result<Option<String>> {
    let mut manager = ConfigManager::load()?;
    let mut config = manager.config().clone();

    let mut updated_key: Option<String> = None;

    match action {
        "reload" => return Ok(None),
        "agent.autonomous_mode:toggle" => {
            config.agent.autonomous_mode = !config.agent.autonomous_mode;
            updated_key = Some("agent.autonomous_mode".to_string());
        }
        "agent.todo_planning_mode:toggle" => {
            config.agent.todo_planning_mode = !config.agent.todo_planning_mode;
            updated_key = Some("agent.todo_planning_mode".to_string());
        }
        "agent.checkpointing.enabled:toggle" => {
            config.agent.checkpointing.enabled = !config.agent.checkpointing.enabled;
            updated_key = Some("agent.checkpointing.enabled".to_string());
        }
        "agent.small_model.enabled:toggle" => {
            config.agent.small_model.enabled = !config.agent.small_model.enabled;
            updated_key = Some("agent.small_model.enabled".to_string());
        }
        "agent.vibe_coding.enabled:toggle" => {
            config.agent.vibe_coding.enabled = !config.agent.vibe_coding.enabled;
            updated_key = Some("agent.vibe_coding.enabled".to_string());
        }
        "prompt_cache.enabled:toggle" => {
            config.prompt_cache.enabled = !config.prompt_cache.enabled;
            updated_key = Some("prompt_cache.enabled".to_string());
        }
        "mcp.enabled:toggle" => {
            config.mcp.enabled = !config.mcp.enabled;
            updated_key = Some("mcp.enabled".to_string());
        }
        "automation.full_auto.enabled:toggle" => {
            config.automation.full_auto.enabled = !config.automation.full_auto.enabled;
            updated_key = Some("automation.full_auto.enabled".to_string());
        }
        "security.human_in_the_loop:toggle" => {
            config.security.human_in_the_loop = !config.security.human_in_the_loop;
            updated_key = Some("security.human_in_the_loop".to_string());
        }
        "pty.enabled:toggle" => {
            config.pty.enabled = !config.pty.enabled;
            updated_key = Some("pty.enabled".to_string());
        }
        "ui.show_sidebar:toggle" => {
            config.ui.show_sidebar = !config.ui.show_sidebar;
            updated_key = Some("ui.show_sidebar".to_string());
        }
        "ui.dim_completed_todos:toggle" => {
            config.ui.dim_completed_todos = !config.ui.dim_completed_todos;
            updated_key = Some("ui.dim_completed_todos".to_string());
        }
        "ui.message_block_spacing:toggle" => {
            config.ui.message_block_spacing = !config.ui.message_block_spacing;
            updated_key = Some("ui.message_block_spacing".to_string());
        }
        "ui.show_turn_timer:toggle" => {
            config.ui.show_turn_timer = !config.ui.show_turn_timer;
            updated_key = Some("ui.show_turn_timer".to_string());
        }
        "ui.allow_tool_ansi:toggle" => {
            config.ui.allow_tool_ansi = !config.ui.allow_tool_ansi;
            updated_key = Some("ui.allow_tool_ansi".to_string());
        }
        "ui.notifications.enabled:toggle" => {
            config.ui.notifications.enabled = !config.ui.notifications.enabled;
            updated_key = Some("ui.notifications.enabled".to_string());
        }
        "ui.notifications.suppress_when_focused:toggle" => {
            config.ui.notifications.suppress_when_focused =
                !config.ui.notifications.suppress_when_focused;
            updated_key = Some("ui.notifications.suppress_when_focused".to_string());
        }
        "ui.notifications.tool_failure:toggle" => {
            config.ui.notifications.tool_failure = !config.ui.notifications.tool_failure;
            updated_key = Some("ui.notifications.tool_failure".to_string());
        }
        "ui.notifications.error:toggle" => {
            config.ui.notifications.error = !config.ui.notifications.error;
            updated_key = Some("ui.notifications.error".to_string());
        }
        "ui.notifications.completion:toggle" => {
            config.ui.notifications.completion = !config.ui.notifications.completion;
            updated_key = Some("ui.notifications.completion".to_string());
        }
        "ui.notifications.hitl:toggle" => {
            config.ui.notifications.hitl = !config.ui.notifications.hitl;
            updated_key = Some("ui.notifications.hitl".to_string());
        }
        "ui.notifications.tool_success:toggle" => {
            config.ui.notifications.tool_success = !config.ui.notifications.tool_success;
            updated_key = Some("ui.notifications.tool_success".to_string());
        }
        "syntax_highlighting.enabled:toggle" => {
            config.syntax_highlighting.enabled = !config.syntax_highlighting.enabled;
            updated_key = Some("syntax_highlighting.enabled".to_string());
        }
        "ui.keyboard_protocol.enabled:toggle" => {
            config.ui.keyboard_protocol.enabled = !config.ui.keyboard_protocol.enabled;
            updated_key = Some("ui.keyboard_protocol.enabled".to_string());
        }
        "ui.reasoning_visible_default:toggle" => {
            config.ui.reasoning_visible_default = !config.ui.reasoning_visible_default;
            updated_key = Some("ui.reasoning_visible_default".to_string());
        }
        "ui.screen_reader_mode:toggle" => {
            config.ui.screen_reader_mode = !config.ui.screen_reader_mode;
            updated_key = Some("ui.screen_reader_mode".to_string());
        }
        "ui.reduce_motion_mode:toggle" => {
            config.ui.reduce_motion_mode = !config.ui.reduce_motion_mode;
            updated_key = Some("ui.reduce_motion_mode".to_string());
        }
        "ui.reduce_motion_keep_progress_animation:toggle" => {
            config.ui.reduce_motion_keep_progress_animation =
                !config.ui.reduce_motion_keep_progress_animation;
            updated_key = Some("ui.reduce_motion_keep_progress_animation".to_string());
        }
        "ui.bold_is_bright:toggle" => {
            config.ui.bold_is_bright = !config.ui.bold_is_bright;
            updated_key = Some("ui.bold_is_bright".to_string());
        }
        "ui.safe_colors_only:toggle" => {
            config.ui.safe_colors_only = !config.ui.safe_colors_only;
            updated_key = Some("ui.safe_colors_only".to_string());
        }
        "ui.tool_output_mode:cycle" => {
            config.ui.tool_output_mode = match config.ui.tool_output_mode {
                ToolOutputMode::Compact => ToolOutputMode::Full,
                ToolOutputMode::Full => ToolOutputMode::Compact,
            };
            updated_key = Some("ui.tool_output_mode".to_string());
        }
        "ui.tool_output_mode:cycle_prev" => {
            config.ui.tool_output_mode = match config.ui.tool_output_mode {
                ToolOutputMode::Compact => ToolOutputMode::Full,
                ToolOutputMode::Full => ToolOutputMode::Compact,
            };
            updated_key = Some("ui.tool_output_mode".to_string());
        }
        "ui.display_mode:cycle" => {
            config.ui.display_mode = match config.ui.display_mode {
                UiDisplayMode::Full => UiDisplayMode::Minimal,
                UiDisplayMode::Minimal => UiDisplayMode::Focused,
                UiDisplayMode::Focused => UiDisplayMode::Full,
            };
            updated_key = Some("ui.display_mode".to_string());
        }
        "ui.display_mode:cycle_prev" => {
            config.ui.display_mode = match config.ui.display_mode {
                UiDisplayMode::Full => UiDisplayMode::Focused,
                UiDisplayMode::Minimal => UiDisplayMode::Full,
                UiDisplayMode::Focused => UiDisplayMode::Minimal,
            };
            updated_key = Some("ui.display_mode".to_string());
        }
        "ui.reasoning_display_mode:cycle" => {
            config.ui.reasoning_display_mode = match config.ui.reasoning_display_mode {
                ReasoningDisplayMode::Always => ReasoningDisplayMode::Toggle,
                ReasoningDisplayMode::Toggle => ReasoningDisplayMode::Hidden,
                ReasoningDisplayMode::Hidden => ReasoningDisplayMode::Always,
            };
            updated_key = Some("ui.reasoning_display_mode".to_string());
        }
        "ui.reasoning_display_mode:cycle_prev" => {
            config.ui.reasoning_display_mode = match config.ui.reasoning_display_mode {
                ReasoningDisplayMode::Always => ReasoningDisplayMode::Hidden,
                ReasoningDisplayMode::Toggle => ReasoningDisplayMode::Always,
                ReasoningDisplayMode::Hidden => ReasoningDisplayMode::Toggle,
            };
            updated_key = Some("ui.reasoning_display_mode".to_string());
        }
        "ui.color_scheme_mode:cycle" => {
            config.ui.color_scheme_mode = match config.ui.color_scheme_mode {
                ColorSchemeMode::Auto => ColorSchemeMode::Light,
                ColorSchemeMode::Light => ColorSchemeMode::Dark,
                ColorSchemeMode::Dark => ColorSchemeMode::Auto,
            };
            updated_key = Some("ui.color_scheme_mode".to_string());
        }
        "ui.color_scheme_mode:cycle_prev" => {
            config.ui.color_scheme_mode = match config.ui.color_scheme_mode {
                ColorSchemeMode::Auto => ColorSchemeMode::Dark,
                ColorSchemeMode::Light => ColorSchemeMode::Auto,
                ColorSchemeMode::Dark => ColorSchemeMode::Light,
            };
            updated_key = Some("ui.color_scheme_mode".to_string());
        }
        "ui.notifications.delivery_mode:cycle" => {
            config.ui.notifications.delivery_mode = match config.ui.notifications.delivery_mode {
                NotificationDeliveryMode::Terminal => NotificationDeliveryMode::Hybrid,
                NotificationDeliveryMode::Hybrid => NotificationDeliveryMode::Desktop,
                NotificationDeliveryMode::Desktop => NotificationDeliveryMode::Terminal,
            };
            updated_key = Some("ui.notifications.delivery_mode".to_string());
        }
        "ui.notifications.delivery_mode:cycle_prev" => {
            config.ui.notifications.delivery_mode = match config.ui.notifications.delivery_mode {
                NotificationDeliveryMode::Terminal => NotificationDeliveryMode::Desktop,
                NotificationDeliveryMode::Hybrid => NotificationDeliveryMode::Terminal,
                NotificationDeliveryMode::Desktop => NotificationDeliveryMode::Hybrid,
            };
            updated_key = Some("ui.notifications.delivery_mode".to_string());
        }
        "agent.reasoning_effort:cycle" => {
            config.agent.reasoning_effort = match config.agent.reasoning_effort {
                ReasoningEffortLevel::None => ReasoningEffortLevel::Minimal,
                ReasoningEffortLevel::Minimal => ReasoningEffortLevel::Low,
                ReasoningEffortLevel::Low => ReasoningEffortLevel::Medium,
                ReasoningEffortLevel::Medium => ReasoningEffortLevel::High,
                ReasoningEffortLevel::High => ReasoningEffortLevel::XHigh,
                ReasoningEffortLevel::XHigh => ReasoningEffortLevel::None,
            };
            updated_key = Some("agent.reasoning_effort".to_string());
        }
        "agent.reasoning_effort:cycle_prev" => {
            config.agent.reasoning_effort = match config.agent.reasoning_effort {
                ReasoningEffortLevel::None => ReasoningEffortLevel::XHigh,
                ReasoningEffortLevel::Minimal => ReasoningEffortLevel::None,
                ReasoningEffortLevel::Low => ReasoningEffortLevel::Minimal,
                ReasoningEffortLevel::Medium => ReasoningEffortLevel::Low,
                ReasoningEffortLevel::High => ReasoningEffortLevel::Medium,
                ReasoningEffortLevel::XHigh => ReasoningEffortLevel::High,
            };
            updated_key = Some("agent.reasoning_effort".to_string());
        }
        "agent.system_prompt_mode:cycle" => {
            config.agent.system_prompt_mode = match config.agent.system_prompt_mode {
                SystemPromptMode::Minimal => SystemPromptMode::Lightweight,
                SystemPromptMode::Lightweight => SystemPromptMode::Default,
                SystemPromptMode::Default => SystemPromptMode::Specialized,
                SystemPromptMode::Specialized => SystemPromptMode::Minimal,
            };
            updated_key = Some("agent.system_prompt_mode".to_string());
        }
        "agent.system_prompt_mode:cycle_prev" => {
            config.agent.system_prompt_mode = match config.agent.system_prompt_mode {
                SystemPromptMode::Minimal => SystemPromptMode::Specialized,
                SystemPromptMode::Lightweight => SystemPromptMode::Minimal,
                SystemPromptMode::Default => SystemPromptMode::Lightweight,
                SystemPromptMode::Specialized => SystemPromptMode::Default,
            };
            updated_key = Some("agent.system_prompt_mode".to_string());
        }
        "agent.tool_documentation_mode:cycle" => {
            config.agent.tool_documentation_mode = match config.agent.tool_documentation_mode {
                ToolDocumentationMode::Minimal => ToolDocumentationMode::Progressive,
                ToolDocumentationMode::Progressive => ToolDocumentationMode::Full,
                ToolDocumentationMode::Full => ToolDocumentationMode::Minimal,
            };
            updated_key = Some("agent.tool_documentation_mode".to_string());
        }
        "agent.tool_documentation_mode:cycle_prev" => {
            config.agent.tool_documentation_mode = match config.agent.tool_documentation_mode {
                ToolDocumentationMode::Minimal => ToolDocumentationMode::Full,
                ToolDocumentationMode::Progressive => ToolDocumentationMode::Minimal,
                ToolDocumentationMode::Full => ToolDocumentationMode::Progressive,
            };
            updated_key = Some("agent.tool_documentation_mode".to_string());
        }
        "agent.verbosity:cycle" => {
            config.agent.verbosity = match config.agent.verbosity {
                VerbosityLevel::Low => VerbosityLevel::Medium,
                VerbosityLevel::Medium => VerbosityLevel::High,
                VerbosityLevel::High => VerbosityLevel::Low,
            };
            updated_key = Some("agent.verbosity".to_string());
        }
        "agent.verbosity:cycle_prev" => {
            config.agent.verbosity = match config.agent.verbosity {
                VerbosityLevel::Low => VerbosityLevel::High,
                VerbosityLevel::Medium => VerbosityLevel::Low,
                VerbosityLevel::High => VerbosityLevel::Medium,
            };
            updated_key = Some("agent.verbosity".to_string());
        }
        "tools.default_policy:cycle" => {
            config.tools.default_policy = match config.tools.default_policy {
                ToolPolicy::Allow => ToolPolicy::Prompt,
                ToolPolicy::Prompt => ToolPolicy::Deny,
                ToolPolicy::Deny => ToolPolicy::Allow,
            };
            updated_key = Some("tools.default_policy".to_string());
        }
        "tools.default_policy:cycle_prev" => {
            config.tools.default_policy = match config.tools.default_policy {
                ToolPolicy::Allow => ToolPolicy::Deny,
                ToolPolicy::Prompt => ToolPolicy::Allow,
                ToolPolicy::Deny => ToolPolicy::Prompt,
            };
            updated_key = Some("tools.default_policy".to_string());
        }
        "acp.zed.workspace_trust:cycle" => {
            config.acp.zed.workspace_trust = match config.acp.zed.workspace_trust {
                AgentClientProtocolZedWorkspaceTrustMode::FullAuto => {
                    AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy
                }
                AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                    AgentClientProtocolZedWorkspaceTrustMode::FullAuto
                }
            };
            updated_key = Some("acp.zed.workspace_trust".to_string());
        }
        "acp.zed.workspace_trust:cycle_prev" => {
            config.acp.zed.workspace_trust = match config.acp.zed.workspace_trust {
                AgentClientProtocolZedWorkspaceTrustMode::FullAuto => {
                    AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy
                }
                AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                    AgentClientProtocolZedWorkspaceTrustMode::FullAuto
                }
            };
            updated_key = Some("acp.zed.workspace_trust".to_string());
        }
        "ui.keyboard_protocol.mode:cycle" => {
            config.ui.keyboard_protocol.mode = match config.ui.keyboard_protocol.mode.as_str() {
                "default" => "full".to_string(),
                "full" => "minimal".to_string(),
                "minimal" => "custom".to_string(),
                _ => "default".to_string(),
            };
            updated_key = Some("ui.keyboard_protocol.mode".to_string());
        }
        "ui.keyboard_protocol.mode:cycle_prev" => {
            config.ui.keyboard_protocol.mode = match config.ui.keyboard_protocol.mode.as_str() {
                "default" => "custom".to_string(),
                "full" => "default".to_string(),
                "minimal" => "full".to_string(),
                "custom" => "minimal".to_string(),
                _ => "default".to_string(),
            };
            updated_key = Some("ui.keyboard_protocol.mode".to_string());
        }
        "agent.theme:cycle" => {
            let themes = theme::available_themes();
            if !themes.is_empty() {
                let current = config.agent.theme.clone();
                let next_index = themes
                    .iter()
                    .position(|entry| *entry == current)
                    .map(|index| (index + 1) % themes.len())
                    .unwrap_or(0);
                config.agent.theme = themes[next_index].to_string();
                updated_key = Some("agent.theme".to_string());
            }
        }
        "agent.theme:cycle_prev" => {
            let themes = theme::available_themes();
            if !themes.is_empty() {
                let current = config.agent.theme.clone();
                let prev_index = themes
                    .iter()
                    .position(|entry| *entry == current)
                    .map(|index| {
                        if index == 0 {
                            themes.len() - 1
                        } else {
                            index - 1
                        }
                    })
                    .unwrap_or(0);
                config.agent.theme = themes[prev_index].to_string();
                updated_key = Some("agent.theme".to_string());
            }
        }
        "context.max_context_tokens:inc" => {
            config.context.max_context_tokens =
                (config.context.max_context_tokens.saturating_add(1024)).clamp(4096, 2000000);
            updated_key = Some("context.max_context_tokens".to_string());
        }
        "context.max_context_tokens:dec" => {
            config.context.max_context_tokens = config
                .context
                .max_context_tokens
                .saturating_sub(1024)
                .clamp(4096, 2000000);
            updated_key = Some("context.max_context_tokens".to_string());
        }
        "context.trim_to_percent:inc" => {
            config.context.trim_to_percent =
                (config.context.trim_to_percent as i64 + 1).clamp(10, 95) as u8;
            updated_key = Some("context.trim_to_percent".to_string());
        }
        "context.trim_to_percent:dec" => {
            config.context.trim_to_percent =
                (config.context.trim_to_percent as i64 - 1).clamp(10, 95) as u8;
            updated_key = Some("context.trim_to_percent".to_string());
        }
        "agent.max_conversation_turns:inc" => {
            config.agent.max_conversation_turns =
                (config.agent.max_conversation_turns as i64 + 1).clamp(10, 500) as usize;
            updated_key = Some("agent.max_conversation_turns".to_string());
        }
        "agent.max_conversation_turns:dec" => {
            config.agent.max_conversation_turns =
                (config.agent.max_conversation_turns as i64 - 1).clamp(10, 500) as usize;
            updated_key = Some("agent.max_conversation_turns".to_string());
        }
        "ui.inline_viewport_rows:inc" => {
            config.ui.inline_viewport_rows =
                (config.ui.inline_viewport_rows as i64 + 1).clamp(5, 50) as u16;
            updated_key = Some("ui.inline_viewport_rows".to_string());
        }
        "ui.inline_viewport_rows:dec" => {
            config.ui.inline_viewport_rows =
                (config.ui.inline_viewport_rows as i64 - 1).clamp(5, 50) as u16;
            updated_key = Some("ui.inline_viewport_rows".to_string());
        }
        "pty.default_rows:inc" => {
            config.pty.default_rows = (config.pty.default_rows as i64 + 1).clamp(10, 100) as u16;
            updated_key = Some("pty.default_rows".to_string());
        }
        "pty.default_rows:dec" => {
            config.pty.default_rows = (config.pty.default_rows as i64 - 1).clamp(10, 100) as u16;
            updated_key = Some("pty.default_rows".to_string());
        }
        "pty.default_cols:inc" => {
            config.pty.default_cols = (config.pty.default_cols as i64 + 1).clamp(40, 200) as u16;
            updated_key = Some("pty.default_cols".to_string());
        }
        "pty.default_cols:dec" => {
            config.pty.default_cols = (config.pty.default_cols as i64 - 1).clamp(40, 200) as u16;
            updated_key = Some("pty.default_cols".to_string());
        }
        "pty.command_timeout_seconds:inc" => {
            config.pty.command_timeout_seconds =
                (config.pty.command_timeout_seconds as i64 + 1).clamp(10, 3600) as u64;
            updated_key = Some("pty.command_timeout_seconds".to_string());
        }
        "pty.command_timeout_seconds:dec" => {
            config.pty.command_timeout_seconds =
                (config.pty.command_timeout_seconds as i64 - 1).clamp(10, 3600) as u64;
            updated_key = Some("pty.command_timeout_seconds".to_string());
        }
        "ui.minimum_contrast:inc" => {
            let levels = [1.0_f64, 3.0, 4.5, 7.0];
            let current = config.ui.minimum_contrast;
            let next = levels
                .iter()
                .position(|entry| (*entry - current).abs() < f64::EPSILON)
                .map(|index| levels[(index + 1) % levels.len()])
                .unwrap_or(4.5);
            config.ui.minimum_contrast = next;
            updated_key = Some("ui.minimum_contrast".to_string());
        }
        _ => {}
    }

    if updated_key.is_some() {
        manager.save_config(&config)?;
    }

    Ok(updated_key)
}

pub(crate) fn show_sessions_palette(
    renderer: &mut AnsiRenderer,
    listings: &[SessionListing],
    limit: usize,
) -> Result<bool> {
    if listings.is_empty() {
        renderer.line(MessageStyle::Info, "No archived sessions found.")?;
        return Ok(false);
    }

    let mut items = Vec::with_capacity(listings.len());
    for (index, listing) in listings.iter().enumerate() {
        let ended_local = listing
            .snapshot
            .ended_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M");
        let duration = listing
            .snapshot
            .ended_at
            .signed_duration_since(listing.snapshot.started_at);
        let duration_std = duration.to_std().unwrap_or_else(|_| Duration::from_secs(0));
        let detail = format!(
            "Duration: {} · {} msgs · {} tools",
            format_duration_label(duration_std),
            listing.snapshot.total_messages,
            listing.snapshot.distinct_tools.len(),
        );
        let badge = (index == 0).then(|| SESSIONS_LATEST_BADGE.to_string());
        items.push(InlineListItem {
            title: format!(
                "{} · {} · {}",
                ended_local,
                listing.snapshot.metadata.model,
                listing.snapshot.metadata.workspace_label,
            ),
            subtitle: Some(detail),
            badge,
            indent: 0,
            selection: Some(InlineListSelection::Session(listing.identifier())),
            search_value: None,
        });
    }

    let lines = vec![
        format!("Showing {} of {} archived sessions", listings.len(), limit),
        SESSIONS_HINT_PRIMARY.to_string(),
        SESSIONS_HINT_SECONDARY.to_string(),
    ];
    let selected = listings
        .first()
        .map(|listing| InlineListSelection::Session(listing.identifier()));
    renderer.show_list_modal(SESSIONS_PALETTE_TITLE, lines, items, selected, None);
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_palette_selection(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    config: &mut vtcode_core::config::types::AgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
    provider_client: &dyn vtcode_core::llm::provider::LLMProvider,
    session_bootstrap: &SessionBootstrap,
    full_auto: bool,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => match selection {
            InlineListSelection::Theme(theme_id) => match mode {
                ThemePaletteMode::Select => {
                    match theme::set_active_theme(&theme_id) {
                        Ok(()) => {
                            let label = theme::active_theme_label();
                            renderer.line(
                                MessageStyle::Info,
                                &format!("Theme switched to {}", label),
                            )?;
                            persist_theme_preference(renderer, &theme_id).await?;
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                        Err(err) => {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Theme '{}' not available: {}", theme_id, err),
                            )?;
                        }
                    }
                    Ok(None)
                }
            },
            _ => Ok(Some(ActivePalette::Theme {
                mode,
                original_theme_id,
            })),
        },
        ActivePalette::Sessions { listings, limit } => {
            // Session selection is handled earlier in the modal handler
            // This path is for refreshing the palette display
            if show_sessions_palette(renderer, &listings, limit)? {
                Ok(Some(ActivePalette::Sessions { listings, limit }))
            } else {
                Ok(None)
            }
        }
        ActivePalette::Config {
            workspace,
            vt_snapshot,
            selected,
        } => {
            let normalized_selection = normalize_config_selection(selection.clone());
            let selected_for_modal = normalized_selection.clone().or(selected.clone());

            if let InlineListSelection::ConfigAction(action) = selection
                && let Some(updated_key) = apply_config_action(&action)?
            {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Saved {} to vtcode.toml", updated_key),
                )?;
            }

            if let Ok(runtime_manager) = ConfigManager::load() {
                let runtime_config = runtime_manager.config().clone();
                *vt_cfg = Some(runtime_config.clone());
                config.reasoning_effort = runtime_config.agent.reasoning_effort;

                let _ = theme::set_active_theme(&runtime_config.agent.theme);
                let styles = theme::active_styles();
                handle.set_theme(inline_theme_from_core_styles(&styles));
                handle.set_appearance(to_tui_appearance(&runtime_config));

                let provider_label = if config.provider.trim().is_empty() {
                    provider_client.name().to_string()
                } else {
                    config.provider.clone()
                };
                let reasoning_label = config.reasoning_effort.as_str().to_string();
                let mode_label = match (config.ui_surface, full_auto) {
                    (vtcode_core::config::types::UiSurfacePreference::Inline, true) => {
                        "auto".to_string()
                    }
                    (vtcode_core::config::types::UiSurfacePreference::Inline, false) => {
                        "inline".to_string()
                    }
                    (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => {
                        "alt".to_string()
                    }
                    (vtcode_core::config::types::UiSurfacePreference::Auto, true) => {
                        "auto".to_string()
                    }
                    (vtcode_core::config::types::UiSurfacePreference::Auto, false) => {
                        "std".to_string()
                    }
                };
                if let Ok(header_context) = build_inline_header_context(
                    config,
                    session_bootstrap,
                    provider_label,
                    config.model.clone(),
                    mode_label,
                    reasoning_label,
                )
                .await
                {
                    handle.set_header_context(header_context);
                }

                apply_prompt_style(handle);
                handle.force_redraw();
            }

            if show_config_palette(renderer, &workspace, &vt_snapshot, selected_for_modal)? {
                Ok(Some(ActivePalette::Config {
                    workspace,
                    vt_snapshot: Box::new(*vt_snapshot),
                    selected: normalized_selection,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

pub(crate) fn handle_palette_preview(
    palette: ActivePalette,
    selection: InlineListSelection,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<Option<ActivePalette>> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => {
            if let InlineListSelection::Theme(theme_id) = selection {
                match mode {
                    ThemePaletteMode::Select => {
                        if let Err(err) = theme::set_active_theme(&theme_id) {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Theme '{}' not available: {}", theme_id, err),
                            )?;
                        } else {
                            let styles = theme::active_styles();
                            handle.set_theme(inline_theme_from_core_styles(&styles));
                            apply_prompt_style(handle);
                            handle.force_redraw();
                        }
                    }
                }
            }
            Ok(Some(ActivePalette::Theme {
                mode,
                original_theme_id,
            }))
        }
        other => Ok(Some(other)),
    }
}

fn normalize_config_selection(selection: InlineListSelection) -> Option<InlineListSelection> {
    match selection {
        InlineListSelection::ConfigAction(action) if action.ends_with(":cycle_prev") => {
            let normalized = action.replace(":cycle_prev", ":cycle");
            Some(InlineListSelection::ConfigAction(normalized))
        }
        value => Some(value),
    }
}

pub(crate) fn handle_palette_cancel(
    palette: ActivePalette,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
) -> Result<()> {
    match palette {
        ActivePalette::Theme {
            mode,
            original_theme_id,
        } => {
            if theme::active_theme_id() != original_theme_id
                && theme::set_active_theme(&original_theme_id).is_ok()
            {
                let styles = theme::active_styles();
                handle.set_theme(inline_theme_from_core_styles(&styles));
                apply_prompt_style(handle);
                handle.force_redraw();
            }
            let message = match mode {
                ThemePaletteMode::Select => "Theme selection cancelled.",
            };
            renderer.line(MessageStyle::Info, message)?;
        }
        ActivePalette::Sessions { .. } => {
            renderer.line(MessageStyle::Info, "Closed session browser.")?;
        }
        ActivePalette::Config { .. } => {
            renderer.line(MessageStyle::Info, "Closed configuration settings.")?;
        }
    }
    Ok(())
}

pub(crate) fn format_duration_label(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 || hours > 0 {
        parts.push(format!("{}m", minutes));
    }
    parts.push(format!("{}s", seconds));
    parts.join(" ")
}

pub(crate) fn apply_prompt_style(handle: &InlineHandle) {
    let styles = theme::active_styles();
    let style = convert_style(styles.primary);
    handle.set_prompt("".to_string(), style);
}
