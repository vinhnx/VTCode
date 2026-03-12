mod docs;
mod items;
mod mutations;
mod path;
mod render;

use std::path::{Path, PathBuf};

use super::config_section_headings::heading_for_path;
use anyhow::{Context, Result, anyhow, bail};
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineListSearchConfig, InlineListSelection};

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
const SETTINGS_HINT: &str =
    "Enter open/apply • ←/→ adjust • Esc back • Double Esc close • Type to filter";
const SETTINGS_SEARCH_LABEL: &str = "Filter settings";
const SETTINGS_SEARCH_PLACEHOLDER: &str = "section, setting, or value";
const ACTION_RELOAD: &str = "settings:reload";
const ACTION_OPEN_ROOT: &str = "settings:open_root";
const ACTION_PREFIX_OPEN: &str = "settings:open:";
const ACTION_PREFIX_ARRAY_ADD: &str = "settings:array_add:";
const ACTION_PREFIX_ARRAY_POP: &str = "settings:array_pop:";
const ACTION_PREFIX_SET: &str = "settings:set:";
const OPTIONAL_DOC_FIELDS: &[&str] = &["provider.openai.service_tier"];

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
        lines.push(format!("{} ({})", heading.title, view_path));
        if !heading.summary.is_empty() {
            lines.push(heading.summary.into_owned());
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

pub(crate) fn apply_settings_action(
    state: &mut SettingsPaletteState,
    action: &str,
) -> Result<SettingsApplyOutcome> {
    let mut outcome = SettingsApplyOutcome::default();

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
            external_editor = "code --wait"
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
        assert!(search_value.contains("tools.editor.external_editor"));
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
            external_editor = "code --wait"
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Tool Defaults"));
        assert!(!items.iter().any(|item| item.title == "External Editor"));
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
        let rendered =
            render_commented_config(&VTCodeConfig::default()).expect("config should render");
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

        assert_eq!(state.source_path, source_path);
        assert_eq!(state.draft.agent.theme, "ansi");
    }
}
