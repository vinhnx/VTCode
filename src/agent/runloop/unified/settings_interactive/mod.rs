mod docs;
mod items;
mod mutations;
mod path;
mod render;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use super::config_section_headings::heading_for_path;
use anyhow::{Context, Result, anyhow, bail};
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineListSearchConfig, InlineListSelection};

#[cfg(test)]
use docs::FIELD_DOCS;
use items::build_settings_items;
use mutations::{
    ScalarOperation, add_array_item, apply_scalar_operation, mutate_draft_and_persist,
    no_config_source_label, pop_array_item, reload_state_from_disk,
};
#[cfg(test)]
use mutations::{mutate_draft, render_commented_config};
pub(crate) use path::parent_view_path;
#[cfg(test)]
use path::{PathToken, parse_path_tokens};

const SETTINGS_TITLE: &str = "VT Code Settings";
const SETTINGS_HINT: &str = "Enter open/apply • ←/→ adjust • Esc back • Double Esc close";
const SETTINGS_SEARCH_LABEL: &str = "Search settings";
const SETTINGS_SEARCH_PLACEHOLDER: &str = "section, setting, or value";
const ACTION_RELOAD: &str = "settings:reload";
const ACTION_OPEN_ROOT: &str = "settings:open_root";
const ACTION_PREFIX_OPEN: &str = "settings:open:";
const ACTION_PREFIX_ARRAY_ADD: &str = "settings:array_add:";
const ACTION_PREFIX_ARRAY_POP: &str = "settings:array_pop:";
const ACTION_PREFIX_SET: &str = "settings:set:";
const OPTIONAL_DOC_FIELDS: &[&str] = &[
    "provider.anthropic.thinking_display",
    "provider.openai.service_tier",
];
pub(crate) const SETTINGS_MODEL_CONFIG_PATH: &str = "model_config";
pub(crate) const SETTINGS_MODEL_CONFIG_MAIN_PATH: &str = "model_config.main";
pub(crate) const SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH: &str = "model_config.lightweight";
pub(crate) const ACTION_PICK_MAIN_MODEL: &str = "settings:pick_main_model";
pub(crate) const ACTION_PICK_LIGHTWEIGHT_MODEL: &str = "settings:pick_lightweight_model";
pub(crate) const ACTION_CONFIGURE_EDITOR: &str = "settings:configure_editor";

#[derive(Clone)]
pub(crate) struct SettingsPaletteState {
    pub(crate) workspace: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) source_label: String,
    pub(crate) draft: VTCodeConfig,
    pub(crate) view_path: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct SettingsApplyOutcome {
    pub(crate) message: Option<String>,
    pub(crate) saved: bool,
}

pub(crate) fn create_settings_palette_state(
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
) -> Result<SettingsPaletteState> {
    let manager =
        ConfigManager::load_from_workspace(workspace).context("Failed to load configuration")?;
    let has_config_file = manager.config_path().is_some();
    let source_path = manager
        .config_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace.join("vtcode.toml"));

    let draft = if has_config_file {
        manager.config().clone()
    } else {
        vt_snapshot
            .clone()
            .unwrap_or_else(|| manager.config().clone())
    };

    let source_label = if has_config_file {
        format!("Configuration source: {}", source_path.display())
    } else {
        no_config_source_label(workspace)
    };

    Ok(SettingsPaletteState {
        workspace: workspace.to_path_buf(),
        source_path,
        source_label,
        draft,
        view_path: None,
    })
}

