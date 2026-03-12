use anyhow::{Context, Result, anyhow, bail};
use std::fmt::Write as _;
use std::path::Path;
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::{ConfigReadRequest, ConfigService};
use vtcode_core::ui::theme;

use crate::agent::runloop::unified::config_section_headings::{
    heading_for_path, normalize_config_path,
};

use super::SettingsPaletteState;
use super::docs::{FIELD_DOCS, FieldDoc};
use super::path::{PathToken, get_node_mut, parse_path_tokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScalarOperation {
    Toggle,
    Increment,
    Decrement,
    CycleNext,
    CyclePrev,
}

pub(super) fn mutate_draft_and_persist<F>(
    state: &mut SettingsPaletteState,
    mutator: F,
) -> Result<()>
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

pub(super) fn reload_state_from_disk(state: &mut SettingsPaletteState) -> Result<()> {
    if let Ok(response) = ConfigService::read(ConfigReadRequest {
        workspace: state.workspace.clone(),
        runtime_overrides: Vec::new(),
    }) && let Ok(config) = serde_json::from_value::<VTCodeConfig>(response.effective_config)
    {
        state.draft = config;
        if state.source_path.exists() {
            state.source_label = format!("Configuration source: {}", state.source_path.display());
        } else {
            state.source_label = no_config_source_label(&state.workspace);
        }
        return Ok(());
    }

    if state.source_path.exists() {
        let manager = ConfigManager::load_from_file(&state.source_path)
            .with_context(|| format!("Failed to load {}", state.source_path.display()))?;
        state.draft = manager.config().clone();
        state.source_label = format!("Configuration source: {}", state.source_path.display());
        return Ok(());
    }

    let manager = ConfigManager::load_from_workspace(&state.workspace)
        .context("Failed to reload runtime defaults")?;
    state.draft = manager.config().clone();
    state.source_label = no_config_source_label(&state.workspace);
    Ok(())
}

pub(super) fn no_config_source_label(workspace: &Path) -> String {
    format!(
        "No vtcode.toml found for {}. Draft starts from runtime defaults.",
        workspace.display()
    )
}

pub(super) fn add_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
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

pub(super) fn pop_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
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

pub(super) fn apply_scalar_operation(
    root: &mut TomlValue,
    path: &str,
    operation: ScalarOperation,
) -> Result<()> {
    let Some(node) = get_node_mut(root, path) else {
        return apply_missing_scalar_operation(root, path, operation);
    };

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

fn apply_missing_scalar_operation(
    root: &mut TomlValue,
    path: &str,
    operation: ScalarOperation,
) -> Result<()> {
    match operation {
        ScalarOperation::CycleNext | ScalarOperation::CyclePrev => {
            let mut options = resolve_cycle_options(path, "");
            if options.is_empty() {
                bail!("Settings path '{}' was not found", path);
            }
            options.sort();
            options.dedup();
            let value = match operation {
                ScalarOperation::CycleNext => options.first().cloned(),
                ScalarOperation::CyclePrev => options.last().cloned(),
                _ => None,
            }
            .ok_or_else(|| anyhow!("{} has no predefined values to cycle", path))?;
            insert_missing_string_value(root, path, value)?;
            Ok(())
        }
        _ => bail!("Settings path '{}' was not found", path),
    }
}

fn insert_missing_string_value(root: &mut TomlValue, path: &str, value: String) -> Result<()> {
    let tokens = parse_path_tokens(path)?;
    if tokens.is_empty() {
        bail!("Settings path '{}' was not found", path);
    }

    let mut current = root;
    for token in &tokens[..tokens.len() - 1] {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    bail!("Parent path for '{}' is not a table", path);
                };
                current = table
                    .entry(key.clone())
                    .or_insert_with(|| TomlValue::Table(toml::map::Map::new()));
            }
            PathToken::Index(_) => bail!("Cannot create missing array path '{}'", path),
        }
    }

    match tokens.last() {
        Some(PathToken::Key(key)) => {
            let TomlValue::Table(table) = current else {
                bail!("Parent path for '{}' is not a table", path);
            };
            table.insert(key.clone(), TomlValue::String(value));
            Ok(())
        }
        Some(PathToken::Index(_)) => bail!("Cannot create missing array path '{}'", path),
        None => bail!("Settings path '{}' was not found", path),
    }
}

pub(super) fn resolve_cycle_options(path: &str, current: &str) -> Vec<String> {
    if normalize_config_path(path) == "agent.theme" {
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

pub(super) fn mutate_draft<F>(state: &mut SettingsPaletteState, mutator: F) -> Result<()>
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

pub(super) fn render_commented_config(config: &VTCodeConfig) -> Result<String> {
    let value = TomlValue::try_from(config.clone())
        .context("Failed to serialize configuration for comment rendering")?;

    let TomlValue::Table(root_table) = value else {
        bail!("Rendered configuration root is not a TOML table")
    };

    let mut output = String::new();
    output.push_str("# VT Code Configuration File\n");
    output.push_str("# Saved from /config with readable section headings.\n");
    output.push_str(
        "# Every field includes descriptions, defaults, and known choices where available.\n\n",
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
    let heading = heading_for_path(path);
    if !heading.title.is_empty() {
        let _ = writeln!(output, "# {}", heading.title);
    }
    if !heading.summary.is_empty() {
        push_comment_lines(output, &heading.summary);
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
