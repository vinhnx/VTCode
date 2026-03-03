use hashbrown::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use regex::Regex;
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineListItem, InlineListSearchConfig, InlineListSelection};

const SETTINGS_TITLE: &str = "VT Code Settings";
const SETTINGS_HINT: &str = "↑/↓ select • Enter apply • ←/→ adjust • Auto-save on change • Esc parent (double Esc closes) • Type to filter";
const SETTINGS_SEARCH_LABEL: &str = "Filter settings";
const SETTINGS_SEARCH_PLACEHOLDER: &str = "path, key, or description";
const ACTION_RELOAD: &str = "settings:reload";
const ACTION_OPEN_ROOT: &str = "settings:open_root";
const ACTION_PREFIX_OPEN: &str = "settings:open:";
const ACTION_PREFIX_ARRAY_ADD: &str = "settings:array_add:";
const ACTION_PREFIX_ARRAY_POP: &str = "settings:array_pop:";
const ACTION_PREFIX_SET: &str = "settings:set:";

const FIELD_REFERENCE_MARKDOWN: &str =
    include_str!("../../../../docs/config/CONFIG_FIELD_REFERENCE.md");

#[derive(Clone)]
pub(crate) struct SettingsPaletteState {
    pub(crate) workspace: PathBuf,
    pub(crate) source_path: PathBuf,
    pub(crate) source_label: String,
    pub(crate) draft: VTCodeConfig,
    pub(crate) view_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct FieldDoc {
    type_name: String,
    default_value: String,
    description: String,
    options: Vec<String>,
}

#[derive(Debug, Default)]
struct FieldDocIndex {
    by_path: HashMap<String, FieldDoc>,
}

impl FieldDocIndex {
    fn lookup(&self, path: &str) -> Option<&FieldDoc> {
        self.by_path
            .get(path)
            .or_else(|| self.by_path.get(&normalize_field_path(path)))
    }
}

#[derive(Debug, Default)]
pub(crate) struct SettingsApplyOutcome {
    pub(crate) message: Option<String>,
    pub(crate) saved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarOperation {
    Toggle,
    Increment,
    Decrement,
    CycleNext,
    CyclePrev,
}

#[derive(Debug, Clone)]
enum PathToken {
    Key(String),
    Index(usize),
}

static QUOTED_VALUE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\"([^\"]+)\""#).expect("valid regex"));
static ARRAY_INDEX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[(\d+)\]").expect("valid regex"));
static FIELD_DOCS: Lazy<FieldDocIndex> = Lazy::new(parse_field_reference_markdown);

pub(crate) fn create_settings_palette_state(
    workspace: &Path,
    vt_snapshot: &Option<VTCodeConfig>,
) -> Result<SettingsPaletteState> {
    let manager = ConfigManager::load().context("Failed to load configuration")?;
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
    lines.push(format!(
        "Editing: {}",
        state
            .view_path
            .as_deref()
            .map_or("categories", |value| value)
    ));
    lines.push("Changes are saved automatically.".to_string());
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

fn build_settings_items(
    state: &SettingsPaletteState,
    draft: &TomlValue,
) -> Result<Vec<InlineListItem>> {
    let mut items = Vec::new();

    items.push(section_item("Actions"));
    items.push(action_item(
        "Reload from disk",
        "Reload effective values from current configuration files",
        Some("Action"),
        ACTION_RELOAD,
    ));

    if let Some(view_path) = state.view_path.as_deref() {
        items.push(action_item(
            "Back to categories",
            "Return to top-level configuration categories",
            Some("Nav"),
            ACTION_OPEN_ROOT,
        ));

        let node = get_node(draft, view_path)
            .ok_or_else(|| anyhow!("Could not resolve settings path {}", view_path))?;
        append_node_items(&mut items, view_path, node)?;
    } else {
        items.push(section_item("Categories"));
        if let TomlValue::Table(table) = draft {
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort();
            for key in keys {
                let Some(value) = table.get(key) else {
                    continue;
                };
                let count = count_leaf_entries(value);
                let subtitle = format!("{} setting{}", count, if count == 1 { "" } else { "s" });
                items.push(InlineListItem {
                    title: key.clone(),
                    subtitle: Some(subtitle),
                    badge: Some("Category".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::ConfigAction(format!(
                        "{}{}",
                        ACTION_PREFIX_OPEN, key
                    ))),
                    search_value: Some(format!("{} category", key)),
                });
            }
        }
    }

    Ok(items)
}

fn append_node_items(items: &mut Vec<InlineListItem>, path: &str, node: &TomlValue) -> Result<()> {
    match node {
        TomlValue::Table(table) => {
            let mut keys: Vec<&String> = table.keys().collect();
            keys.sort();
            for key in keys {
                let Some(value) = table.get(key) else {
                    continue;
                };
                let child_path = format!("{}.{}", path, key);
                items.push(item_for_value(key, &child_path, value));
            }
        }
        TomlValue::Array(entries) => {
            items.push(action_item(
                "Add item",
                "Append a new default item to this array",
                Some("Array"),
                &format!("{}{}", ACTION_PREFIX_ARRAY_ADD, path),
            ));
            items.push(action_item(
                "Remove last item",
                "Remove the final array entry",
                Some("Array"),
                &format!("{}{}", ACTION_PREFIX_ARRAY_POP, path),
            ));

            for (index, value) in entries.iter().enumerate() {
                let child_path = format!("{}[{}]", path, index);
                let label = format!("[{}]", index);
                items.push(item_for_value(&label, &child_path, value));
            }
        }
        _ => {
            items.push(item_for_value(path, path, node));
        }
    }

    Ok(())
}

fn item_for_value(label: &str, path: &str, value: &TomlValue) -> InlineListItem {
    let doc = FIELD_DOCS.lookup(path);
    let description = doc
        .and_then(|entry| {
            if entry.description.is_empty() {
                None
            } else {
                Some(entry.description.clone())
            }
        })
        .unwrap_or_default();

    let summary = summarize_value(value);
    let subtitle = setting_subtitle(path, &summary, &description, false);

    match value {
        TomlValue::Boolean(_) => InlineListItem {
            title: label.to_string(),
            subtitle: Some(subtitle),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}:toggle",
                ACTION_PREFIX_SET, path
            ))),
            search_value: Some(search_value(path, label, doc)),
        },
        TomlValue::Integer(_) | TomlValue::Float(_) => InlineListItem {
            title: label.to_string(),
            subtitle: Some(setting_subtitle(path, &summary, &description, true)),
            badge: Some("Number".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}:inc",
                ACTION_PREFIX_SET, path
            ))),
            search_value: Some(search_value(path, label, doc)),
        },
        TomlValue::String(current) => {
            let has_options = resolve_cycle_options(path, current).len() > 1;
            InlineListItem {
                title: label.to_string(),
                subtitle: Some(setting_subtitle(path, &summary, &description, has_options)),
                badge: Some(if has_options {
                    "Cycle".to_string()
                } else {
                    "String".to_string()
                }),
                indent: 0,
                selection: has_options.then(|| {
                    InlineListSelection::ConfigAction(format!(
                        "{}{}:cycle",
                        ACTION_PREFIX_SET, path
                    ))
                }),
                search_value: Some(search_value(path, label, doc)),
            }
        }
        TomlValue::Array(entries) => InlineListItem {
            title: label.to_string(),
            subtitle: Some(format!(
                "{} • {} entries",
                setting_subtitle(path, &summary, &description, false),
                entries.len()
            )),
            badge: Some("Array".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                ACTION_PREFIX_OPEN, path
            ))),
            search_value: Some(search_value(path, label, doc)),
        },
        TomlValue::Table(table) => InlineListItem {
            title: label.to_string(),
            subtitle: Some(format!(
                "{} • {} keys",
                setting_subtitle(path, &summary, &description, false),
                table.len()
            )),
            badge: Some("Section".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                ACTION_PREFIX_OPEN, path
            ))),
            search_value: Some(search_value(path, label, doc)),
        },
        _ => InlineListItem {
            title: label.to_string(),
            subtitle: Some(setting_subtitle(path, &summary, &description, false)),
            badge: Some("Value".to_string()),
            indent: 0,
            selection: None,
            search_value: Some(search_value(path, label, doc)),
        },
    }
}

