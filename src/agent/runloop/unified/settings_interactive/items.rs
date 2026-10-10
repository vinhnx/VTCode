use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use super::docs::{FIELD_DOCS, FieldDoc};
use super::mutations::resolve_cycle_options;
use super::path::{PathToken, get_node, parse_path_tokens, path_with_key};
use super::render::{
    action_item, action_item_with_tone, display_title, search_value_for_missing_doc, search_value_with_content,
    section_item, summarize_value,
};
use super::{
    ACTION_BACK, ACTION_CONFIGURE_EDITOR, ACTION_PICK_MAIN_MODEL, ACTION_PREFIX_ARRAY_ADD, ACTION_PREFIX_ARRAY_POP,
    ACTION_PREFIX_EDIT, ACTION_PREFIX_OPEN, ACTION_PREFIX_SET, ACTION_RESET, ACTION_RESET_CANCEL, ACTION_RESET_CONFIRM,
    OPTIONAL_DOC_FIELDS, RESET_CONFIRMATION_VIEW, SETTINGS_ADVANCED_VIEW_PATH, SETTINGS_GROUP_PREFIX,
    SETTINGS_MODEL_CONFIG_MAIN_PATH, SETTINGS_MODEL_CONFIG_PATH, SettingsPaletteState,
};
use crate::agent::runloop::unified::config_section_headings::humanize_identifier;

const HIDDEN_SETTINGS_PATHS: &[&str] = &["agent.small_model"];

#[derive(Debug, Clone, Copy)]
pub(super) struct CuratedSettingsGroup {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) description: &'static str,
    pub(super) paths: &'static [&'static str],
}

static CURATED_GROUPS: &[CuratedSettingsGroup] = &[
    CuratedSettingsGroup {
        id: "model_provider",
        title: "Model & Provider",
        description: "Choose the active provider, model, and response behavior.",
        paths: &[
            "agent.provider",
            "agent.default_model",
            "agent.reasoning_effort",
            "agent.temperature",
            "agent.verbosity",
            "custom_providers",
        ],
    },
    CuratedSettingsGroup {
        id: "agent_automation",
        title: "Agent & Automation",
        description: "Tune planning, prompts, retries, and unattended runs.",
        paths: &[
            "agent.require_plan_confirmation",
            "agent.todo_planning_mode",
            "agent.tool_documentation_mode",
            "agent.system_prompt_mode",
            "agent.max_conversation_turns",
            "agent.max_review_passes",
            "agent.max_task_retries",
            "automation.full_auto.enabled",
            "automation.full_auto.max_turns",
            "automation.full_auto.require_profile_ack",
            "automation.scheduled_tasks.enabled",
        ],
    },
    CuratedSettingsGroup {
        id: "approvals_security",
        title: "Approvals & Security",
        description: "Control approval rules, sandboxing, and protection defaults.",
        paths: &[
            "permissions.enabled",
            "permissions.audit_enabled",
            "permissions.auto.use_decisions_probe",
            "permissions.allow",
            "permissions.ask",
            "permissions.deny",
            "security.human_in_the_loop",
            "security.hitl_notification_bell",
            "security.require_write_tool_for_claims",
            "sandbox.enabled",
            "sandbox.default_policy",
            "sandbox.network.policy",
            "dotfile_protection.enabled",
        ],
    },
    CuratedSettingsGroup {
        id: "tools_integrations",
        title: "Tools & Integrations",
        description: "Configure tools, the editor, search, MCP, and IDE bridges.",
        paths: &[
            "tools.default_policy",
            "tools.profile",
            "tools.max_tool_loops",
            "tools.client_tool_search",
            "tools.editor",
            "tools.web_fetch.mode",
            "tools.web_fetch.strict_https_only",
            "tools.web_search.provider",
            "mcp.enabled",
            "mcp.providers",
            "acp.enabled",
        ],
    },
    CuratedSettingsGroup {
        id: "context_memory",
        title: "Context & Memory",
        description: "Manage context budgets, dynamic context, and persistent memory.",
        paths: &[
            "features.memories",
            "agent.persistent_memory.enabled",
            "agent.persistent_memory.auto_write",
            "agent.persistent_memory.memories.use_memories",
            "agent.persistent_memory.memories.generate_memories",
            "context.max_context_tokens",
            "context.dynamic.enabled",
            "context.dynamic.tool_output_threshold",
            "context.dynamic.retained_user_messages",
            "context.ledger.enabled",
            "context.ledger.include_in_prompt",
            "workspace.include_context",
        ],
    },
    CuratedSettingsGroup {
        id: "interface_terminal",
        title: "Interface & Terminal",
        description: "Shape the chat surface, transcript, theme, and shell sessions.",
        paths: &[
            "agent.theme",
            "ui.display_mode",
            "ui.tool_display_mode",
            "ui.tool_output_mode",
            "ui.reasoning_display_mode",
            "ui.show_sidebar",
            "ui.show_task_panel",
            "ui.vim_mode",
            "ui.color_scheme_mode",
            "ui.fullscreen.mouse_capture",
            "ui.fullscreen.copy_on_select",
            "ui.fullscreen.scroll_speed",
            "pty.enabled",
            "pty.command_timeout_seconds",
            "pty.scrollback_lines",
        ],
    },
    CuratedSettingsGroup {
        id: "performance_diagnostics",
        title: "Performance & Diagnostics",
        description: "Balance runtime limits, caching, timeouts, and diagnostics.",
        paths: &[
            "agent.harness.max_tool_calls_per_turn",
            "agent.harness.max_tool_wall_clock_secs",
            "agent.harness.max_tool_retries",
            "agent.harness.max_parallel_tool_calls",
            "agent.harness.auto_compaction_enabled",
            "optimization.command_cache.enabled",
            "optimization.file_read_cache.enabled",
            "optimization.llm_client.enable_response_caching",
            "timeouts.default_ceiling_seconds",
            "timeouts.long_running_command_ceiling_seconds",
            "telemetry.trajectory_enabled",
            "telemetry.atif_enabled",
            "ui.show_diagnostics_in_transcript",
            "optimization.profiling.enabled",
        ],
    },
];

