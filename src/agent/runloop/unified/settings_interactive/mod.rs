mod docs;
mod items;
mod mutations;
mod path;
mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::config_section_headings::{heading_for_path, humanize_identifier};
use anyhow::{Context, Result, anyhow, bail};
use path::get_node;
use render::summarize_value;
use toml::Value as TomlValue;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::{ConfigResetRequest, ConfigService, ConfigWriteTarget};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection};

#[cfg(test)]
use docs::FIELD_DOCS;
use items::build_settings_items;
pub(crate) use mutations::reload_state_from_disk;
use mutations::{
    ScalarOperation, add_array_item, apply_scalar_operation, mutate_draft_and_persist, no_config_source_label,
    pop_array_item,
};
#[cfg(test)]
use mutations::{mutate_draft, render_commented_config};
pub(crate) use path::parent_view_path;
#[cfg(test)]
use path::{PathToken, parse_path_tokens};

const SETTINGS_TITLE: &str = "VT Code Settings";
const SETTINGS_SEARCH_PLACEHOLDER: &str = "group, setting, or value";
const ADVANCED_SEARCH_PLACEHOLDER: &str = "path, label, description, value, or option";
pub(crate) const ACTION_RELOAD: &str = "settings:reload";
pub(crate) const ACTION_OPEN_ROOT: &str = "settings:open_root";
pub(crate) const ACTION_BACK: &str = "settings:back";
pub(crate) const ACTION_RESET: &str = "settings:reset";
pub(crate) const ACTION_RESET_CONFIRM: &str = "settings:reset_confirm";
pub(crate) const ACTION_RESET_CANCEL: &str = "settings:reset_cancel";
const ACTION_PREFIX_OPEN: &str = "settings:open:";
const ACTION_PREFIX_ARRAY_ADD: &str = "settings:array_add:";
const ACTION_PREFIX_ARRAY_POP: &str = "settings:array_pop:";
const ACTION_PREFIX_SET: &str = "settings:set:";
pub(crate) const ACTION_PREFIX_EDIT: &str = "settings:edit:";
const OPTIONAL_DOC_FIELDS: &[&str] = &["provider.anthropic.thinking_display", "provider.openai.service_tier"];
pub(crate) const SETTINGS_MODEL_CONFIG_PATH: &str = "model_config";
pub(crate) const SETTINGS_MODEL_CONFIG_MAIN_PATH: &str = "model_config.main";
pub(crate) const ACTION_PICK_MAIN_MODEL: &str = "settings:pick_main_model";
pub(crate) const ACTION_CONFIGURE_EDITOR: &str = "settings:configure_editor";
pub(crate) const SETTINGS_ADVANCED_VIEW_PATH: &str = "advanced";
pub(crate) const SETTINGS_ADVANCED_NESTED_PREFIX: &str = "advanced:";
pub(crate) const SETTINGS_GROUP_PREFIX: &str = "group:";
const RESET_CONFIRMATION_VIEW: &str = "__settings_reset_confirmation";

#[derive(Clone)]
pub(crate) struct SettingsPaletteState {
    pub(crate) workspace: PathBuf,
    pub(crate) source_path: PathBuf,
    /// Header notice shown only when the config source needs explanation
    /// (e.g. no `vtcode.toml` on disk yet); `None` keeps the header minimal.
    pub(crate) source_label: Option<String>,
    pub(crate) draft: VTCodeConfig,
    pub(crate) view_path: Option<String>,
    /// Last submitted selection in the current settings view.
    pub(crate) last_selection: Option<InlineListSelection>,
    /// Selection memory keyed by view path; the empty key is the root view.
    pub(crate) selection_by_view: BTreeMap<String, InlineListSelection>,
    /// Path currently being edited in the settings value wizard.
    pub(crate) pending_edit_path: Option<String>,
    /// Last apply/save feedback shown as the modal status strip.
    pub(crate) status: Option<vtcode_commons::ui_protocol::InlineStatus>,
}

impl SettingsPaletteState {
    fn view_key(view_path: Option<&str>) -> &str {
        view_path.unwrap_or("")
    }

    pub(crate) fn remember_selection(&mut self, view_path: Option<&str>, selection: InlineListSelection) {
        self.last_selection = Some(selection.clone());
        self.selection_by_view.insert(Self::view_key(view_path).to_string(), selection);
    }

    pub(crate) fn selection_for_view(&self, view_path: Option<&str>) -> Option<InlineListSelection> {
        self.selection_by_view.get(Self::view_key(view_path)).cloned()
    }
}

#[derive(Debug, Default)]
pub(crate) struct SettingsApplyOutcome {
    pub(crate) message: Option<String>,
    pub(crate) saved: bool,
    /// Tone for the in-modal status strip (`None` maps to Success when saved).
    pub(crate) tone: Option<vtcode_commons::ui_protocol::InlineTone>,
}

pub(crate) fn create_settings_palette_state(
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
) -> Result<SettingsPaletteState> {
    let manager = crate::main_helpers::load_workspace_config(workspace)?;
    let has_config_file = manager.config_path().is_some();
    let source_path = manager
        .config_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| workspace.join("vtcode.toml"));

    let draft = if has_config_file {
        manager.config().clone()
    } else {
        vt_snapshot.clone().unwrap_or_else(|| manager.config().clone())
    };

    let source_label = (!has_config_file).then(no_config_source_label);

    Ok(SettingsPaletteState {
        workspace: workspace.to_path_buf(),
        source_path,
        source_label,
        draft,
        view_path: None,
        last_selection: None,
        selection_by_view: BTreeMap::new(),
        pending_edit_path: None,
        status: None,
    })
}

pub(crate) fn begin_string_edit(state: &mut SettingsPaletteState, path: &str) -> Result<String> {
    let draft_value = TomlValue::try_from(state.draft.clone()).context("Failed to serialize draft configuration")?;
    let current = get_node(&draft_value, path)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
        .to_string();
    state.pending_edit_path = Some(path.to_string());
    Ok(current)
}

pub(crate) fn apply_string_edit(
    state: &mut SettingsPaletteState,
    path: &str,
    value: String,
) -> Result<SettingsApplyOutcome> {
    mutate_draft_and_persist(state, path, |draft| path::set_node(draft, path, TomlValue::String(value)))?;

    Ok(SettingsApplyOutcome {
        message: Some(format!("Updated {}.", change_title(path))),
        saved: true,
        tone: Some(vtcode_commons::ui_protocol::InlineTone::Success),
    })
}