fn setting_subtitle(path: &str, summary: &str, description: &str, adjustable: bool) -> String {
    let value_display = if adjustable {
        format!("<- {} ->", summary)
    } else {
        summary.to_string()
    };
    let mut parts = vec![format!("{} = {}", path, value_display)];
    if !description.is_empty() {
        parts.push(description.to_string());
    }
    parts.join(" • ")
}

fn section_item(label: &str) -> InlineListItem {
    InlineListItem {
        title: label.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    }
}

fn action_item(title: &str, subtitle: &str, badge: Option<&str>, action: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(str::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(action.to_string())),
        search_value: Some(format!("{} {}", title, subtitle)),
    }
}

fn search_value(path: &str, label: &str, doc: Option<&FieldDoc>) -> String {
    let mut parts = vec![path.to_string(), label.to_string()];
    if let Some(doc) = doc {
        if !doc.description.is_empty() {
            parts.push(doc.description.clone());
        }
        if !doc.options.is_empty() {
            parts.push(doc.options.join(" "));
        }
    }
    parts.join(" ").to_ascii_lowercase()
}

fn count_leaf_entries(value: &TomlValue) -> usize {
    match value {
        TomlValue::Table(table) => table.values().map(count_leaf_entries).sum(),
        TomlValue::Array(values) => values.len().max(1),
        _ => 1,
    }
}