pub(super) fn curated_group(id: &str) -> Option<&'static CuratedSettingsGroup> {
    curated_groups().iter().find(|group| group.id == id)
}

pub(super) fn curated_groups() -> &'static [CuratedSettingsGroup] {
    CURATED_GROUPS
}

pub(super) fn build_settings_items(state: &SettingsPaletteState, draft: &TomlValue) -> Result<Vec<InlineListItem>> {
    let mut items = Vec::new();

    if state.view_path.as_deref() == Some(RESET_CONFIRMATION_VIEW) {
        items.push(section_item("Confirm reset"));
        items.push(action_item_with_tone(
            "Reset everything",
            "Clear every setting in the current write target (credentials are preserved)",
            Some("Destructive"),
            ACTION_RESET_CONFIRM,
            vtcode_commons::ui_protocol::InlineTone::Danger,
        ));
        items.push(action_item_with_tone(
            "Keep settings",
            "Return to the settings sections without changing configuration",
            Some("Cancel"),
            ACTION_RESET_CANCEL,
            vtcode_commons::ui_protocol::InlineTone::Neutral,
        ));
        return Ok(items);
    }

    if let Some(view_path) = state.view_path.as_deref() {
        items.push(action_item_with_tone(
            "← Back",
            "Return to the previous settings view",
            None,
            ACTION_BACK,
            vtcode_commons::ui_protocol::InlineTone::Neutral,
        ));
        items.push(action_item(
            "Reload configuration",
            "Reload effective values from current configuration files",
            None,
            super::ACTION_RELOAD,
        ));

        if view_path == SETTINGS_ADVANCED_VIEW_PATH
            || view_path.starts_with("advanced.")
            || view_path.starts_with(super::SETTINGS_ADVANCED_NESTED_PREFIX)
        {
            append_advanced_items(&mut items, view_path, draft)?;
            return Ok(items);
        }

        if let Some(group_view) = view_path.strip_prefix(SETTINGS_GROUP_PREFIX) {
            if let Some((group_id, nested_path)) = group_view.split_once(':') {
                let node = get_node(draft, nested_path)
                    .ok_or_else(|| anyhow!("Could not resolve settings path {nested_path}"))?;
                let open_prefix = format!("{SETTINGS_GROUP_PREFIX}{group_id}:");
                append_node_items_with_context(&mut items, nested_path, node, draft, Some(open_prefix.as_str()))?;
                return Ok(items);
            }

            if let Some(group) = curated_group(group_view) {
                append_curated_group_items(&mut items, group, draft);
                return Ok(items);
            }
        }

        if append_synthetic_model_config_items(&mut items, view_path, draft)? {
            return Ok(items);
        }

        let node = get_node(draft, view_path).ok_or_else(|| anyhow!("Could not resolve settings path {view_path}"))?;
        append_node_items(&mut items, view_path, node, draft)?;
    } else {
        append_curated_root_items(&mut items, draft);
        items.push(section_item("Actions"));
        items.push(action_item(
            "Reload configuration",
            "Re-read effective values from the config files",
            None,
            super::ACTION_RELOAD,
        ));
        items.push(action_item_with_tone(
            "Reset configuration",
            "Clear every setting in the write target (asks first)",
            None,
            ACTION_RESET,
            vtcode_commons::ui_protocol::InlineTone::Danger,
        ));
    }

    Ok(items)
}

