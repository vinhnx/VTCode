use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;
use vtcode_commons::VtCodePaths;
use vtcode_commons::config_keys::ConfigKey;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig, explicit_config_path};
use vtcode_core::config::{constants::defaults, constants::model_helpers};
use vtcode_core::llm::{auto_lightweight_model, lightweight_model_choices};
use vtcode_tui_runtime::ui::theme;

use crate::agent::runloop::unified::config_section_headings::{heading_for_path, normalize_config_path};

use super::SettingsPaletteState;
use super::docs::{FIELD_DOCS, FieldDoc};
use super::path::{PathToken, get_node, get_node_mut, parse_path_tokens, path_with_key, set_node};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScalarOperation {
    Toggle,
    Increment,
    Decrement,
    CycleNext,
    CyclePrev,
}

pub(super) fn mutate_draft_and_persist<F>(state: &mut SettingsPaletteState, path: &str, mutator: F) -> Result<()>
where
    F: FnOnce(&mut TomlValue) -> Result<()>,
{
    let previous_draft = state.draft.clone();
    mutate_draft(state, mutator)?;
    if let Err(err) = persist_draft(state, path) {
        state.draft = previous_draft;
        return Err(err);
    }
    Ok(())
}

pub(crate) fn reload_state_from_disk(state: &mut SettingsPaletteState) -> Result<()> {
    ConfigManager::invalidate_workspace_cache(&state.workspace);
    let manager = ConfigManager::load_from_workspace_with_repository_repair(&state.workspace)
        .context("Failed to reload configuration")?;
    state.draft = manager.config().clone();
    if let Some(source_path) = manager.config_path() {
        state.source_path = source_path.to_path_buf();
        state.source_label = None;
    } else {
        state.source_path = state.workspace.join("vtcode.toml");
        state.source_label = Some(no_config_source_label());
    }
    Ok(())
}

pub(super) fn no_config_source_label() -> String {
    "No vtcode.toml found. Draft starts from runtime defaults.".to_string()
}

pub(super) fn add_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
    let node = get_node_mut(root, path).ok_or_else(|| anyhow!("Array path '{path}' was not found"))?;

    let TomlValue::Array(values) = node else {
        bail!("Path '{path}' is not an array");
    };

    let value = default_array_item(path, values);
    values.push(value);
    Ok(())
}

fn default_array_item(path: &str, existing: &[TomlValue]) -> TomlValue {
    if normalize_config_path(path) == ConfigKey::CODEX_APP_SERVER_ARGS {
        return TomlValue::String("app-server".to_string());
    }

    if normalize_config_path(path) == "custom_providers" {
        return default_custom_provider_item(existing);
    }

    existing.first().cloned().unwrap_or_else(|| TomlValue::String(String::new()))
}

fn default_custom_provider_item(existing: &[TomlValue]) -> TomlValue {
    let mut used_names: HashSet<String> = HashSet::new();
    for value in existing {
        let TomlValue::Table(table) = value else {
            continue;
        };
        if let Some(name) = table.get("name").and_then(TomlValue::as_str) {
            used_names.insert(name.to_ascii_lowercase());
        }
    }

    let mut suffix = existing.len().max(1);
    let name = loop {
        let candidate = format!("custom-provider-{suffix}");
        if !used_names.contains(&candidate) {
            break candidate;
        }
        suffix += 1;
    };

    let mut table = toml::map::Map::new();
    table.insert("name".to_string(), TomlValue::String(name));
    table.insert("display_name".to_string(), TomlValue::String(format!("Custom Provider {suffix}")));
    table.insert("base_url".to_string(), TomlValue::String("https://llm.example/v1".to_string()));
    table.insert("api_key_env".to_string(), TomlValue::String(String::new()));
    table.insert("model".to_string(), TomlValue::String(String::new()));
    TomlValue::Table(table)
}

pub(super) fn pop_array_item(root: &mut TomlValue, path: &str) -> Result<()> {
    let node = get_node_mut(root, path).ok_or_else(|| anyhow!("Array path '{path}' was not found"))?;

    let TomlValue::Array(values) = node else {
        bail!("Path '{path}' is not an array");
    };

    if values.pop().is_none() {
        bail!("Array '{path}' is already empty");
    }

    Ok(())
}