fn summarize_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => format!("\"{}\"", truncate_middle(text, 48)),
        TomlValue::Integer(number) => number.to_string(),
        TomlValue::Float(number) => number.to_string(),
        TomlValue::Boolean(value) => value.to_string(),
        TomlValue::Array(values) => format!(
            "[{} item{}]",
            values.len(),
            if values.len() == 1 { "" } else { "s" }
        ),
        TomlValue::Table(values) => format!(
            "{{{} key{}}}",
            values.len(),
            if values.len() == 1 { "" } else { "s" }
        ),
        _ => "<unsupported>".to_string(),
    }
}

fn truncate_middle(value: &str, max_len: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_len {
        return value.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }

    let prefix_len = max_len / 2;
    let suffix_len = max_len.saturating_sub(prefix_len + 3);
    let prefix: String = value.chars().take(prefix_len).collect();
    let suffix: String = value
        .chars()
        .skip(total_chars.saturating_sub(suffix_len))
        .collect();
    format!("{prefix}...{suffix}")
}

fn parse_field_reference_markdown() -> FieldDocIndex {
    let mut by_path = HashMap::new();

    for raw_line in FIELD_REFERENCE_MARKDOWN.lines() {
        let line = raw_line.trim();
        if !line.starts_with('|') {
            continue;
        }
        if line.contains("| Field | Type | Required | Default | Description |") {
            continue;
        }
        if line.contains("|-------") {
            continue;
        }

        let columns = split_markdown_row(line);
        if columns.len() < 7 {
            continue;
        }

        let field = columns[1].trim().trim_matches('`').to_string();
        if field.is_empty() {
            continue;
        }

        let type_name = columns[2].trim().to_string();
        let default_value = columns[4].trim().trim_matches('`').to_string();
        let description = columns[5].trim().to_string();

        let options = extract_options(&description);
        by_path.insert(
            field,
            FieldDoc {
                type_name,
                default_value,
                description,
                options,
            },
        );
    }

    FieldDocIndex { by_path }
}

fn split_markdown_row(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '|' {
            cells.push(current.trim().to_string());
            current.clear();
            continue;
        }

        current.push(ch);
    }

    cells.push(current.trim().to_string());
    cells
}