fn append_curated_root_items(items: &mut Vec<InlineListItem>, draft: &TomlValue) {
    for group in curated_groups() {
        let count = curated_group_items(group, draft, None).len();
        let count_label = if count == 1 {
            "editable setting"
        } else {
            "editable settings"
        };
        let paths = group.paths.join(" ");
        items.push(
            vtcode_ui::design::list::action(
                group.title,
                format!("{} • {count} {count_label}", group.description),
                None,
                vtcode_commons::ui_protocol::InlineTone::Neutral,
                Some(InlineListSelection::ConfigAction(format!(
                    "{ACTION_PREFIX_OPEN}{SETTINGS_GROUP_PREFIX}{}",
                    group.id
                ))),
            )
            .with_search_value(
                format!("{} {} {} {paths}", group.title, group.id, group.description).to_ascii_lowercase(),
            ),
        );
    }

    items.push(action_item(
        "Advanced settings",
        &format!(
            "Search the complete documented configuration • {} editable settings",
            advanced_editable_count(draft)
        ),
        None,
        &format!("{ACTION_PREFIX_OPEN}{SETTINGS_ADVANCED_VIEW_PATH}"),
    ));
}

fn curated_group_items(
    group: &CuratedSettingsGroup,
    draft: &TomlValue,
    open_prefix: Option<&str>,
) -> Vec<InlineListItem> {
    group
        .paths
        .iter()
        .filter_map(|path| curated_setting_item(path, draft, open_prefix))
        .filter(|item| item.selection.is_some())
        .collect()
}

fn append_curated_group_items(items: &mut Vec<InlineListItem>, group: &CuratedSettingsGroup, draft: &TomlValue) {
    let open_prefix = format!("{SETTINGS_GROUP_PREFIX}{}:", group.id);
    items.extend(
        group
            .paths
            .iter()
            .filter_map(|path| curated_setting_item(path, draft, Some(open_prefix.as_str()))),
    );
}

fn curated_setting_item(path: &str, draft: &TomlValue, open_prefix: Option<&str>) -> Option<InlineListItem> {
    if let Some(value) = get_node(draft, path) {
        return Some(with_open_context(
            item_for_value(path.rsplit('.').next().unwrap_or(path), path, value, draft),
            open_prefix,
        ));
    }

    let doc = FIELD_DOCS.lookup(path)?;
    (!doc.options.is_empty()).then(|| {
        with_open_context(item_for_missing_doc_value(path.rsplit('.').next().unwrap_or(path), path), open_prefix)
    })
}

fn append_advanced_items(items: &mut Vec<InlineListItem>, view_path: &str, draft: &TomlValue) -> Result<()> {
    if let Some(path) = view_path.strip_prefix(super::SETTINGS_ADVANCED_NESTED_PREFIX) {
        if let Some(node) = get_node(draft, path)
            && matches!(node, TomlValue::Array(_) | TomlValue::Table(_))
        {
            return append_node_items_with_context(
                items,
                path,
                node,
                draft,
                Some(super::SETTINGS_ADVANCED_NESTED_PREFIX),
            );
        }
        return append_advanced_path_item(items, path, draft, Some(super::SETTINGS_ADVANCED_NESTED_PREFIX));
    }

    if let Some(path) = view_path.strip_prefix("advanced.") {
        if let Some(node) = get_node(draft, path)
            && matches!(node, TomlValue::Array(_) | TomlValue::Table(_))
        {
            return append_node_items_with_context(
                items,
                path,
                node,
                draft,
                Some(super::SETTINGS_ADVANCED_NESTED_PREFIX),
            );
        }
        return append_advanced_path_item(items, path, draft, None);
    }

    for path in advanced_paths(draft) {
        append_advanced_path_item(items, &path, draft, Some("advanced."))?;
    }

    Ok(())
}