pub(super) fn apply_scalar_operation(root: &mut TomlValue, path: &str, operation: ScalarOperation) -> Result<()> {
    let precomputed_cycle_options = matches!(operation, ScalarOperation::CycleNext | ScalarOperation::CyclePrev)
        .then(|| resolve_cycle_options(Some(root), path, ""));
    let Some(node) = get_node_mut(root, path) else {
        return apply_missing_scalar_operation(root, path, operation);
    };

    match node {
        TomlValue::Boolean(value) => {
            if operation != ScalarOperation::Toggle {
                bail!("{path} supports toggle only");
            }
            value.toggle();
            Ok(())
        }
        TomlValue::Integer(value) => {
            match operation {
                ScalarOperation::Increment => *value = value.saturating_add(1),
                ScalarOperation::Decrement => *value = value.saturating_sub(1),
                _ => bail!("{path} supports numeric increment/decrement"),
            }
            Ok(())
        }
        TomlValue::Float(value) => {
            match operation {
                ScalarOperation::Increment => *value += 0.1,
                ScalarOperation::Decrement => *value -= 0.1,
                _ => bail!("{path} supports numeric increment/decrement"),
            }
            Ok(())
        }
        TomlValue::String(current) => {
            let options = precomputed_cycle_options
                .clone()
                .unwrap_or_else(|| resolve_cycle_options(None, path, current));
            if options.is_empty() {
                bail!("{path} has no predefined values to cycle");
            }
            let next = cycle_string_option(current, &options, operation)?;
            *current = next;
            Ok(())
        }
        _ => bail!("{path} is not a scalar setting"),
    }
}

fn apply_missing_scalar_operation(root: &mut TomlValue, path: &str, operation: ScalarOperation) -> Result<()> {
    match operation {
        ScalarOperation::CycleNext | ScalarOperation::CyclePrev => {
            let mut options = resolve_cycle_options(Some(root), path, "");
            if options.is_empty() {
                bail!("Settings path '{path}' was not found");
            }
            options.sort();
            options.dedup();
            let value = match operation {
                ScalarOperation::CycleNext => options.first().cloned(),
                ScalarOperation::CyclePrev => options.last().cloned(),
                _ => None,
            }
            .ok_or_else(|| anyhow!("{path} has no predefined values to cycle"))?;
            insert_missing_string_value(root, path, value)?;
            Ok(())
        }
        _ => bail!("Settings path '{path}' was not found"),
    }
}

fn insert_missing_string_value(root: &mut TomlValue, path: &str, value: String) -> Result<()> {
    let tokens = parse_path_tokens(path)?;
    if tokens.is_empty() {
        bail!("Settings path '{path}' was not found");
    }

    let mut current = root;
    for token in &tokens[..tokens.len() - 1] {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    bail!("Parent path for '{path}' is not a table");
                };
                current = table
                    .entry(key.clone())
                    .or_insert_with(|| TomlValue::Table(toml::map::Map::new()));
            }
            PathToken::Index(_) => bail!("Cannot create missing array path '{path}'"),
        }
    }

    match tokens.last() {
        Some(PathToken::Key(key)) => {
            let TomlValue::Table(table) = current else {
                bail!("Parent path for '{path}' is not a table");
            };
            table.insert(key.clone(), TomlValue::String(value));
            Ok(())
        }
        Some(PathToken::Index(_)) => bail!("Cannot create missing array path '{path}'"),
        None => bail!("Settings path '{path}' was not found"),
    }
}

