use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml::Value as TomlValue;
use vtcode_core::config::current_config_defaults;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::loader::layers::ConfigLayerSource;

use crate::agent::runloop::unified::palettes::refresh_runtime_config_from_manager;

use super::super::super::config_toml::{
    ensure_child_table, load_toml_value, preferred_workspace_config_path, save_toml_value,
};
use super::SlashCommandContext;

pub(super) async fn persist_workspace_config_change<F>(
    ctx: &mut SlashCommandContext<'_>,
    update: F,
) -> Result<()>
where
    F: FnOnce(&mut toml::map::Map<String, TomlValue>) -> Result<()>,
{
    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let workspace_config_path = preferred_workspace_config_path(&manager, &ctx.config.workspace);
    let mut root = load_toml_value(&workspace_config_path)?;
    let root_table = root
        .as_table_mut()
        .context("Workspace config root is not a TOML table")?;
    update(root_table)?;
    save_toml_value(&workspace_config_path, &root)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await
}

pub(super) async fn persist_user_directory_override(
    ctx: &mut SlashCommandContext<'_>,
    value: Option<String>,
) -> Result<()> {
    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let user_config_path =
        preferred_user_config_path(&manager).context("Could not resolve user config path")?;
    write_user_directory_override(&user_config_path, value)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await
}

pub(super) fn write_user_directory_override(path: &Path, value: Option<String>) -> Result<()> {
    let mut root = load_toml_value(path)?;

    let root_table = root
        .as_table_mut()
        .context("User config root is not a TOML table")?;
    match value {
        Some(value) if !value.trim().is_empty() => {
            let agent_table = ensure_child_table(root_table, "agent");
            let memory_table = ensure_child_table(agent_table, "persistent_memory");
            memory_table.insert("directory_override".to_string(), TomlValue::String(value));
        }
        _ => {
            let remove_memory_table = {
                let agent_table = ensure_child_table(root_table, "agent");
                let memory_table = ensure_child_table(agent_table, "persistent_memory");
                memory_table.remove("directory_override");
                memory_table.is_empty()
            };
            if remove_memory_table {
                let remove_agent_table = {
                    let agent_table = ensure_child_table(root_table, "agent");
                    agent_table.remove("persistent_memory");
                    agent_table.is_empty()
                };
                if remove_agent_table {
                    root_table.remove("agent");
                }
            }
        }
    }

    save_toml_value(path, &root)
}

pub(super) fn set_workspace_memory_enabled(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: bool,
) {
    let features_table = ensure_child_table(root_table, "features");
    features_table.insert("memories".to_string(), TomlValue::Boolean(value));

    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert("enabled".to_string(), TomlValue::Boolean(value));
}

pub(super) fn set_workspace_memory_auto_write(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: bool,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert("auto_write".to_string(), TomlValue::Boolean(value));
}

pub(super) fn set_workspace_memory_line_limit(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert(
        "startup_line_limit".to_string(),
        TomlValue::Integer(usize_to_toml_integer(value, "startup_line_limit")?),
    );
    Ok(())
}

pub(super) fn set_workspace_memory_byte_limit(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert(
        "startup_byte_limit".to_string(),
        TomlValue::Integer(usize_to_toml_integer(value, "startup_byte_limit")?),
    );
    Ok(())
}

pub(super) fn set_workspace_instruction_import_depth(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    agent_table.insert(
        "instruction_import_max_depth".to_string(),
        TomlValue::Integer(usize_to_toml_integer(
            value,
            "instruction_import_max_depth",
        )?),
    );
    Ok(())
}

pub(super) fn set_workspace_instruction_excludes(
    root_table: &mut toml::map::Map<String, TomlValue>,
    values: Vec<String>,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    agent_table.insert(
        "instruction_excludes".to_string(),
        TomlValue::Array(values.into_iter().map(TomlValue::String).collect()),
    );
}

pub(super) fn set_workspace_small_model_for_memory(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: bool,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    let small_model_table = ensure_child_table(agent_table, "small_model");
    small_model_table.insert("use_for_memory".to_string(), TomlValue::Boolean(value));
}