fn advanced_paths(draft: &TomlValue) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    collect_advanced_paths(draft, "", &mut paths);
    for path in FIELD_DOCS.sorted_paths() {
        paths.insert(path.to_string());
    }
    paths
}

fn advanced_editable_count(draft: &TomlValue) -> usize {
    advanced_paths(draft)
        .into_iter()
        .filter(|path| {
            if is_schema_path(path) {
                return false;
            }
            if let Some(value) = get_node(draft, path) {
                return item_for_value(&advanced_label(path), path, value, draft).selection.is_some();
            }

            FIELD_DOCS.lookup(path).is_some_and(|doc| !doc.options.is_empty())
        })
        .count()
}

fn append_advanced_path_item(
    items: &mut Vec<InlineListItem>,
    path: &str,
    draft: &TomlValue,
    open_prefix: Option<&str>,
) -> Result<()> {
    let label = advanced_label(path);
    if let Some(value) = get_node(draft, path) {
        items.push(advanced_item_with_path(item_for_value(&label, path, value, draft), path, open_prefix));
        return Ok(());
    }

    if let Some(doc) = FIELD_DOCS.lookup(path) {
        if !is_schema_path(path) && !doc.options.is_empty() {
            items.push(advanced_item_with_path(item_for_missing_doc_value(&label, path), path, open_prefix));
        } else {
            items.push(advanced_schema_item(path, doc));
        }
        return Ok(());
    }

    Err(anyhow!("Could not resolve advanced settings path {path}"))
}

fn is_schema_path(path: &str) -> bool {
    path.contains("[]") || path.split('.').any(|segment| segment == "*")
}

fn collect_advanced_paths(value: &TomlValue, path: &str, paths: &mut BTreeSet<String>) {
    match value {
        TomlValue::Table(table) => {
            for key in super::render::sorted_table_keys(table) {
                let Some(child) = table.get(key) else {
                    continue;
                };
                let child_path = path_with_key(path, key);
                collect_advanced_paths(child, &child_path, paths);
            }
        }
        TomlValue::Array(entries) => {
            if !path.is_empty() {
                paths.insert(path.to_string());
            }
            for (index, entry) in entries.iter().enumerate() {
                collect_advanced_paths(entry, &format!("{path}[{index}]"), paths);
            }
        }
        _ if !path.is_empty() => {
            paths.insert(path.to_string());
        }
        _ => {}
    }
}

fn advanced_item_with_path(mut item: InlineListItem, path: &str, open_prefix: Option<&str>) -> InlineListItem {
    item = with_open_context(item, open_prefix);
    let detail = item.subtitle.take().unwrap_or_default();
    item.subtitle = Some(format!("{path} • {detail}"));
    item
}

fn advanced_schema_item(path: &str, doc: &FieldDoc) -> InlineListItem {
    let label = advanced_label(path);
    let mut terms = vec![path.to_string(), label.clone(), humanize_identifier(path)];
    if !doc.default_value.is_empty() {
        terms.push(doc.default_value.clone());
    }
    if !doc.description.is_empty() {
        terms.push(doc.description.clone());
    }
    terms.extend(doc.options.iter().cloned());

    vtcode_ui::design::list::hint(label)
        .with_subtitle(format!(
            "{path} • {}",
            if doc.description.is_empty() {
                "Documented schema field; configure a concrete entry to edit it."
            } else {
                doc.description.as_str()
            }
        ))
        .with_badge("Schema", vtcode_commons::ui_protocol::InlineTone::Neutral)
        .with_search_value(terms.join(" ").to_ascii_lowercase())
}

fn advanced_label(path: &str) -> String {
    let fallback = path
        .strip_suffix("[]")
        .or_else(|| path.strip_suffix(".*"))
        .unwrap_or(path)
        .rsplit('.')
        .next()
        .unwrap_or(path);
    let label = parse_path_tokens(path)
        .ok()
        .and_then(|tokens| tokens.into_iter().next_back())
        .and_then(|token| match token {
            PathToken::Key(key) if key != "*" => Some(key),
            PathToken::Index(_) => None,
            PathToken::Key(_) => None,
        })
        .unwrap_or_else(|| fallback.to_string());
    humanize_identifier(if label.is_empty() { path } else { &label })
}

