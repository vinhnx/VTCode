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
const OPTIONAL_DOC_FIELDS: &[&str] = &[
    "provider.anthropic.thinking_display",
    "provider.anthropic.speed",
    "provider.openai.service_tier",
];
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

/// CLI-owned metadata used by native clients to render the same `/config` catalog.
#[derive(Clone, Debug)]
pub(crate) struct DesktopConfigField {
    pub(crate) path: String,
    pub(crate) label: String,
    pub(crate) type_name: String,
    pub(crate) default_value: String,
    pub(crate) description: String,
    pub(crate) options: Vec<String>,
    pub(crate) group: String,
    pub(crate) group_description: String,
    pub(crate) editable: bool,
}

/// Build the native client's catalog from the exact field docs and paths used by `/config`.
pub(crate) fn desktop_config_catalog(config: &VTCodeConfig) -> Result<Vec<DesktopConfigField>> {
    let draft = TomlValue::try_from(config.clone()).context("Failed to serialize effective configuration")?;
    let mut fields = Vec::new();

    for path in items::advanced_paths(&draft) {
        // The native editor edits an array as one typed JSON value, matching its
        // CLI behavior without inventing a second indexed-path writer.
        if path.contains('[') || path.contains(']') {
            continue;
        }

        let doc = FIELD_DOCS.lookup(&path);
        let group = items::curated_group_for_path(&path);
        let type_name = doc
            .map(|entry| entry.type_name.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| get_node(&draft, &path).map(config_value_type))
            .unwrap_or_else(|| "documented setting".to_string());
        let options = doc
            .map(|entry| entry.options.clone())
            .filter(|options| !options.is_empty())
            .unwrap_or_else(|| {
                get_node(&draft, &path)
                    .and_then(TomlValue::as_str)
                    .map(|current| mutations::resolve_cycle_options(Some(&draft), &path, current))
                    .filter(|options| options.len() > 1)
                    .unwrap_or_default()
            });
        let editable = items::path_is_editable(&path, &draft)
            || (!items::is_schema_path(&path)
                && get_node(&draft, &path).is_none()
                && desktop_type_is_editable(&type_name));

        fields.push(DesktopConfigField {
            label: items::advanced_label(&path),
            editable,
            default_value: doc.map_or_else(String::new, |entry| entry.default_value.clone()),
            description: doc.map_or_else(String::new, |entry| entry.description.clone()),
            options,
            group: group.map_or_else(|| "Advanced settings".to_string(), |group| group.title.to_string()),
            group_description: group.map_or_else(
                || "Search all documented CLI settings.".to_string(),
                |group| group.description.to_string(),
            ),
            path,
            type_name,
        });
    }

    Ok(fields)
}

fn config_value_type(value: &TomlValue) -> String {
    match value {
        TomlValue::String(_) => "string",
        TomlValue::Integer(_) => "integer",
        TomlValue::Float(_) => "number",
        TomlValue::Boolean(_) => "boolean",
        TomlValue::Array(_) => "array",
        TomlValue::Table(_) => "table",
        TomlValue::Datetime(_) => "datetime",
    }
    .to_string()
}

fn desktop_type_is_editable(type_name: &str) -> bool {
    let type_name = type_name.to_ascii_lowercase();
    type_name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|token| {
            matches!(
                token,
                "bool"
                    | "boolean"
                    | "int"
                    | "integer"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "isize"
                    | "usize"
                    | "float"
                    | "f32"
                    | "f64"
                    | "number"
                    | "string"
                    | "str"
                    | "path"
                    | "pathbuf"
                    | "array"
                    | "vec"
                    | "list"
                    | "table"
                    | "object"
                    | "map"
                    | "hashmap"
                    | "btreemap"
            )
        })
}

pub(crate) fn create_settings_palette_state(
    workspace: &Path,
    vt_snapshot: Option<&VTCodeConfig>,
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
        vt_snapshot.cloned().unwrap_or_else(|| manager.config().clone())
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
mod tests;