pub(super) fn resolve_cycle_options(root: Option<&TomlValue>, path: &str, current: &str) -> Vec<String> {
    match normalize_config_path(path).as_str() {
        ConfigKey::CODEX_APP_SERVER_COMMAND => {
            return codex_sidecar_cycle_options(current, "codex");
        }
        "agent.codex_app_server.args[]" => {
            return codex_sidecar_cycle_options(current, "app-server");
        }
        _ => {}
    }

    if normalize_config_path(path) == ConfigKey::THEME {
        return theme::available_themes().into_iter().map(str::to_string).collect();
    }
    if normalize_config_path(path) == ConfigKey::SMALL_MODEL_NAME {
        return lightweight_model_cycle_options(root, current);
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

fn codex_sidecar_cycle_options(current: &str, default: &str) -> Vec<String> {
    let mut options = vec![default.to_string()];
    let trimmed = current.trim();
    if !trimmed.is_empty() && trimmed != default {
        options.push(trimmed.to_string());
    }
    options
}

fn lightweight_model_cycle_options(root: Option<&TomlValue>, current: &str) -> Vec<String> {
    let provider = root
        .and_then(|value| get_node(value, ConfigKey::PROVIDER))
        .and_then(TomlValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(defaults::DEFAULT_PROVIDER);
    let main_model = root
        .and_then(|value| get_node(value, ConfigKey::DEFAULT_MODEL))
        .and_then(TomlValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| model_helpers::default_for(provider).map(str::to_string))
        .unwrap_or_else(|| current.to_string());

    let mut options = vec![String::new(), main_model.clone()];
    options.extend(lightweight_model_choices(provider, &main_model));
    if !current.trim().is_empty() {
        options.push(current.to_string());
    }

    let auto_model = auto_lightweight_model(provider, &main_model);
    let mut deduped = Vec::new();
    for option in options {
        if deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(option.as_str()))
        {
            continue;
        }
        deduped.push(option);
    }

    if let Some(auto_index) = deduped.iter().position(|value| value.eq_ignore_ascii_case(auto_model.as_str())) {
        let auto = deduped.remove(auto_index);
        deduped.insert(1, auto);
    }

    deduped
}

fn cycle_string_option(current: &str, options: &[String], operation: ScalarOperation) -> Result<String> {
    if options.is_empty() {
        bail!("No cycle options available")
    }

    let mut ordered = options.to_vec();
    ordered.sort();
    ordered.dedup();

    let current_index = ordered.iter().position(|entry| entry == current).unwrap_or(0);
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
    let mut draft_value =
        TomlValue::try_from(state.draft.clone()).context("Failed to serialize draft configuration")?;
    mutator(&mut draft_value)?;

    let parsed: VTCodeConfig = draft_value.try_into().context("Updated draft configuration is invalid")?;
    parsed.validate().context("Updated draft configuration failed validation")?;

    state.draft = parsed;
    Ok(())
}

fn write_commented_config(path: &Path, config: &VTCodeConfig) -> Result<()> {
    let content = render_commented_config(config)?;
    VtCodePaths::write_private_file_atomic(path, content.as_bytes())
        .with_context(|| format!("Failed to write configuration file {}", path.display()))
}

fn persist_draft(state: &mut SettingsPaletteState, path: &str) -> Result<()> {
    let draft_value = TomlValue::try_from(state.draft.clone()).context("Failed to serialize draft configuration")?;
    let (target_path, persisted_path, persisted_value, repository_controlled) =
        persistence_target(state, path, &draft_value)?;

    let mut target_value = load_or_default_toml(&target_path)?;
    set_node(&mut target_value, &persisted_path, persisted_value)?;

    let target_config: VTCodeConfig = target_value
        .try_into()
        .with_context(|| format!("Updated configuration at {} could not be deserialized", target_path.display()))?;
    target_config
        .validate()
        .with_context(|| format!("Updated configuration at {} failed validation", target_path.display()))?;

    if repository_controlled {
        ConfigManager::save_repository_config_to_path(&target_path, &target_config)
    } else {
        write_commented_config(&target_path, &target_config)
    }
    .with_context(|| format!("Failed to save {}", target_path.display()))?;
    ConfigManager::invalidate_workspace_cache(&state.workspace);
    Ok(())
}

fn persistence_target(
    state: &SettingsPaletteState,
    path: &str,
    draft: &TomlValue,
) -> Result<(PathBuf, String, TomlValue, bool)> {
    let value = get_node(draft, path)
        .cloned()
        .ok_or_else(|| anyhow!("Settings path '{path}' was not found after applying the change"))?;
    let normalized_path = normalize_config_path(path);

    if explicit_config_path().is_some() {
        return Ok((state.source_path.clone(), path.to_string(), value, false));
    }

    let manager = ConfigManager::load_from_workspace_with_repository_repair(&state.workspace).context(
        if is_trusted_provider_path(path) {
            "Failed to resolve the trusted configuration destination"
        } else {
            "Failed to resolve the configuration destination"
        },
    )?;

    if !is_trusted_provider_path(path) {
        return Ok((
            state.source_path.clone(),
            path.to_string(),
            value,
            manager.is_repository_controlled_path(&state.source_path),
        ));
    }

    let user_path = manager
        .preferred_user_config_path()
        .context("No canonical user configuration path is available for provider settings")?;

    // A custom-provider array is replaced as a unit by the layered loader.
    // Persist the complete effective array so editing a provider can create
    // the trusted layer without manufacturing a partial array.
    if normalized_path == "custom_providers" || normalized_path.starts_with("custom_providers[]") {
        let providers = get_node(draft, "custom_providers")
            .cloned()
            .ok_or_else(|| anyhow!("Settings path 'custom_providers' was not found after applying the change"))?;
        return Ok((user_path, "custom_providers".to_string(), providers, false));
    }

    Ok((user_path, path.to_string(), value, false))
}

fn is_trusted_provider_path(path: &str) -> bool {
    let normalized = normalize_config_path(path);
    if normalized == "custom_providers" || normalized.starts_with("custom_providers[]") {
        return true;
    }

    let mut segments = normalized.split('.');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some("provider_overrides"), Some(_), Some("base_url" | "api_key_env"), None)
    )
}