pub(crate) fn show_settings_palette(
    renderer: &mut AnsiRenderer,
    state: &SettingsPaletteState,
    selected: Option<InlineListSelection>,
) -> Result<bool> {
    let draft_value = TomlValue::try_from(state.draft.clone()).context("Failed to serialize draft configuration")?;

    let lines = settings_header_lines(state);

    let items = build_settings_items(state, &draft_value)?;
    if items.is_empty() {
        return Ok(false);
    }

    let selected = preferred_settings_selection(state, &items, selected);
    // ConfigAction lists are FixedComfortable and render the shared
    // navigation hint; an explicit footer is ignored.
    renderer.show_list_modal_with_status(
        SETTINGS_TITLE,
        lines,
        items,
        selected,
        Some(settings_search_config(state)),
        None,
        state.status.clone(),
    );

    Ok(true)
}

/// Build the search box for the settings modal. Fuzzy matching is enabled so
/// group titles and child settings surface on partial or out-of-order input.
fn settings_search_config(state: &SettingsPaletteState) -> InlineListSearchConfig {
    let advanced_view = state.view_path.as_deref() == Some(SETTINGS_ADVANCED_VIEW_PATH)
        || state.view_path.as_deref().is_some_and(|path| path.starts_with("advanced."))
        || state
            .view_path
            .as_deref()
            .is_some_and(|path| path.starts_with(SETTINGS_ADVANCED_NESTED_PREFIX));
    InlineListSearchConfig {
        label: String::new(),
        placeholder: Some(
            if advanced_view {
                ADVANCED_SEARCH_PLACEHOLDER
            } else {
                SETTINGS_SEARCH_PLACEHOLDER
            }
            .to_string(),
        ),
        fuzzy: true,
    }
}

fn preferred_settings_selection(
    state: &SettingsPaletteState,
    items: &[InlineListItem],
    selected: Option<InlineListSelection>,
) -> Option<InlineListSelection> {
    let selected = if state.view_path.as_deref() == Some(RESET_CONFIRMATION_VIEW) {
        // Safer default: Keep settings is preselected on the reset confirm view.
        Some(InlineListSelection::ConfigAction(ACTION_RESET_CANCEL.to_string()))
    } else if matches!(
        selected.as_ref(),
        Some(InlineListSelection::ConfigAction(action)) if action == ACTION_BACK
    ) {
        None
    } else {
        selected
    };
    selected
        .into_iter()
        .chain(state.selection_for_view(state.view_path.as_deref()))
        .find(|candidate| items.iter().any(|item| item.selection.as_ref() == Some(candidate)))
        .or_else(|| {
            items.iter().find_map(|item| {
                let InlineListSelection::ConfigAction(action) = item.selection.as_ref()? else {
                    return item.selection.clone();
                };
                (!matches!(
                    action.as_str(),
                    ACTION_BACK
                        | ACTION_OPEN_ROOT
                        | ACTION_RELOAD
                        | ACTION_RESET
                        | ACTION_RESET_CANCEL
                        | ACTION_RESET_CONFIRM
                ))
                .then(|| item.selection.clone())
                .flatten()
            })
        })
        .or_else(|| items.iter().find_map(|item| item.selection.clone()))
}

fn format_permission_summary(config: &VTCodeConfig) -> String {
    format!(
        "Rules • deny {} • ask {} • allow {}.",
        config.permissions.deny.len(),
        config.permissions.ask.len(),
        config.permissions.allow.len()
    )
}