fn extract_options(description: &str) -> Vec<String> {
    let options_source = description
        .split_once("Options:")
        .map(|(_, tail)| tail)
        .or_else(|| description.split_once("options:").map(|(_, tail)| tail));

    let mut values: Vec<String> = Vec::new();

    if let Some(source) = options_source {
        for capture in QUOTED_VALUE_RE.captures_iter(source) {
            if let Some(value) = capture.get(1) {
                values.push(value.as_str().to_string());
            }
        }

        if values.is_empty() {
            values.extend(extract_comma_options(source));
        }
    }

    if values.is_empty()
        && let (Some(start), Some(end)) = (description.find('('), description.find(')'))
        && end > start
    {
        let inside = &description[start + 1..end];
        values.extend(extract_comma_options(inside));
    }

    values.sort();
    values.dedup();
    values
}

fn extract_comma_options(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .map(|entry| entry.trim_matches('\"').trim_matches('\''))
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            entry
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
        })
        .map(str::to_string)
        .collect()
}

fn normalize_field_path(path: &str) -> String {
    ARRAY_INDEX_RE.replace_all(path, "[]").to_string()
}

pub(crate) fn parent_view_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    if path.ends_with(']')
        && let Some(start) = path.rfind('[')
    {
        let parent = &path[..start];
        return (!parent.is_empty()).then(|| parent.to_string());
    }

    path.rfind('.').map(|idx| path[..idx].to_string())
}

fn parse_path_tokens(path: &str) -> Result<Vec<PathToken>> {
    let mut tokens = Vec::new();

    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }

        let mut rest = segment;
        loop {
            if let Some(index_start) = rest.find('[') {
                let key = &rest[..index_start];
                if !key.is_empty() {
                    tokens.push(PathToken::Key(key.to_string()));
                }

                let after_start = &rest[index_start + 1..];
                let Some(index_end) = after_start.find(']') else {
                    bail!(
                        "Invalid path segment '{}': missing closing bracket",
                        segment
                    );
                };

                let index_text = &after_start[..index_end];
                let index = index_text
                    .parse::<usize>()
                    .with_context(|| format!("Invalid array index '{}'", index_text))?;
                tokens.push(PathToken::Index(index));

                rest = &after_start[index_end + 1..];
                if rest.is_empty() {
                    break;
                }
            } else {
                tokens.push(PathToken::Key(rest.to_string()));
                break;
            }
        }
    }

    Ok(tokens)
}

fn get_node<'a>(root: &'a TomlValue, path: &str) -> Option<&'a TomlValue> {
    let tokens = parse_path_tokens(path).ok()?;
    let mut current = root;

    for token in tokens {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    return None;
                };
                current = table.get(&key)?;
            }
            PathToken::Index(index) => {
                let TomlValue::Array(entries) = current else {
                    return None;
                };
                current = entries.get(index)?;
            }
        }
    }

    Some(current)
}

fn get_node_mut<'a>(root: &'a mut TomlValue, path: &str) -> Option<&'a mut TomlValue> {
    let tokens = parse_path_tokens(path).ok()?;
    let mut current = root;

    for token in tokens {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    return None;
                };
                current = table.get_mut(&key)?;
            }
            PathToken::Index(index) => {
                let TomlValue::Array(entries) = current else {
                    return None;
                };
                current = entries.get_mut(index)?;
            }
        }
    }

    Some(current)
}

fn mutate_draft<F>(state: &mut SettingsPaletteState, mutator: F) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<()>,
{
    let mut draft_value = TomlValue::try_from(state.draft.clone())
        .context("Failed to serialize draft configuration")?;
    mutator(&mut draft_value)?;

    let parsed: VTCodeConfig = draft_value
        .try_into()
        .context("Updated draft configuration is invalid")?;
    parsed
        .validate()
        .context("Updated draft configuration failed validation")?;

    state.draft = parsed;
    Ok(())
}

fn mutate_draft_and_persist<F>(state: &mut SettingsPaletteState, mutator: F) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<()>,
{
    let previous_draft = state.draft.clone();
    mutate_draft(state, mutator)?;
    if let Err(err) = persist_draft(state) {
        state.draft = previous_draft;
        return Err(err);
    }
    Ok(())
}