fn load_or_default_toml(path: &Path) -> Result<TomlValue> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Refusing to read symlinked config file: {}", path.display())
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!("Config path is not a regular file: {}", path.display())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TomlValue::Table(toml::Table::new()));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect config file {}", path.display()));
        }
    }

    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Configuration file {} is not valid UTF-8", path.display()))?;
    toml::from_str(&content).with_context(|| format!("Failed to parse config file {}", path.display()))
}

pub(super) fn render_commented_config(config: &VTCodeConfig) -> Result<String> {
    let value = ConfigManager::sparse_config_value(config)
        .context("Failed to serialize configuration for comment rendering")?;

    let TomlValue::Table(root_table) = value else {
        bail!("Rendered configuration root is not a TOML table")
    };

    let mut output = String::new();
    output.push_str("# VT Code Configuration File\n");
    output.push_str("# Saved from /config with readable section headings.\n");
    output.push_str("# Every field includes descriptions, defaults, and known choices where available.\n\n");

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
        let header = toml_header_path(path)?;
        writeln!(output, "[{header}]").context("Failed to render table header")?;
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
        let _ = writeln!(output, "# {line}");
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
        Some(prefix) if !prefix.is_empty() => path_with_key(prefix, key),
        _ => path_with_key("", key),
    }
}

fn toml_header_path(path: &str) -> Result<String> {
    let tokens = parse_path_tokens(path).with_context(|| format!("Invalid TOML table path '{path}'"))?;
    let mut header = String::new();

    for token in tokens {
        match token {
            PathToken::Key(key) => {
                if !header.is_empty() {
                    header.push('.');
                }
                header.push_str(&toml_key_component(&key));
            }
            PathToken::Index(index) => {
                bail!("Cannot render array item {index} as a TOML table header: {path}");
            }
        }
    }

    if header.is_empty() {
        bail!("Cannot render an empty TOML table header for path '{path}'");
    }
    Ok(header)
}

fn toml_key_component(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return key.to_string();
    }

    let mut escaped = String::with_capacity(key.len());
    for character in key.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\u{0008}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{000C}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            character if character.is_control() => {
                let _ = write!(escaped, "\\u{:04X}", character as u32);
            }
            character => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}