pub(super) fn set_workspace_small_model_model(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: String,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    let small_model_table = ensure_child_table(agent_table, "small_model");
    small_model_table.insert("model".to_string(), TomlValue::String(value));
}

fn usize_to_toml_integer(value: usize, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{} is too large to persist", label))
}

pub(super) fn preferred_user_config_path(manager: &ConfigManager) -> Option<PathBuf> {
    manager
        .layer_stack()
        .layers()
        .iter()
        .rev()
        .find_map(|layer| match &layer.source {
            ConfigLayerSource::User { file } if layer.is_enabled() => Some(file.clone()),
            _ => None,
        })
        .or_else(|| {
            let defaults = current_config_defaults();
            defaults
                .home_config_paths(manager.config_file_name())
                .into_iter()
                .next()
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(manager.config_file_name())))
}

pub(super) fn parse_positive_usize(value: &str, label: &str) -> Result<usize> {
    let parsed = value
        .trim()
        .parse::<usize>()
        .with_context(|| format!("Failed to parse {}", label))?;
    if parsed == 0 {
        bail!("{} must be greater than 0", label);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_user_directory_override_removes_empty_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.toml");

        write_user_directory_override(&path, Some("/tmp/memory".to_string())).expect("write");
        assert!(path.exists());

        write_user_directory_override(&path, None).expect("clear");
        assert!(!path.exists());
    }

    #[test]
    fn workspace_memory_settings_preserve_unrelated_keys() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("vtcode.toml");
        std::fs::write(
            &path,
            "[agent]\ntheme = \"ciapre\"\n[agent.small_model]\nmodel = \"gpt-5-mini\"\n",
        )
        .expect("seed config");

        let mut root = load_toml_value(&path).expect("load config");
        let root_table = root.as_table_mut().expect("root table");
        set_workspace_memory_enabled(root_table, false);
        set_workspace_memory_auto_write(root_table, false);
        set_workspace_memory_line_limit(root_table, 111).expect("line limit");
        set_workspace_memory_byte_limit(root_table, 222).expect("byte limit");
        set_workspace_instruction_import_depth(root_table, 7).expect("import depth");
        set_workspace_instruction_excludes(
            root_table,
            vec!["**/other-team/.vtcode/rules/**".to_string()],
        );
        set_workspace_small_model_for_memory(root_table, false);
        save_toml_value(&path, &root).expect("save config");

        let saved = load_toml_value(&path).expect("reload config");
        let agent = saved
            .get("agent")
            .and_then(TomlValue::as_table)
            .expect("agent table");
        assert_eq!(
            agent.get("theme").and_then(TomlValue::as_str),
            Some("ciapre")
        );
        assert!(agent.get("provider").is_none());
        assert_eq!(
            agent
                .get("instruction_import_max_depth")
                .and_then(TomlValue::as_integer),
            Some(7)
        );
        assert_eq!(
            agent
                .get("instruction_excludes")
                .and_then(TomlValue::as_array)
                .map(|entries| entries.len()),
            Some(1)
        );

        let memory = agent
            .get("persistent_memory")
            .and_then(TomlValue::as_table)
            .expect("persistent memory table");
        assert_eq!(
            memory.get("enabled").and_then(TomlValue::as_bool),
            Some(false)
        );
        assert_eq!(
            memory.get("auto_write").and_then(TomlValue::as_bool),
            Some(false)
        );
        assert_eq!(
            memory
                .get("startup_line_limit")
                .and_then(TomlValue::as_integer),
            Some(111)
        );
        assert_eq!(
            memory
                .get("startup_byte_limit")
                .and_then(TomlValue::as_integer),
            Some(222)
        );

        let small_model = agent
            .get("small_model")
            .and_then(TomlValue::as_table)
            .expect("small model table");
        assert_eq!(
            small_model.get("model").and_then(TomlValue::as_str),
            Some("gpt-5-mini")
        );
        assert_eq!(
            small_model
                .get("use_for_memory")
                .and_then(TomlValue::as_bool),
            Some(false)
        );
    }
}