fn reload_state_from_disk(state: &mut SettingsPaletteState) -> Result<()> {
    if state.source_path.exists() {
        let manager = ConfigManager::load_from_file(&state.source_path)
            .with_context(|| format!("Failed to load {}", state.source_path.display()))?;
        state.draft = manager.config().clone();
        state.source_label = format!("Configuration source: {}", state.source_path.display());
        return Ok(());
    }

    let manager = ConfigManager::load().context("Failed to reload runtime defaults")?;
    state.draft = manager.config().clone();
    state.source_label = no_config_source_label(&state.workspace);
    Ok(())
}

fn no_config_source_label(workspace: &Path) -> String {
    format!(
        "No vtcode.toml found for {}. Draft starts from runtime defaults.",
        workspace.display()
    )
}

fn add_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
    let node =
        get_node_mut(root, path).ok_or_else(|| anyhow!("Array path '{}' was not found", path))?;

    let TomlValue::Array(values) = node else {
        bail!("Path '{}' is not an array", path);
    };

    let value = values
        .first()
        .cloned()
        .unwrap_or_else(|| TomlValue::String(String::new()));
    values.push(value);
    Ok(())
}

fn pop_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
    let node =
        get_node_mut(root, path).ok_or_else(|| anyhow!("Array path '{}' was not found", path))?;

    let TomlValue::Array(values) = node else {
        bail!("Path '{}' is not an array", path);
    };

    if values.pop().is_none() {
        bail!("Array '{}' is already empty", path);
    }

    Ok(())
}

fn apply_scalar_operation(
    root: &mut TomlValue,
    path: &str,
    operation: ScalarOperation,
) -> Result<()> {
    let node = get_node_mut(root, path)
        .ok_or_else(|| anyhow!("Settings path '{}' was not found", path))?;

    match node {
        TomlValue::Boolean(value) => {
            if operation != ScalarOperation::Toggle {
                bail!("{} supports toggle only", path);
            }
            *value = !*value;
            Ok(())
        }
        TomlValue::Integer(value) => {
            match operation {
                ScalarOperation::Increment => *value = value.saturating_add(1),
                ScalarOperation::Decrement => *value = value.saturating_sub(1),
                _ => bail!("{} supports numeric increment/decrement", path),
            }
            Ok(())
        }
        TomlValue::Float(value) => {
            match operation {
                ScalarOperation::Increment => *value += 0.1,
                ScalarOperation::Decrement => *value -= 0.1,
                _ => bail!("{} supports numeric increment/decrement", path),
            }
            Ok(())
        }
        TomlValue::String(current) => {
            let options = resolve_cycle_options(path, current);
            if options.is_empty() {
                bail!("{} has no predefined values to cycle", path);
            }
            let next = cycle_string_option(current, &options, operation)?;
            *current = next;
            Ok(())
        }
        _ => bail!("{} is not a scalar setting", path),
    }
}

fn resolve_cycle_options(path: &str, current: &str) -> Vec<String> {
    if normalize_field_path(path) == "agent.theme" {
        return theme::available_themes()
            .into_iter()
            .map(str::to_string)
            .collect();
    }

    FIELD_DOCS
        .lookup(path)
        .map(|doc| doc.options.clone())
        .filter(|options| !options.is_empty())
        .unwrap_or_else(|| {
            if current.is_empty() {
                Vec::new()
            } else {
                vec![current.to_string()]
            }
        })
}

fn cycle_string_option(
    current: &str,
    options: &[String],
    operation: ScalarOperation,
) -> Result<String> {
    if options.is_empty() {
        bail!("No cycle options available")
    }

    let mut ordered = options.to_vec();
    ordered.sort();
    ordered.dedup();

    let current_index = ordered
        .iter()
        .position(|entry| entry == current)
        .unwrap_or(0);
    let next_index = match operation {
        ScalarOperation::CycleNext => (current_index + 1) % ordered.len(),
        ScalarOperation::CyclePrev => {
            if current_index == 0 {
                ordered.len() - 1
            } else {
                current_index - 1
            }
        }
        _ => bail!("Invalid string cycle operation"),
    };

    Ok(ordered[next_index].clone())
}