/// Render `path` compactly for the settings header: relative to the workspace
/// when inside it, else tilde-abbreviated under the home directory, else as-is.
fn short_display_path(path: &Path, workspace: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(workspace) {
        if !relative.as_os_str().is_empty() {
            return relative.display().to_string();
        }
    }
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(&home) {
            if relative.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}

fn settings_header_lines(state: &SettingsPaletteState) -> Vec<String> {
    let write_target = format!("Writing to {}", short_display_path(&state.source_path, &state.workspace));

    if state.view_path.as_deref() == Some(RESET_CONFIRMATION_VIEW) {
        let mut lines = vec![
            "Settings › Reset.".to_string(),
            "Clears every setting in this file. Credentials are kept.".to_string(),
            write_target,
        ];
        if let Some(label) = state.source_label.as_deref() {
            lines.push(label.to_string());
        }
        return lines;
    }
    if let Some(view_path) = state.view_path.as_deref() {
        let (breadcrumb, mut detail) = settings_breadcrumb_and_detail(view_path);
        let mut lines = vec![format!("{breadcrumb}.")];
        if view_path == "permissions" {
            let counts = format_permission_summary(&state.draft);
            if detail.is_empty() {
                detail = counts;
            } else {
                detail.push(' ');
                detail.push_str(&counts);
            }
        }
        if !detail.is_empty() {
            lines.push(detail);
        }
        lines.push(write_target);
        if let Some(label) = state.source_label.as_deref() {
            lines.push(label.to_string());
        }
        return lines;
    }
    let mut lines = vec!["Pick a group to edit.".to_string(), write_target];
    if let Some(label) = state.source_label.as_deref() {
        lines.push(label.to_string());
    }
    lines
}

fn settings_breadcrumb_and_detail(view_path: &str) -> (String, String) {
    if view_path == SETTINGS_ADVANCED_VIEW_PATH {
        return (
            "Settings › Advanced settings".to_string(),
            "Search the complete configuration by path, label, description, current value, or option.".to_string(),
        );
    }
    if let Some(path) = view_path.strip_prefix(SETTINGS_ADVANCED_NESTED_PREFIX) {
        return (format!("Settings › Advanced settings › {path}"), "Editing a nested advanced setting.".to_string());
    }
    if let Some(path) = view_path.strip_prefix("advanced.") {
        return (
            format!("Settings › Advanced settings › {path}"),
            "Editing a documented advanced setting.".to_string(),
        );
    }
    if let Some(group_id) = view_path.strip_prefix(SETTINGS_GROUP_PREFIX) {
        if let Some((group_id, nested_path)) = group_id.split_once(':') {
            let group = items::curated_group(group_id);
            return match group {
                Some(group) => (
                    format!("Settings › {} › {nested_path}", group.title),
                    "Editing a setting in this group.".to_string(),
                ),
                None => (format!("Settings › {} › {nested_path}", humanize_identifier(group_id)), String::new()),
            };
        }
        let group = items::curated_group(group_id);
        return match group {
            Some(group) => (format!("Settings › {}", group.title), group.description.to_string()),
            None => (format!("Settings › {}", humanize_identifier(group_id)), String::new()),
        };
    }

    let heading = heading_for_path(view_path);
    (format!("Settings › {}", heading.title), heading.summary.into_owned())
}

pub(crate) fn apply_settings_action(state: &mut SettingsPaletteState, action: &str) -> Result<SettingsApplyOutcome> {
    let mut outcome = SettingsApplyOutcome::default();

    if matches!(action, ACTION_PICK_MAIN_MODEL | ACTION_CONFIGURE_EDITOR) {
        return Ok(outcome);
    }

    match action {
        ACTION_RELOAD => {
            reload_state_from_disk(state)?;
            outcome.message = Some("Reloaded settings from disk.".to_string());
            outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Success);
            return Ok(outcome);
        }
        ACTION_RESET => {
            state.view_path = Some(RESET_CONFIRMATION_VIEW.to_string());
            outcome.message = Some(
                "Confirm reset: this clears every setting in the write target. Credentials are preserved.".to_string(),
            );
            outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Warning);
            return Ok(outcome);
        }
        ACTION_RESET_CANCEL => {
            state.view_path = None;
            outcome.message = Some("Configuration reset cancelled.".to_string());
            outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Accent);
            return Ok(outcome);
        }
        ACTION_RESET_CONFIRM => {
            let response = ConfigService::reset(ConfigResetRequest {
                workspace: state.workspace.clone(),
                target: ConfigWriteTarget::Workspace,
                expected_layer_version: None,
                path: Some(state.source_path.clone()),
            })?;
            state.draft = serde_json::from_value(response.effective_config)
                .context("Reset configuration could not be converted to the effective settings")?;
            state.view_path = None;
            state.source_label = None;
            outcome.saved = true;
            outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Success);
            outcome.message =
                Some(format!("Reset configuration at {}.", short_display_path(&response.path, &state.workspace)));
            return Ok(outcome);
        }
        ACTION_OPEN_ROOT => {
            state.view_path = None;
            return Ok(outcome);
        }
        ACTION_BACK => {
            let parent = state.view_path.as_deref().and_then(parent_view_path);
            state.view_path = parent;
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
        mutate_draft_and_persist(state, path, |draft| add_array_item(draft, path))?;
        outcome.saved = true;
        outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Success);
        outcome.message = Some(describe_array_change(path, true));
        return Ok(outcome);
    }

    if let Some(path) = action.strip_prefix(ACTION_PREFIX_ARRAY_POP) {
        mutate_draft_and_persist(state, path, |draft| pop_array_item(draft, path))?;
        outcome.saved = true;
        outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Success);
        outcome.message = Some(describe_array_change(path, false));
        return Ok(outcome);
    }

    if let Some(rest) = action.strip_prefix(ACTION_PREFIX_SET) {
        let (path, op) = rest
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("Invalid settings action: {action}"))?;

        let operation = match op {
            "toggle" => ScalarOperation::Toggle,
            "inc" => ScalarOperation::Increment,
            "dec" => ScalarOperation::Decrement,
            "cycle" => ScalarOperation::CycleNext,
            "cycle_prev" => ScalarOperation::CyclePrev,
            _ => bail!("Unsupported settings operation: {op}"),
        };

        mutate_draft_and_persist(state, path, |draft| apply_scalar_operation(draft, path, operation))?;
        outcome.saved = true;
        outcome.tone = Some(vtcode_commons::ui_protocol::InlineTone::Success);
        outcome.message = Some(describe_scalar_change(state, path, operation));
        return Ok(outcome);
    }

    bail!("Unknown settings action: {action}")
}

pub(crate) fn resolve_settings_view_path(path: &str) -> String {
    match path.trim() {
        "model" => SETTINGS_MODEL_CONFIG_PATH.to_string(),
        "model.main" => SETTINGS_MODEL_CONFIG_MAIN_PATH.to_string(),
        "codex" | "codex_app_server" | "codex.app_server" | "app_server" => "agent.codex_app_server".to_string(),
        "advanced" => SETTINGS_ADVANCED_VIEW_PATH.to_string(),
        path if path.starts_with("advanced.") => path.to_string(),
        other if items::curated_group(other).is_some() => format!("{SETTINGS_GROUP_PREFIX}{other}"),
        other => other.to_string(),
    }
}

/// Human-readable label for a settings path, used in change feedback messages.
///
/// For generic boolean leaf names (`enabled`, `show`, ...) the parent section is
/// used instead so feedback reads naturally (e.g. "Disabled Todo Planning Mode" rather
/// than "Disabled Enabled").
fn change_title(path: &str) -> String {
    let last_segment = path
        .rsplit('.')
        .next()
        .unwrap_or(path)
        .trim_end_matches(|c: char| c.is_numeric() || c == ']' || c == '[');
    let leaf = humanize_identifier(last_segment);

    const GENERIC_BOOL_LEAVES: &[&str] = &["Enabled", "Disabled", "Show", "Hide", "On", "Off"];
    if GENERIC_BOOL_LEAVES.contains(&leaf.as_str()) {
        if let Some(parent) = path.rsplit_once('.') {
            let parent_leaf = parent
                .0
                .rsplit('.')
                .next()
                .unwrap_or(parent.0)
                .trim_end_matches(|c: char| c.is_numeric() || c == ']' || c == '[');
            return humanize_identifier(parent_leaf);
        }
    }

    leaf
}

/// Builds a status-line message describing a scalar config change.
fn describe_scalar_change(state: &SettingsPaletteState, path: &str, operation: ScalarOperation) -> String {
    let title = change_title(path);
    let draft_value = TomlValue::try_from(state.draft.clone()).ok();
    let value = draft_value.as_ref().and_then(|value| get_node(value, path));

    match (operation, value) {
        (ScalarOperation::Toggle, Some(TomlValue::Boolean(enabled))) => {
            format!("{} {}", if *enabled { "Enabled" } else { "Disabled" }, title)
        }
        (_, Some(value)) => format!("{} → {}", title, summarize_value(value)),
        _ => format!("Updated {}", title),
    }
}