fn append_node_items(
    items: &mut Vec<InlineListItem>,
    path: &str,
    node: &TomlValue,
    draft_root: &TomlValue,
) -> Result<()> {
    append_node_items_with_context(items, path, node, draft_root, None)
}

fn append_node_items_with_context(
    items: &mut Vec<InlineListItem>,
    path: &str,
    node: &TomlValue,
    draft_root: &TomlValue,
    open_prefix: Option<&str>,
) -> Result<()> {
    match node {
        TomlValue::Table(table) => {
            append_table_items(items, table, Some(path), Some(node), draft_root, open_prefix);
        }
        TomlValue::Array(entries) => {
            items.push(action_item(
                "Add item",
                "Append a new default item to this array",
                None,
                &format!("{ACTION_PREFIX_ARRAY_ADD}{path}"),
            ));
            items.push(action_item(
                "Remove last item",
                "Remove the final array entry",
                None,
                &format!("{ACTION_PREFIX_ARRAY_POP}{path}"),
            ));

            for (index, value) in entries.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                let label = format!("[{index}]");
                items.push(with_open_context(item_for_value(&label, &child_path, value, draft_root), open_prefix));
            }
        }
        _ => {
            items.push(with_open_context(item_for_value(path, path, node, draft_root), open_prefix));
        }
    }

    Ok(())
}