fn write_commented_config(path: &Path, config: &VTCodeConfig) -> Result<()> {
    let content = render_commented_config(config)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write configuration file {}", path.display()))
}

fn persist_draft(state: &mut SettingsPaletteState) -> Result<()> {
    write_commented_config(&state.source_path, &state.draft)
        .with_context(|| format!("Failed to save {}", state.source_path.display()))?;
    Ok(())
}

fn render_commented_config(config: &VTCodeConfig) -> Result<String> {
    let value = TomlValue::try_from(config.clone())
        .context("Failed to serialize configuration for comment rendering")?;

    let TomlValue::Table(root_table) = value else {
        bail!("Rendered configuration root is not a TOML table")
    };

    let mut output = String::new();
    output.push_str("# VT Code Configuration File\n");
    output.push_str("# This file was generated by /settings interactive save.\n");
    output.push_str(
        "# Every field includes description, possible values (when known), and default.\n\n",
    );

    render_table_with_comments(&mut output, &root_table, None)?;
    Ok(output)
}

fn render_table_with_comments(
    output: &mut String,
    table: &toml::map::Map<String, TomlValue>,
    prefix: Option<&str>,
) -> Result<()> {
    if let Some(path) = prefix
        && !path.is_empty()
    {
        write_section_comments(output, path);
        writeln!(output, "[{}]", path).context("Failed to render table header")?;
    }

    let mut scalar_keys = Vec::new();
    let mut table_keys = Vec::new();

    for (key, value) in table {
        match value {
            TomlValue::Table(_) => table_keys.push(key),
            _ => scalar_keys.push(key),
        }
    }

    scalar_keys.sort();
    table_keys.sort();

    for key in scalar_keys {
        let Some(value) = table.get(key) else {
            continue;
        };
        let path = build_path(prefix, key);
        write_field_comments(output, &path);

        let rendered = render_key_value(key, value)?;
        output.push_str(rendered.trim_end());
        output.push_str("\n\n");
    }

    for (idx, key) in table_keys.iter().enumerate() {
        let Some(TomlValue::Table(child_table)) = table.get(*key) else {
            continue;
        };
        let path = build_path(prefix, key);
        render_table_with_comments(output, child_table, Some(&path))?;

        if idx + 1 < table_keys.len() {
            output.push('\n');
        }
    }

    Ok(())
}

fn render_key_value(key: &str, value: &TomlValue) -> Result<String> {
    let mut table = toml::map::Map::new();
    table.insert(key.to_string(), value.clone());
    toml::to_string_pretty(&TomlValue::Table(table)).context("Failed to render field to TOML")
}

fn write_section_comments(output: &mut String, path: &str) {
    if let Some(doc) = FIELD_DOCS.lookup(path) {
        write_doc_comments(output, doc, false);
    }
}

fn write_field_comments(output: &mut String, path: &str) {
    if let Some(doc) = FIELD_DOCS.lookup(path) {
        write_doc_comments(output, doc, true);
    }
}

fn write_doc_comments(output: &mut String, doc: &FieldDoc, include_type_and_options: bool) {
    if !doc.description.is_empty() {
        push_comment_lines(output, &doc.description);
    }
    if include_type_and_options && !doc.options.is_empty() {
        let _ = writeln!(output, "# Possible values: {}", doc.options.join(", "));
    }
    if !doc.default_value.is_empty() {
        let _ = writeln!(output, "# Default: {}", doc.default_value);
    }
    if include_type_and_options && !doc.type_name.is_empty() {
        let _ = writeln!(output, "# Type: {}", doc.type_name);
    }
}

fn push_comment_lines(output: &mut String, description: &str) {
    for line in wrap_comment(description, 100) {
        let _ = writeln!(output, "# {}", line);
    }
}

fn wrap_comment(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
            continue;
        }

        if current.len() + 1 + word.len() > max_width {
            lines.push(current);
            current = word.to_string();
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn build_path(prefix: Option<&str>, key: &str) -> String {
    match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{}.{}", prefix, key),
        _ => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            normalize_field_path("commands.allow_list[12]"),
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
}