/// Builds a status-line message describing an array add/remove change.
fn describe_array_change(path: &str, added: bool) -> String {
    let title = change_title(path);
    if added {
        format!("Added item to {}", title)
    } else {
        format!("Removed last item from {}", title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::config_section_headings::normalize_config_path;
    use serial_test::serial;
    use std::sync::Arc;
    use vtcode_commons::canonicalize;
    use vtcode_commons::reference::StaticWorkspacePaths;
    use vtcode_config::defaults::WorkspacePathsDefaults;
    use vtcode_config::defaults::provider::with_config_defaults_provider_for_test;
    use vtcode_core::config::ConfigManager;

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
        assert_eq!(normalize_config_path("commands.allow_list[12]"), "commands.allow_list[]");
    }

    #[test]
    fn parent_view_path_handles_nested_segments() {
        assert_eq!(parent_view_path("agent"), None);
        assert_eq!(parent_view_path("agent.vibe_coding"), Some("agent".to_string()));
        assert_eq!(parent_view_path("group:model_provider:custom_providers"), Some("group:model_provider".to_string()));
        assert_eq!(
            parent_view_path("group:model_provider:custom_providers[0]"),
            Some("group:model_provider:custom_providers".to_string())
        );
        assert_eq!(parent_view_path("advanced:custom_providers[0]"), Some("advanced:custom_providers".to_string()));
        assert_eq!(
            parent_view_path("hooks.lifecycle.pre_tool_use[0].hooks[2]"),
            Some("hooks.lifecycle.pre_tool_use[0].hooks".to_string())
        );
    }

    #[test]
    fn settings_selection_memory_restores_parent_and_falls_back_when_removed() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let root_selection = InlineListSelection::ConfigAction("settings:open:group:agent_automation".to_string());
        let nested_selection =
            InlineListSelection::ConfigAction("settings:set:agent.todo_planning_mode:toggle".to_string());

        state.remember_selection(None, root_selection.clone());
        let group_path = format!("{SETTINGS_GROUP_PREFIX}agent_automation");
        state.view_path = Some(group_path.clone());
        state.remember_selection(Some(&group_path), nested_selection.clone());

        assert_eq!(state.selection_for_view(Some(&group_path)), Some(nested_selection));
        assert_eq!(state.selection_for_view(None), Some(root_selection));
        assert_eq!(
            state.last_selection,
            Some(InlineListSelection::ConfigAction("settings:set:agent.todo_planning_mode:toggle".to_string(),))
        );

        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");
        let items = build_settings_items(&state, &draft).expect("settings items");
        let removed_selection = InlineListSelection::ConfigAction("settings:set:agent.removed:cycle".to_string());
        state.selection_by_view.remove(&group_path);
        let fallback = preferred_settings_selection(&state, &items, Some(removed_selection));
        let first_setting = items.iter().find_map(|item| {
            let InlineListSelection::ConfigAction(action) = item.selection.as_ref()? else {
                return item.selection.clone();
            };
            (!matches!(action.as_str(), ACTION_BACK | ACTION_OPEN_ROOT | ACTION_RELOAD))
                .then(|| item.selection.clone())
                .flatten()
        });
        assert_eq!(fallback, first_setting);
        let back_fallback = preferred_settings_selection(
            &state,
            &items,
            Some(InlineListSelection::ConfigAction(ACTION_BACK.to_string())),
        );
        assert_eq!(back_fallback, first_setting);
    }

    #[test]
    fn reset_confirmation_lists_only_confirm_and_cancel_actions() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(RESET_CONFIRMATION_VIEW.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("reset confirmation items");
        let actions = items.iter().filter_map(|item| item.selection.as_ref()).collect::<Vec<_>>();
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&&InlineListSelection::ConfigAction(ACTION_RESET_CONFIRM.to_string())));
        assert!(actions.contains(&&InlineListSelection::ConfigAction(ACTION_RESET_CANCEL.to_string())));
    }

    #[test]
    fn parse_field_docs_has_known_entry() {
        assert!(FIELD_DOCS.lookup("agent.provider").is_some());
        assert!(FIELD_DOCS.lookup("provider_overrides.openai.base_url").is_some());
        assert!(
            FIELD_DOCS
                .lookup("custom_providers[0].profiles.gpt-5-mini.temperature")
                .is_some()
        );
    }

    #[test]
    fn tool_display_mode_is_exposed_as_a_cycle_setting() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("ui".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let item = items
            .iter()
            .find(|item| item.title == "Tool Display Mode")
            .expect("tool display mode entry");
        assert_eq!(
            item.selection,
            Some(InlineListSelection::ConfigAction("settings:set:ui.tool_display_mode:cycle".to_string()))
        );
        assert_eq!(item.badge.as_deref(), None, "setting rows carry value, not action badges");
        assert!(
            item.value
                .as_deref()
                .is_some_and(|value| value.contains("compact") || value.contains("expanded")),
            "live value should render in the value slot: {:?}",
            item.value
        );
    }

    #[test]
    fn tool_display_mode_cycle_persists_to_disk() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: source_path.clone(),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("ui".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };

        let outcome = apply_settings_action(&mut state, "settings:set:ui.tool_display_mode:cycle")
            .expect("cycle tool display mode");

        assert!(outcome.saved);
        // The default is compact; cycling advances to expanded.
        assert_eq!(state.draft.ui.tool_display_mode, vtcode_core::config::ToolDisplayMode::Expanded);
        let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
        assert!(persisted.contains("tool_display_mode = \"expanded\""));
    }

    #[test]
    fn interface_terminal_group_exposes_copy_on_select_toggle() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(format!("{SETTINGS_GROUP_PREFIX}interface_terminal")),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let item = items
            .iter()
            .find(|item| {
                item.selection.as_ref().is_some_and(|selection| {
                    matches!(selection, InlineListSelection::ConfigAction(action)
                        if action == "settings:set:ui.fullscreen.copy_on_select:toggle")
                })
            })
            .expect("copy on select entry in Interface & Terminal");
        assert_eq!(item.badge.as_deref(), None, "boolean state lives in the value slot");
        assert_eq!(item.kind, vtcode_commons::ui_protocol::InlineItemKind::Setting);
    }

    #[test]
    fn copy_on_select_toggle_persists_manual_mode_to_disk() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: source_path.clone(),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(format!("{SETTINGS_GROUP_PREFIX}interface_terminal")),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        assert!(state.draft.ui.fullscreen.copy_on_select);

        let outcome = apply_settings_action(&mut state, "settings:set:ui.fullscreen.copy_on_select:toggle")
            .expect("toggle copy on select");

        assert!(outcome.saved);
        assert!(!state.draft.ui.fullscreen.copy_on_select);
        let persisted = std::fs::read_to_string(&source_path).expect("persisted config");
        assert!(persisted.contains("copy_on_select = false"));
    }

    #[test]
    #[serial]
    fn settings_palette_uses_explicit_session_override_as_source() {
        use vtcode_config::loader::set_explicit_config_path;

        struct OverrideGuard;

        impl OverrideGuard {
            fn set(path: Option<PathBuf>) -> Self {
                set_explicit_config_path(path);
                Self
            }
        }

        impl Drop for OverrideGuard {
            fn drop(&mut self) {
                set_explicit_config_path(None);
            }
        }

        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        std::fs::write(workspace.join("vtcode.toml"), "agent.provider = \"anthropic\"\n").expect("workspace config");

        let override_path = temp.path().join("custom-night.toml");
        std::fs::write(&override_path, "agent.provider = \"openai\"\n").expect("override config");

        let _guard = OverrideGuard::set(Some(override_path.clone()));
        let state = create_settings_palette_state(&workspace, &None);

        let state = state.expect("settings state with override");
        assert_eq!(
            canonicalize(&state.source_path).expect("canonical source path"),
            canonicalize(&override_path).expect("canonical override path"),
            "settings palette must treat the explicit override file as its source"
        );
        assert_eq!(state.draft.agent.provider, "openai");
    }

    #[test]
    fn root_settings_contains_only_curated_groups_and_global_actions() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let titles: Vec<_> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Model & Provider",
                "Agent & Automation",
                "Approvals & Security",
                "Tools & Integrations",
                "Context & Memory",
                "Interface & Terminal",
                "Performance & Diagnostics",
                "Advanced settings",
                "Actions",
                "Reload configuration",
                "Reset configuration",
            ]
        );
        assert!(items.iter().all(|item| item.selection.is_some() || item.title == "Actions"));
        assert!(items.iter().all(|item| {
            item.title == "Actions"
                || item
                    .subtitle
                    .as_deref()
                    .is_some_and(|subtitle| subtitle.contains("editable setting"))
                || matches!(item.title.as_str(), "Advanced settings" | "Reload configuration" | "Reset configuration")
        }));
    }

    #[test]
    fn root_settings_remove_quick_access_duplicates_and_nested_fields() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [tools.editor]
            preferred_editor = "code --wait"
            "#,
        )
        .expect("valid draft value");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Tools & Integrations"));
        for duplicate in [
            "Quick Access",
            "Model Config",
            "External Editor",
            "Editor Mode",
            "Codex App Server",
        ] {
            assert!(!items.iter().any(|item| item.title == duplicate), "unexpected root item {duplicate}");
        }
        assert!(!items.iter().any(|item| item.title == "Preferred Editor"));
        let tools = items
            .iter()
            .find(|item| item.title == "Tools & Integrations")
            .expect("tools group");
        assert!(
            !tools
                .search_value
                .as_deref()
                .is_some_and(|search| search.contains("preferred_editor"))
        );
    }

    #[test]
    fn root_settings_do_not_show_missing_nested_optional_fields() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(!items.iter().any(|item| item.title == "Service Tier"));
    }

    #[test]
    fn nested_settings_titles_are_humanized() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [agent]
            default_model = "gpt-5.6-sol"
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
    fn agent_view_hides_deprecated_auto_permissions_field() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(!items.iter().any(|item| item.title == "Autonomous Execution"));
    }

    #[test]
    fn provider_openai_view_includes_missing_service_tier_doc_field() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Service Tier"));
    }

    #[test]
    fn render_commented_config_includes_section_heading() {
        let mut config = VTCodeConfig::default();
        config.agent.default_model = "gpt-5.6-sol".to_string();

        let rendered = render_commented_config(&config).expect("config should render");
        assert!(rendered.contains("# Agent Defaults"));
        assert!(rendered.contains("[agent]"));
    }

    #[test]
    fn render_commented_config_quotes_dynamic_table_keys() {
        let mut config = VTCodeConfig::default();
        config.mcp.allowlist.providers.insert(
            "provider.with.dot".to_string(),
            vtcode_config::mcp::McpAllowListRules {
                tools: Some(vec!["*".to_string()]),
                ..Default::default()
            },
        );

        let rendered = render_commented_config(&config).expect("config should render");
        assert!(rendered.contains("[mcp.allowlist.providers.\"provider.with.dot\"]"));

        let reparsed: VTCodeConfig = toml::from_str(&rendered).expect("rendered config should remain valid TOML");
        assert!(reparsed.mcp.allowlist.providers.contains_key("provider.with.dot"));
    }

    #[test]
    fn missing_service_tier_cycle_creates_value() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };

        mutate_draft(&mut state, |draft| {
            apply_scalar_operation(draft, "provider.openai.service_tier", ScalarOperation::CycleNext)
        })
        .expect("service tier should be inserted");

        assert_eq!(state.draft.provider.openai.service_tier, Some(vtcode_config::OpenAIServiceTier::Flex));
    }

    #[test]
    fn service_tier_cycle_advances_from_flex_to_priority() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        state.draft.provider.openai.service_tier = Some(vtcode_config::OpenAIServiceTier::Flex);

        mutate_draft(&mut state, |draft| {
            apply_scalar_operation(draft, "provider.openai.service_tier", ScalarOperation::CycleNext)
        })
        .expect("service tier should advance");

        assert_eq!(state.draft.provider.openai.service_tier, Some(vtcode_config::OpenAIServiceTier::Priority));
    }

    #[test]
    fn service_tier_cycle_advances_from_priority_to_ultrafast() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("provider.openai".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        state.draft.provider.openai.service_tier = Some(vtcode_config::OpenAIServiceTier::Priority);

        mutate_draft(&mut state, |draft| {
            apply_scalar_operation(draft, "provider.openai.service_tier", ScalarOperation::CycleNext)
        })
        .expect("service tier should advance");

        assert_eq!(state.draft.provider.openai.service_tier, Some(vtcode_config::OpenAIServiceTier::Ultrafast));
    }

    #[test]
    fn curated_groups_resolve_to_their_declared_field_paths() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        for group in items::curated_groups() {
            let group_state = SettingsPaletteState {
                view_path: Some(format!("{SETTINGS_GROUP_PREFIX}{}", group.id)),
                ..state.clone()
            };
            let items = build_settings_items(&group_state, &draft).expect("curated group items");
            let editable = items.iter().filter(|item| item.selection.is_some()).count();
            assert!(editable > 0, "{} should expose editable settings", group.title);
            for path in group.paths {
                assert!(
                    get_node(&draft, path).is_some()
                        || FIELD_DOCS.lookup(path).is_some_and(|doc| !doc.options.is_empty()),
                    "curated path {path} should resolve or have documented options"
                );
            }
        }
    }

    #[test]
    fn resolve_settings_view_path_maps_model_aliases() {
        assert_eq!(resolve_settings_view_path("model"), SETTINGS_MODEL_CONFIG_PATH);
        assert_eq!(resolve_settings_view_path("model.main"), SETTINGS_MODEL_CONFIG_MAIN_PATH);
        assert_eq!(resolve_settings_view_path("codex"), "agent.codex_app_server");
        assert_eq!(resolve_settings_view_path("codex_app_server"), "agent.codex_app_server");
        assert_eq!(resolve_settings_view_path("advanced"), SETTINGS_ADVANCED_VIEW_PATH);
        for group in items::curated_groups() {
            assert_eq!(resolve_settings_view_path(group.id), format!("{SETTINGS_GROUP_PREFIX}{}", group.id));
        }
        assert_eq!(
            resolve_settings_view_path("advanced.agent.harness.max_tool_calls_per_turn"),
            "advanced.agent.harness.max_tool_calls_per_turn"
        );
    }

    #[test]
    fn advanced_view_indexes_every_documented_field_for_search() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Max Tool Calls Per Turn"));
        let provider_search = items
            .iter()
            .find(|item| {
                item.search_value
                    .as_deref()
                    .is_some_and(|search| search.contains("agent.provider"))
            })
            .and_then(|item| item.search_value.as_deref())
            .expect("provider search entry");
        assert!(provider_search.contains(&VTCodeConfig::default().agent.provider.to_ascii_lowercase()));
        let tool_limit_search = items
            .iter()
            .find(|item| item.title == "Max Tool Calls Per Turn")
            .and_then(|item| item.search_value.as_deref())
            .expect("tool limit search entry");
        assert!(tool_limit_search.contains("maximum number of tool calls allowed per turn"));
        assert!(items.iter().any(|item| {
            item.search_value
                .as_deref()
                .is_some_and(|search| search.contains("flex") && search.contains("priority"))
        }));
        for path in FIELD_DOCS.sorted_paths() {
            let needle = path.to_ascii_lowercase();
            assert!(
                items
                    .iter()
                    .any(|item| item.search_value.as_deref().is_some_and(|search| search.contains(&needle))),
                "advanced search index is missing {path}"
            );
        }
    }

    #[test]
    fn advanced_view_keeps_uninstantiated_wildcard_fields_read_only() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("advanced settings items");
        let wildcard = items
            .iter()
            .find(|item| {
                item.search_value
                    .as_deref()
                    .is_some_and(|search| search.contains("custom_providers[].api_key_env"))
            })
            .expect("wildcard provider field should remain searchable");
        assert_eq!(wildcard.badge.as_deref(), Some("Schema"));
        assert!(wildcard.selection.is_none());
    }

    #[test]
    fn advanced_direct_path_opens_the_specific_nested_field() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("advanced.agent.harness.max_tool_calls_per_turn".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "Max Tool Calls Per Turn")
            .expect("advanced direct field");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction(
                "settings:set:agent.harness.max_tool_calls_per_turn:inc".to_string()
            ))
        );
    }

    #[test]
    fn curated_container_navigation_returns_to_its_group() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(format!("{SETTINGS_GROUP_PREFIX}model_provider")),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("curated group items");
        let providers = items
            .iter()
            .find(|item| item.title == "Custom Providers")
            .expect("custom providers entry");
        let action = providers.selection.clone().expect("custom providers should be navigable");
        assert_eq!(
            action,
            InlineListSelection::ConfigAction("settings:open:group:model_provider:custom_providers".to_string())
        );

        let InlineListSelection::ConfigAction(action) = &action else {
            panic!("expected a config action");
        };
        apply_settings_action(&mut state, action).expect("open custom providers");
        assert_eq!(state.view_path.as_deref(), Some("group:model_provider:custom_providers"));
        apply_settings_action(&mut state, ACTION_BACK).expect("back to model provider group");
        assert_eq!(state.view_path.as_deref(), Some("group:model_provider"));
    }

    #[test]
    fn advanced_container_navigation_returns_to_advanced_search() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(state.draft.clone()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("advanced settings");
        let providers = items
            .iter()
            .find(|item| {
                item.search_value
                    .as_deref()
                    .is_some_and(|search| search.contains("custom_providers"))
                    && item.selection
                        == Some(InlineListSelection::ConfigAction(
                            "settings:open:advanced.custom_providers".to_string(),
                        ))
            })
            .expect("advanced custom providers entry");
        let action = providers.selection.as_ref().expect("entry action");
        let InlineListSelection::ConfigAction(action) = action else {
            panic!("expected a config action");
        };
        apply_settings_action(&mut state, action).expect("open advanced custom providers");
        assert_eq!(state.view_path.as_deref(), Some("advanced.custom_providers"));
        apply_settings_action(&mut state, ACTION_BACK).expect("back to advanced search");
        assert_eq!(state.view_path.as_deref(), Some(SETTINGS_ADVANCED_VIEW_PATH));
    }

    #[test]
    fn advanced_view_handles_dotted_dynamic_map_keys() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_ADVANCED_VIEW_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft: TomlValue = toml::from_str(
            r#"
            [[custom_providers]]
            name = "mycorp"
            display_name = "MyCorp"
            base_url = "https://llm.corp.example/v1"

            [custom_providers.profiles."gpt-5.4"]
            temperature = 0.2
            "#,
        )
        .expect("valid custom provider profile");

        let items = build_settings_items(&state, &draft).expect("advanced settings with dynamic map keys");
        assert!(items.iter().any(|item| {
            item.subtitle
                .as_deref()
                .is_some_and(|subtitle| subtitle.contains(r#"custom_providers[0].profiles["gpt-5.4"].temperature"#))
        }));
    }

    #[test]
    fn curated_group_navigation_preserves_breadcrumb_parent() {
        let mut state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(format!("{SETTINGS_GROUP_PREFIX}tools_integrations")),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "External Editor"));
        assert_eq!(parent_view_path(&format!("{SETTINGS_GROUP_PREFIX}tools_integrations")), None);
        assert_eq!(parent_view_path("advanced.agent.harness.max_tool_calls_per_turn"), Some("advanced".to_string()));
        assert_eq!(parent_view_path("advanced:custom_providers[0]"), Some("advanced:custom_providers".to_string()));

        apply_settings_action(&mut state, ACTION_BACK).expect("group back action");
        assert_eq!(state.view_path, None);
        state.view_path = Some("advanced.agent.harness.max_tool_calls_per_turn".to_string());
        apply_settings_action(&mut state, ACTION_BACK).expect("advanced back action");
        assert_eq!(state.view_path.as_deref(), Some(SETTINGS_ADVANCED_VIEW_PATH));
    }

    #[test]
    fn codex_app_server_custom_command_entry_is_cycleable() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("agent.codex_app_server".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
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
        let entry = items.iter().find(|item| item.title == "Command").expect("command entry");
        assert_eq!(
            entry.selection,
            Some(InlineListSelection::ConfigAction("settings:set:agent.codex_app_server.command:cycle".to_string()))
        );
    }

    #[test]
    fn model_config_root_shows_main_section() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_MODEL_CONFIG_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Main Model"));
        assert!(!items.iter().any(|item| item.title == "Lightweight Model"));
    }

    #[test]
    fn model_config_main_uses_picker_backed_default_model() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some(SETTINGS_MODEL_CONFIG_MAIN_PATH.to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        assert!(items.iter().any(|item| item.title == "Provider"));
        let default_model = items
            .iter()
            .find(|item| item.title == "Default Model")
            .expect("default model entry");
        assert_eq!(
            default_model.selection,
            Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string()))
        );
    }

    #[test]
    fn tools_view_routes_external_editor_to_configure_action() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let entry = items
            .iter()
            .find(|item| item.title == "External Editor")
            .expect("tools.external editor entry");
        assert_eq!(entry.selection, Some(InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string())));
    }

    #[test]
    fn agent_view_uses_picker_backed_default_model() {
        let state = SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("agent".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };
        let draft = TomlValue::try_from(VTCodeConfig::default()).expect("default config should serialize");

        let items = build_settings_items(&state, &draft).expect("settings items");
        let default_model = items
            .iter()
            .find(|item| item.title == "Default Model")
            .expect("default model entry");
        assert_eq!(
            default_model.selection,
            Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string()))
        );
    }

    #[test]
    #[serial]
    fn custom_providers_array_add_uses_valid_template() {
        let temp = tempfile::tempdir().expect("temp dir");
        let user_path = temp.path().join("home").join("vtcode.toml");
        std::fs::create_dir_all(user_path.parent().expect("user config parent")).expect("user config dir");
        let paths = StaticWorkspacePaths::new(temp.path(), temp.path().join(".vtcode"));
        let provider = WorkspacePathsDefaults::new(Arc::new(paths))
            .with_home_paths(vec![user_path.clone()])
            .with_system_config_paths(Vec::new());

        with_config_defaults_provider_for_test(Arc::new(provider), || {
            let mut state = SettingsPaletteState {
                workspace: temp.path().to_path_buf(),
                source_path: temp.path().join("vtcode.toml"),
                source_label: None,
                draft: VTCodeConfig::default(),
                view_path: Some("custom_providers".to_string()),
                last_selection: None,
                selection_by_view: BTreeMap::new(),
                pending_edit_path: None,
                status: None,
            };

            let outcome = apply_settings_action(&mut state, "settings:array_add:custom_providers")
                .expect("add custom provider template");

            assert_eq!(outcome.message.as_deref(), Some("Added item to Custom Providers"));
            assert!(outcome.saved);

            assert_eq!(state.draft.custom_providers.len(), 1);
            let provider = &state.draft.custom_providers[0];
            assert_eq!(provider.name, "custom-provider-1");
            assert_eq!(provider.display_name, "Custom Provider 1");
            assert_eq!(provider.base_url, "https://llm.example/v1");
            assert_eq!(provider.api_key_env, "");
            assert_eq!(provider.model, "");

            state.view_path = Some("custom_providers[0]".to_string());
            let draft = TomlValue::try_from(state.draft.clone()).expect("draft should serialize");
            let items = build_settings_items(&state, &draft).expect("custom provider fields should render");
            let base_url = items
                .iter()
                .find(|item| item.title == "Base Url")
                .expect("base URL field should be visible");
            assert_eq!(
                base_url.selection,
                Some(InlineListSelection::ConfigAction("settings:edit:custom_providers[0].base_url".to_string()))
            );

            apply_string_edit(&mut state, "custom_providers[0].base_url", "https://gateway.example/v1".to_string())
                .expect("custom provider base URL should be editable");
            assert_eq!(state.draft.custom_providers[0].base_url, "https://gateway.example/v1");
            let persisted = std::fs::read_to_string(&user_path).expect("updated provider config");
            assert!(persisted.contains("https://gateway.example/v1"));

            assert!(persisted.contains("custom_providers"));
        });
    }

    #[test]
    #[serial]
    fn settings_edit_does_not_copy_trusted_provider_into_workspace_config() {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path();
        let user_path = workspace.join("home").join("vtcode.toml");
        let workspace_path = workspace.join("vtcode.toml");
        std::fs::create_dir_all(user_path.parent().expect("user config parent")).expect("user config dir");
        std::fs::write(
            &user_path,
            r#"
[[custom_providers]]
name = "trusted"
display_name = "Trusted"
base_url = "https://llm.example/v1"
model = "model"
api_key_env = "TRUSTED_API_KEY"
"#,
        )
        .expect("user config");
        std::fs::write(&workspace_path, "agent.provider = \"openai\"\n").expect("workspace config");

        let paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
        let provider = WorkspacePathsDefaults::new(Arc::new(paths))
            .with_home_paths(vec![user_path.clone()])
            .with_system_config_paths(Vec::new());

        with_config_defaults_provider_for_test(Arc::new(provider), || {
            let mut state = create_settings_palette_state(workspace, &None).expect("settings state");
            assert_eq!(
                canonicalize(&state.source_path).expect("canonical source path"),
                canonicalize(&workspace_path).expect("canonical workspace path")
            );
            assert_eq!(state.draft.custom_providers.len(), 1);

            apply_settings_action(&mut state, "settings:set:agent.todo_planning_mode:toggle")
                .expect("workspace setting should persist");

            let workspace_content = std::fs::read_to_string(&workspace_path).expect("workspace config content");
            assert!(workspace_content.contains("todo_planning_mode = false"));
            assert!(!workspace_content.contains("custom_providers"));
            assert!(!workspace_content.contains("trusted"));

            let manager = ConfigManager::load_from_workspace(workspace).expect("reloaded configuration");
            assert_eq!(manager.config().custom_providers.len(), 1);
            assert!(!manager.config().agent.todo_planning_mode);

            apply_settings_action(&mut state, "settings:array_add:custom_providers")
                .expect("custom provider should persist to the trusted layer");
            let workspace_content = std::fs::read_to_string(&workspace_path).expect("workspace config content");
            let user_content = std::fs::read_to_string(&user_path).expect("user config content");
            assert!(!workspace_content.contains("custom_providers"));
            assert!(user_content.contains("custom-provider-1"));

            let manager = ConfigManager::load_from_workspace(workspace).expect("reloaded provider configuration");
            assert_eq!(manager.config().custom_providers.len(), 2);
        });
    }

    #[test]
    fn toggle_action_produces_change_feedback_message() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        let mut state = SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: source_path.clone(),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: None,
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        };

        let outcome = apply_settings_action(&mut state, "settings:set:agent.todo_planning_mode:toggle")
            .expect("toggle planning mode");

        assert_eq!(outcome.message.as_deref(), Some("Disabled Todo Planning Mode"));
        assert!(outcome.saved);
        assert!(!state.draft.agent.todo_planning_mode);
    }

    #[test]
    fn settings_palette_state_loads_workspace_config_directly() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_path = temp.path().join("vtcode.toml");
        std::fs::write(&source_path, "[workspace]\nuse_root_config = true\n\n[agent]\ntheme = \"ansi\"\n")
            .expect("workspace config should be written");

        let state = create_settings_palette_state(temp.path(), &None).expect("settings state should load");

        assert_eq!(
            canonicalize(&state.source_path).expect("canonical state source path"),
            canonicalize(&source_path).expect("canonical expected source path")
        );
        assert_eq!(state.draft.agent.theme, "ansi");
    }

    #[test]
    #[serial]
    fn settings_reload_is_fail_closed_and_tracks_layer_creation_and_deletion() {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path();
        let fallback_path = workspace.join(".vtcode").join("vtcode.toml");
        let paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
        let provider = WorkspacePathsDefaults::new(Arc::new(paths))
            .with_home_paths(Vec::new())
            .with_system_config_paths(Vec::new());

        with_config_defaults_provider_for_test(Arc::new(provider), || {
            let mut state = create_settings_palette_state(workspace, &None).expect("settings state should load");
            let initial_provider = state.draft.agent.provider.clone();

            std::fs::create_dir_all(fallback_path.parent().expect("fallback parent")).expect("fallback dir");
            std::fs::write(&fallback_path, "agent.provider = \"openai\"\n").expect("fallback config");
            reload_state_from_disk(&mut state).expect("created fallback should reload");
            assert_eq!(state.draft.agent.provider, "openai");
            assert_eq!(
                canonicalize(&state.source_path).expect("canonical source path"),
                canonicalize(&fallback_path).expect("canonical fallback path")
            );

            let valid_provider = state.draft.agent.provider.clone();
            std::fs::write(&fallback_path, "agent.provider = [\n").expect("malformed fallback config");
            assert!(reload_state_from_disk(&mut state).is_err());
            assert_eq!(state.draft.agent.provider, valid_provider);

            std::fs::remove_file(&fallback_path).expect("remove fallback config");
            reload_state_from_disk(&mut state).expect("deleted fallback should reload defaults");
            assert_eq!(state.draft.agent.provider, initial_provider);
            assert_eq!(state.source_path, workspace.join("vtcode.toml"));
        });
    }

    #[test]
    fn permission_view_summary_includes_mode_and_rule_counts() {
        let mut config = VTCodeConfig::default();
        config.permissions.allow = vec!["Read".to_string()];
        config.permissions.ask = vec!["Bash".to_string(), "Write".to_string()];
        config.permissions.deny = vec!["Edit".to_string()];

        let summary = format_permission_summary(&config);
        assert!(summary.contains("Rules"));
        assert!(summary.contains("deny 1"));
        assert!(summary.contains("ask 2"));
        assert!(summary.contains("allow 1"));
    }

    fn header_test_state(view_path: Option<&str>) -> SettingsPaletteState {
        SettingsPaletteState {
            workspace: PathBuf::from("."),
            source_path: PathBuf::from("vtcode.toml"),
            source_label: Some("test".to_string()),
            draft: VTCodeConfig::default(),
            view_path: view_path.map(ToString::to_string),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        }
    }

    #[test]
    fn settings_header_shows_target_source_and_breadcrumbs() {
        for view in [
            None,
            Some("group:model_provider"),
            Some(SETTINGS_ADVANCED_VIEW_PATH),
            Some("advanced.agent.harness.max_tool_calls_per_turn"),
            Some("permissions"),
            Some(RESET_CONFIRMATION_VIEW),
        ] {
            let state = header_test_state(view);
            let lines = settings_header_lines(&state);
            assert!(lines.iter().any(|line| line.contains("Writing to vtcode.toml")));
            assert!(lines.iter().any(|line| line == "test"));
            assert!(lines.iter().all(|line| !line.contains("Enter") && !line.contains("Esc")));
        }
        let root = settings_header_lines(&header_test_state(None));
        assert_eq!(root.first().map(String::as_str), Some("Pick a group to edit."));
        let advanced = settings_header_lines(&header_test_state(Some(SETTINGS_ADVANCED_VIEW_PATH)));
        assert!(advanced.iter().any(|line| line.contains("Advanced settings")));
    }

    #[test]
    fn settings_header_omits_source_notice_when_config_file_exists() {
        let mut state = header_test_state(None);
        state.source_label = None;
        let lines = settings_header_lines(&state);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.first().map(String::as_str), Some("Pick a group to edit."));
        assert!(lines[1].starts_with("Writing to "));
        assert!(lines.iter().all(|line| !line.contains("Configuration source")));
    }

    #[test]
    fn settings_search_config_enables_fuzzy_with_view_specific_placeholder() {
        let state = header_test_state(None);
        let config = settings_search_config(&state);
        assert!(config.fuzzy);
        assert_eq!(config.placeholder.as_deref(), Some(SETTINGS_SEARCH_PLACEHOLDER));

        let advanced = header_test_state(Some(SETTINGS_ADVANCED_VIEW_PATH));
        let config = settings_search_config(&advanced);
        assert!(config.fuzzy);
        assert_eq!(config.placeholder.as_deref(), Some(ADVANCED_SEARCH_PLACEHOLDER));
    }

    #[test]
    fn short_display_path_prefers_workspace_relative_then_home() {
        let workspace = Path::new("/repo");
        assert_eq!(short_display_path(Path::new("/repo/vtcode.toml"), workspace), "vtcode.toml");
        assert_eq!(short_display_path(Path::new("/repo/sub/dir/vtcode.toml"), workspace), "sub/dir/vtcode.toml");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(short_display_path(&home.join("vtcode.toml"), Path::new("/elsewhere")), "~/vtcode.toml");
        }
        assert_eq!(
            short_display_path(Path::new("/opt/other/vtcode.toml"), Path::new("/elsewhere")),
            "/opt/other/vtcode.toml"
        );
    }
}