pub(crate) fn show_settings_palette(
    renderer: &mut AnsiRenderer,
    state: &SettingsPaletteState,
    selected: Option<InlineListSelection>,
) -> Result<bool> {
    let draft_value = TomlValue::try_from(state.draft.clone())
        .context("Failed to serialize draft configuration")?;

    let mut lines = Vec::new();
    lines.push(state.source_label.clone());
    if let Some(view_path) = state.view_path.as_deref() {
        let heading = heading_for_path(view_path);
        lines.push(format!(
            "{} ({})",
            heading.title,
            display_settings_view_path(view_path)
        ));
        if !heading.summary.is_empty() {
            lines.push(heading.summary.into_owned());
        }
        if view_path == "permissions" {
            lines.push(format_permission_summary(&state.draft));
        }
    } else {
        lines.push("Choose a section to edit.".to_string());
    }
    lines.push(SETTINGS_HINT.to_string());

    let items = build_settings_items(state, &draft_value)?;
    if items.is_empty() {
        return Ok(false);
    }

    renderer.show_list_modal(
        SETTINGS_TITLE,
        lines,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: SETTINGS_SEARCH_LABEL.to_string(),
            placeholder: Some(SETTINGS_SEARCH_PLACEHOLDER.to_string()),
        }),
    );

    Ok(true)
}

fn format_permission_summary(config: &VTCodeConfig) -> String {
    let mode = match config.permissions.default_mode {
        vtcode_core::config::PermissionMode::Default => "default",
        vtcode_core::config::PermissionMode::AcceptEdits => "accept_edits",
        vtcode_core::config::PermissionMode::Auto => "auto",
        vtcode_core::config::PermissionMode::Plan => "plan",
        vtcode_core::config::PermissionMode::DontAsk => "dont_ask",
        vtcode_core::config::PermissionMode::BypassPermissions => "bypass_permissions",
    };

    format!(
        "Effective mode: {mode} | deny: {} | ask: {} | allow: {}",
        config.permissions.deny.len(),
        config.permissions.ask.len(),
        config.permissions.allow.len()
    )
}

pub(crate) fn apply_settings_action(
    state: &mut SettingsPaletteState,
    action: &str,
) -> Result<SettingsApplyOutcome> {
    let mut outcome = SettingsApplyOutcome::default();

    if matches!(
        action,
        ACTION_PICK_MAIN_MODEL | ACTION_PICK_LIGHTWEIGHT_MODEL | ACTION_CONFIGURE_EDITOR
    ) {
        return Ok(outcome);
    }

    match action {
        ACTION_RELOAD => {
            reload_state_from_disk(state)?;
            outcome.message = Some("Reloaded settings from disk.".to_string());
            return Ok(outcome);
        }
        ACTION_OPEN_ROOT => {
            state.view_path = None;
            return Ok(outcome);
        }
        _ => {}
    }

    if let Some(path) = action.strip_prefix(ACTION_PREFIX_OPEN) {
        if path.trim().is_empty() {
            state.view_path = None;
        } else {
            state.view_path = Some(path.to_string());
        }
        return Ok(outcome);
    }

    if let Some(path) = action.strip_prefix(ACTION_PREFIX_ARRAY_ADD) {
        mutate_draft_and_persist(state, |draft| add_array_item(draft, path))?;
        outcome.saved = true;
        return Ok(outcome);
    }

    if let Some(path) = action.strip_prefix(ACTION_PREFIX_ARRAY_POP) {
        mutate_draft_and_persist(state, |draft| pop_array_item(draft, path))?;
        outcome.saved = true;
        return Ok(outcome);
    }

    if let Some(rest) = action.strip_prefix(ACTION_PREFIX_SET) {
        let (path, op) = rest
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("Invalid settings action: {}", action))?;

        let operation = match op {
            "toggle" => ScalarOperation::Toggle,
            "inc" => ScalarOperation::Increment,
            "dec" => ScalarOperation::Decrement,
            "cycle" => ScalarOperation::CycleNext,
            "cycle_prev" => ScalarOperation::CyclePrev,
            _ => bail!("Unsupported settings operation: {}", op),
        };

        mutate_draft_and_persist(state, |draft| {
            apply_scalar_operation(draft, path, operation)
        })?;
        outcome.saved = true;
        return Ok(outcome);
    }

    bail!("Unknown settings action: {}", action)
}