fn append_synthetic_model_config_items(
    items: &mut Vec<InlineListItem>,
    view_path: &str,
    draft_root: &TomlValue,
) -> Result<bool> {
    match view_path {
        SETTINGS_MODEL_CONFIG_PATH => {
            items.push(section_item("Sections"));
            items.push(action_item(
                "Main Model",
                "Provider and default model for the active conversation model",
                None,
                &format!("{ACTION_PREFIX_OPEN}{SETTINGS_MODEL_CONFIG_MAIN_PATH}"),
            ));
            Ok(true)
        }
        SETTINGS_MODEL_CONFIG_MAIN_PATH => {
            items.push(section_item("Settings"));
            append_mapped_setting_item(items, draft_root, "agent.provider");
            append_mapped_setting_item(items, draft_root, "agent.default_model");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn append_mapped_setting_item(items: &mut Vec<InlineListItem>, draft_root: &TomlValue, path: &str) {
    let Some(value) = get_node(draft_root, path) else {
        return;
    };
    let label = path.rsplit('.').next().unwrap_or(path);
    items.push(item_for_value(label, path, value, draft_root));
}

fn append_table_items(
    items: &mut Vec<InlineListItem>,
    table: &toml::map::Map<String, TomlValue>,
    parent_path: Option<&str>,
    optional_doc_root: Option<&TomlValue>,
    draft_root: &TomlValue,
    open_prefix: Option<&str>,
) {
    let mut section_items = Vec::new();
    let mut setting_items = Vec::new();

    for key in super::render::sorted_table_keys(table) {
        let Some(value) = table.get(key) else {
            continue;
        };
        let path = parent_path
            .map(|parent| path_with_key(parent, key))
            .unwrap_or_else(|| path_with_key("", key));
        if HIDDEN_SETTINGS_PATHS.contains(&path.as_str()) {
            continue;
        }
        let entry = with_open_context(item_for_value(key, &path, value, draft_root), open_prefix);
        if matches!(value, TomlValue::Table(_)) {
            section_items.push(entry);
        } else {
            setting_items.push(entry);
        }
    }

    if let (Some(root), Some(path)) = (optional_doc_root, parent_path) {
        append_missing_optional_doc_items(&mut setting_items, root, Some(path));
    }

    if !section_items.is_empty() {
        items.push(section_item("Sections"));
        items.extend(section_items);
    }
    if !setting_items.is_empty() {
        items.push(section_item("Settings"));
        items.extend(setting_items);
    }
}

fn with_open_context(mut item: InlineListItem, open_prefix: Option<&str>) -> InlineListItem {
    let Some(open_prefix) = open_prefix else {
        return item;
    };
    let Some(InlineListSelection::ConfigAction(action)) = item.selection.as_mut() else {
        return item;
    };
    let Some(path) = action.strip_prefix(ACTION_PREFIX_OPEN) else {
        return item;
    };

    *action = format!("{ACTION_PREFIX_OPEN}{open_prefix}{path}");
    item
}

fn append_missing_optional_doc_items(items: &mut Vec<InlineListItem>, root: &TomlValue, parent_path: Option<&str>) {
    for path in OPTIONAL_DOC_FIELDS {
        let lookup_path = parent_path
            .and_then(|parent| path.strip_prefix(parent).and_then(|suffix| suffix.strip_prefix('.')))
            .unwrap_or(path);
        if get_node(root, lookup_path).is_some() {
            continue;
        }

        let Some(label) = missing_doc_label(path, parent_path) else {
            continue;
        };
        items.push(item_for_missing_doc_value(label, path));
    }
}

/// Canonical setting row: title + value + description (design::list::setting).
fn setting_row(
    title: String,
    value: String,
    description: Option<String>,
    tone: vtcode_commons::ui_protocol::InlineTone,
    selection: Option<InlineListSelection>,
    search_value: String,
) -> InlineListItem {
    let mut row = vtcode_ui::design::list::setting(title, Some(value), description, selection);
    row.badge_tone = tone;
    row.search_value = Some(search_value);
    row
}

fn item_for_value(label: &str, path: &str, value: &TomlValue, draft_root: &TomlValue) -> InlineListItem {
    use vtcode_commons::ui_protocol::InlineTone;

    let doc = FIELD_DOCS.lookup(path);
    let title = display_title(label, path, value);
    let description = doc.and_then(|entry| (!entry.description.is_empty()).then(|| entry.description.clone()));
    let summary = summarize_value(value);
    let search_value = search_value_with_content(path, label, value, doc);

    if path == "agent.default_model" {
        return setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string())),
            search_value,
        );
    }

    if path == "tools.editor" {
        return setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string())),
            search_value,
        );
    }

    match value {
        TomlValue::Boolean(enabled) => setting_row(
            title,
            if *enabled { "On".to_string() } else { "Off".to_string() },
            description,
            if *enabled {
                InlineTone::Success
            } else {
                InlineTone::Neutral
            },
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_SET}{path}:toggle"))),
            search_value,
        ),
        TomlValue::Integer(_) | TomlValue::Float(_) => setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_SET}{path}:inc"))),
            search_value,
        ),
        TomlValue::String(current) => {
            let has_options = resolve_cycle_options(Some(draft_root), path, current).len() > 1;
            let action = if has_options {
                format!("{ACTION_PREFIX_SET}{path}:cycle")
            } else {
                format!("{ACTION_PREFIX_EDIT}{path}")
            };
            setting_row(
                title,
                summary,
                description,
                InlineTone::Accent,
                Some(InlineListSelection::ConfigAction(action)),
                search_value,
            )
        }
        TomlValue::Array(entries) => setting_row(
            title,
            format!("{} item{}", entries.len(), if entries.len() == 1 { "" } else { "s" }),
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_OPEN}{path}"))),
            search_value,
        ),
        TomlValue::Table(_) => setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_OPEN}{path}"))),
            search_value,
        ),
        _ => setting_row(title, summary, description, InlineTone::Neutral, None, search_value),
    }
}

fn item_for_missing_doc_value(label: &str, path: &str) -> InlineListItem {
    let doc = FIELD_DOCS.lookup(path);
    let description = doc.and_then(|entry| (!entry.description.is_empty()).then(|| entry.description.clone()));
    let has_options = doc.map(|entry| !entry.options.is_empty()).unwrap_or(false);
    let action = if has_options {
        format!("{ACTION_PREFIX_SET}{path}:cycle")
    } else {
        format!("{ACTION_PREFIX_EDIT}{path}")
    };

    setting_row(
        humanize_identifier(label),
        "<unset>".to_string(),
        description,
        vtcode_commons::ui_protocol::InlineTone::Neutral,
        Some(InlineListSelection::ConfigAction(action)),
        search_value_for_missing_doc(path, label, doc),
    )
}

fn missing_doc_label<'a>(path: &'a str, parent_path: Option<&str>) -> Option<&'a str> {
    match parent_path {
        Some(parent) => path
            .strip_prefix(parent)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .filter(|suffix| !suffix.contains('.') && !suffix.contains('[')),
        None => Some(path),
    }
}

#[cfg(test)]
mod tests {
    use super::advanced_label;

    #[test]
    fn advanced_label_uses_quoted_map_key() {
        assert_eq!(advanced_label(r#"mcp.providers[0].env["A.B"]"#), "A B");
    }
}