pub(crate) fn resolve_settings_view_path(path: &str) -> String {
    match path.trim() {
        "model" => SETTINGS_MODEL_CONFIG_PATH.to_string(),
        "model.main" => SETTINGS_MODEL_CONFIG_MAIN_PATH.to_string(),
        "model.lightweight" => SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH.to_string(),
        "codex" | "codex_app_server" | "codex.app_server" | "app_server" => {
            "agent.codex_app_server".to_string()
        }
        other => other.to_string(),
    }
}

pub(crate) fn display_settings_view_path(path: &str) -> Cow<'_, str> {
    match path {
        SETTINGS_MODEL_CONFIG_PATH => Cow::Borrowed("model"),
        SETTINGS_MODEL_CONFIG_MAIN_PATH => Cow::Borrowed("model.main"),
        SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH => Cow::Borrowed("model.lightweight"),
        other => Cow::Borrowed(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::config_section_headings::normalize_config_path;

    #[test]
    fn parse_path_handles_arrays() {
        let tokens = parse_path_tokens("commands.allow_list[1]").expect("tokens");
        assert_eq!(tokens.len(), 3);
        matches!(tokens[0], PathToken::Key(_));
        matches!(tokens[1], PathToken::Key(_));
        matches!(tokens[2], PathToken::Index(1));
    }

    #[test]
    fn normalize_field_path_replaces_indexes() {
        assert_eq!(
            normalize_config_path("commands.allow_list[12]"),
            "commands.allow_list[]"
        );
    }

    #[test]
    fn parent_view_path_handles_nested_segments() {
        assert_eq!(parent_view_path("agent"), None);
        assert_eq!(
            parent_view_path("agent.vibe_coding"),
            Some("agent".to_string())
        );
        assert_eq!(
            parent_view_path("hooks.lifecycle.pre_tool_use[0].hooks[2]"),
            Some("hooks.lifecycle.pre_tool_use[0].hooks".to_string())
        );
    }

    #[test]
    fn parse_field_docs_has_known_entry() {
        assert!(FIELD_DOCS.lookup("agent.provider").is_some());
    }

    #[test]
    fn root_settings_items_include_nested_keys_for_global_search() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [tools.editor]
            preferred_editor = "code --wait"
            [agent]
            quiet = true
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let tools_entry = items
            .iter()
            .find(|item| item.title == "Tool Defaults")
            .expect("root should show section heading");
        let search_value = tools_entry.search_value.as_deref().expect("search value");
        assert!(search_value.contains("tools.editor.preferred_editor"));
        assert!(search_value.contains("code --wait"));
    }

    #[test]
    fn root_settings_hide_nested_items_until_section_is_opened() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [tools.editor]
            preferred_editor = "code --wait"
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Tool Defaults"));
        assert!(!items.iter().any(|item| item.title == "Preferred Editor"));
    }

    #[test]
    fn root_settings_do_not_show_missing_nested_optional_fields() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(!items.iter().any(|item| item.title == "Service Tier"));
    }

    #[test]
    fn nested_settings_titles_are_humanized() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [agent]
            default_model = "gpt-5.4"
            [agent.circuit_breaker]
            enabled = true
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Default Model"));
        assert!(items.iter().any(|item| item.title == "Circuit Breaker"));
    }

    #[test]
    fn agent_view_hides_deprecated_autonomous_mode_field() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(!items.iter().any(|item| item.title == "Autonomous Mode"));
    }

    #[test]
    fn provider_openai_view_includes_missing_service_tier_doc_field() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Service Tier"));
    }

    #[test]
    fn render_commented_config_includes_section_heading() {
        let mut config = VTCodeConfig::default();
        config.agent.default_model = "gpt-5.4".to_string();

        let rendered = render_commented_config(&config).expect("config should render");
        assert!(rendered.contains("# Agent Defaults"));
        assert!(rendered.contains("[agent]"));
    }

    #[test]
    fn missing_service_tier_cycle_creates_value() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
        };

        mutate_draft(&mut state, |draft| {
            apply_scalar_operation(
                draft,
                "provider.openai.service_tier",
                ScalarOperation::CycleNext,
            )
        })
        .expect("service tier should be inserted");

        assert_eq!(
            state.draft.provider.openai.service_tier,
            Some(vtcode_config::OpenAIServiceTier::Flex)
        );
    }

    #[test]
    fn service_tier_cycle_advances_from_flex_to_priority() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
        };
        state.draft.provider.openai.service_tier = Some(vtcode_config::OpenAIServiceTier::Flex);

        mutate_draft(&mut state, |draft| {
            apply_scalar_operation(
                draft,
                "provider.openai.service_tier",
                ScalarOperation::CycleNext,
            )
        })
        .expect("service tier should advance");

        assert_eq!(
            state.draft.provider.openai.service_tier,
            Some(vtcode_config::OpenAIServiceTier::Priority)
        );
    }

    #[test]
    fn root_settings_include_ide_context_section() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "IDE Context"));
        assert!(items.iter().any(|item| item.title == "Custom Providers"));
    }

    #[test]
    fn resolve_settings_view_path_maps_model_aliases() {
        assert_eq!(
            resolve_settings_view_path("model"),
            SETTINGS_MODEL_CONFIG_PATH
        );
        assert_eq!(
            resolve_settings_view_path("model.main"),
            SETTINGS_MODEL_CONFIG_MAIN_PATH
        );
        assert_eq!(
            resolve_settings_view_path("model.lightweight"),
            SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH
        );
        assert_eq!(
            resolve_settings_view_path("codex"),
            "agent.codex_app_server"
        );
        assert_eq!(
            resolve_settings_view_path("codex_app_server"),
            "agent.codex_app_server"
        );
    }

    #[test]
    fn root_settings_include_model_config_quick_access() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "Model Config")
            .expect("model config quick access");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                ACTION_PREFIX_OPEN, SETTINGS_MODEL_CONFIG_PATH
            )))
        );
    }

    #[test]
    fn root_settings_include_external_editor_quick_access() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "External Editor")
            .expect("external editor quick access");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_CONFIGURE_EDITOR.to_string()
            ))
        );
    }

    #[test]
    fn root_settings_include_codex_app_server_quick_access() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: None,
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "Codex App Server")
            .expect("codex app server quick access");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(
                "settings:open:agent.codex_app_server".to_string()
            ))
        );
    }

    #[test]
    fn codex_app_server_custom_command_entry_is_cycleable() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("agent.codex_app_server".to_string()),
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [agent.codex_app_server]
            command = "/usr/local/bin/codex"
            args = ["app-server"]
            startup_timeout_secs = 10
            experimental_features = false
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "Command")
            .expect("command entry");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(
                "settings:set:agent.codex_app_server.command:cycle".to_string()
            ))
        );
    }

    #[test]
    fn model_config_root_shows_main_and_lightweight_sections() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_MODEL_CONFIG_PATH.to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Main Model"));
        assert!(items.iter().any(|item| item.title == "Lightweight Model"));
    }

    #[test]
    fn model_config_main_uses_picker_backed_default_model() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_MODEL_CONFIG_MAIN_PATH.to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Provider"));
        let default_model = items
            .iter()
            .find(|item| item.title == "Default Model")
            .expect("default model entry");
        assert_eq!(
            default_model.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_PICK_MAIN_MODEL.to_string()
            ))
        );
    }

    #[test]
    fn model_config_lightweight_shows_full_small_model_block() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH.to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        for title in [
            "Enabled",
            "Model",
            "Temperature",
            "Use For Git History",
            "Use For Large Reads",
            "Use For Web Summary",
            "Use For Memory",
        ] {
            assert!(
                items.iter().any(|item| item.title == title),
                "missing {title}"
            );
        }

        let model = items
            .iter()
            .find(|item| item.title == "Model")
            .expect("lightweight model entry");
        assert_eq!(
            model.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_PICK_LIGHTWEIGHT_MODEL.to_string()
            ))
        );
    }

    #[test]
    fn ide_context_view_includes_provider_section_navigation() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("ide_context.providers".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "VS Code Family"));
        assert!(items.iter().any(|item| item.title == "Zed Family"));
        assert!(items.iter().any(|item| item.title == "Generic Bridge"));
    }

    #[test]
    fn tools_view_routes_external_editor_to_configure_action() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "External Editor")
            .expect("tools.external editor entry");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_CONFIGURE_EDITOR.to_string()
            ))
        );
    }

    #[test]
    fn agent_view_uses_picker_backed_default_model() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let default_model = items
            .iter()
            .find(|item| item.title == "Default Model")
            .expect("default model entry");
        assert_eq!(
            default_model.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_PICK_MAIN_MODEL.to_string()
            ))
        );
    }

    #[test]
    fn lightweight_model_view_uses_picker_backed_model_entry() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("agent.small_model".to_string()),
        };
        let draft =
            TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let model = items
            .iter()
            .find(|item| item.title == "Model")
            .expect("model entry");
        assert_eq!(
            model.selection,
            Some(InlineListSelection::ConfigAction(
                ACTION_PICK_LIGHTWEIGHT_MODEL.to_string()
            ))
        );
        assert!(
            model
                .subtitle
                .as_deref()
                .is_some_and(|subtitle| subtitle.contains("Automatic"))
        );
    }

    #[test]
    fn ide_context_toggle_action_persists_to_disk() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: source_path.clone(),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("ide_context".to_string()),
        };

        apply_settings_action(&mut state, "settings:set:ide_context.enabled:toggle")
            .expect("toggle ide context");

        assert!(!state.draft.ide_context.enabled);
        let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
        assert!(persisted.contains("[ide_context]"));
        assert!(persisted.contains("enabled = false"));
    }

    #[test]
    fn custom_providers_array_add_uses_valid_template() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: source_path.clone(),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("custom_providers".to_string()),
        };

        apply_settings_action(&mut state, "settings:array_add:custom_providers")
            .expect("add custom provider template");

        assert_eq!(state.draft.custom_providers.len(), 1);
        let provider = &state.draft.custom_providers[0];
        assert_eq!(provider.name, "custom-provider-1");
        assert_eq!(provider.display_name, "Custom Provider 1");
        assert_eq!(provider.base_url, "https://llm.example/v1");
        assert_eq!(provider.api_key_env, "");
        assert_eq!(provider.model, "");

        let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
        assert!(persisted.contains("custom_providers"));
    }

    #[test]
    fn settings_palette_state_loads_workspace_config_directly() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut config = VTCodeConfig::default();
        config.agent.theme = "ansi".to_string();
        std::fs::write(
            &source_path,
            toml::to_string(&config).expect("config should serialize"),
        )
        .expect("workspace config should be written");

        let state =
            create_settings_palette_state(temp.path(), &None).expect("settings state should load");

        assert_eq!(
            std::fs::canonicalize(&state.source_path).expect("canonical state source path"),
            std::fs::canonicalize(&source_path).expect("canonical expected source path")
        );
        assert_eq!(state.draft.agent.theme, "ansi");
    }

    #[test]
    fn permission_view_summary_includes_mode_and_rule_counts() {
        let mut config = VTCodeConfig::default();
        config.permissions.default_mode = vtcode_core::config::PermissionMode::DontAsk;
        config.permissions.allow = vec!["Read".to_string()];
        config.permissions.ask = vec!["Bash".to_string(), "Write".to_string()];
        config.permissions.deny = vec!["Edit".to_string()];

        let summary = format_permission_summary(&config);
        assert!(summary.contains("Effective mode: dont_ask"));
        assert!(summary.contains("deny: 1"));
        assert!(summary.contains("ask: 2"));
        assert!(summary.contains("allow: 1"));
    }
}
