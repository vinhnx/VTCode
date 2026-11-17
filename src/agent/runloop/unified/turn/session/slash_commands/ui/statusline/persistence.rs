use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml::Value as TomlValue;
use vtcode_core::config::StatusLineConfig;
use vtcode_tui_runtime::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::StatuslineTargetMode;
use crate::agent::runloop::unified::palettes::refresh_runtime_config_from_manager;
use crate::agent::runloop::unified::turn::session::slash_commands::SlashCommandContext;

use super::super::super::config_toml::{
    ensure_child_table, load_private_toml_value, load_toml_value, save_private_toml_value, save_toml_value,
};
use super::config::statusline_mode_id;
use vtcode_commons::VtCodePaths;

const STATUSLINE_SCRIPT_FILE_NAME: &str = "statusline.sh";
const STATUSLINE_SCRIPT_TEMPLATE: &str = r#"#!/bin/sh
payload="$(cat)"
branch="$(printf '%s' "$payload" | jq -r '.git.branch // ""' 2>/dev/null)"
dirty="$(printf '%s' "$payload" | jq -r '.git.dirty // false' 2>/dev/null)"
model="$(printf '%s' "$payload" | jq -r '.model.display_name // .model.id // ""' 2>/dev/null)"
reasoning="$(printf '%s' "$payload" | jq -r '.runtime.reasoning_effort // ""' 2>/dev/null)"

git_part=""
if [ -n "$branch" ]; then
  git_part="$branch"
  if [ "$dirty" = "true" ]; then
    git_part="$git_part*"
  fi
fi

model_part="$model"
if [ -n "$reasoning" ]; then
  model_part="$model_part ($reasoning)"
fi

if [ -n "$git_part" ] && [ -n "$model_part" ]; then
  printf '%s | %s\n' "$git_part" "$model_part"
elif [ -n "$git_part" ]; then
  printf '%s\n' "$git_part"
elif [ -n "$model_part" ]; then
  printf '%s\n' "$model_part"
fi
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScriptScaffoldResult {
    Created,
    Replaced,
    SkippedExisting,
}

pub(super) async fn persist_statusline_config(
    ctx: &mut SlashCommandContext<'_>,
    config_path: &Path,
    draft: StatusLineConfig,
    target: StatuslineTargetMode,
    script_path: &Path,
) -> Result<()> {
    write_statusline_config(config_path, &draft, target)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await?;

    if target == StatuslineTargetMode::Workspace {
        let command = draft.command.as_deref().map(str::trim).unwrap_or_default().to_string();
        if command == default_script_command(target, script_path) && !statusline_script_exists(script_path) {
            ctx.renderer.line(
                MessageStyle::Warning,
                "Saved command path points to a missing script. Use \"Create script template\" to scaffold it.",
            )?;
        }
    }

    Ok(())
}

fn write_statusline_config(config_path: &Path, draft: &StatusLineConfig, target: StatuslineTargetMode) -> Result<()> {
    let mut root = match target {
        StatuslineTargetMode::User => load_private_toml_value(config_path)?,
        StatuslineTargetMode::Workspace => load_toml_value(config_path)?,
    };
    let root_table = root.as_table_mut().context("Status line config root is not a TOML table")?;
    let ui_table = ensure_child_table(root_table, "ui");
    let status_table = ensure_child_table(ui_table, "status_line");

    status_table.insert("mode".to_string(), TomlValue::String(statusline_mode_id(&draft.mode).to_string()));
    status_table.insert("show_clock".to_string(), TomlValue::Boolean(draft.show_clock));
    if let Some(command) = draft.command.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        status_table.insert("command".to_string(), TomlValue::String(command.to_string()));
    } else {
        status_table.remove("command");
    }
    status_table.insert(
        "refresh_interval_ms".to_string(),
        TomlValue::Integer(u64_to_toml_integer(draft.refresh_interval_ms, "refresh_interval_ms")?),
    );
    status_table.insert(
        "command_timeout_ms".to_string(),
        TomlValue::Integer(u64_to_toml_integer(draft.command_timeout_ms, "command_timeout_ms")?),
    );

    match target {
        StatuslineTargetMode::User => save_private_toml_value(config_path, &root),
        StatuslineTargetMode::Workspace => save_toml_value(config_path, &root),
    }
}

fn u64_to_toml_integer(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{label} is too large to persist"))
}

pub(super) fn statusline_script_path(target: StatuslineTargetMode, workspace: &Path, config_path: &Path) -> PathBuf {
    match target {
        StatuslineTargetMode::Workspace => workspace.join(".vtcode").join(STATUSLINE_SCRIPT_FILE_NAME),
        StatuslineTargetMode::User => config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| workspace.to_path_buf())
            .join(STATUSLINE_SCRIPT_FILE_NAME),
    }
}

pub(super) fn default_script_command(target: StatuslineTargetMode, script_path: &Path) -> String {
    match target {
        StatuslineTargetMode::Workspace => ".vtcode/statusline.sh".to_string(),
        StatuslineTargetMode::User => shell_quote(script_path),
    }
}

fn shell_quote(path: &Path) -> String {
    let path = path.to_string_lossy();
    format!("'{}'", path.replace('\'', "'\\''"))
}

pub(super) fn scaffold_statusline_script(
    target: StatuslineTargetMode,
    path: &Path,
    replace_existing: bool,
) -> Result<ScriptScaffoldResult> {
    let existing = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("Refusing to use symlinked status line script {}", path.display());
        }
        Ok(metadata) if !metadata.is_file() => {
            anyhow::bail!("Status line script path is not a regular file: {}", path.display());
        }
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    };
    if existing && !replace_existing {
        return Ok(ScriptScaffoldResult::SkippedExisting);
    }

    if let Some(parent) = path.parent() {
        if target == StatuslineTargetMode::User {
            VtCodePaths::ensure_user_dir(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
        } else {
            fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    let result = if existing {
        ScriptScaffoldResult::Replaced
    } else {
        ScriptScaffoldResult::Created
    };
    if target == StatuslineTargetMode::User {
        VtCodePaths::write_private_file_atomic(path, STATUSLINE_SCRIPT_TEMPLATE.as_bytes())
            .with_context(|| format!("Failed to write {}", path.display()))?;
    } else {
        fs::write(path, STATUSLINE_SCRIPT_TEMPLATE).with_context(|| format!("Failed to write {}", path.display()))?;
    }
    set_executable(path)?;
    Ok(result)
}

pub(super) fn statusline_script_exists(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("Failed to read metadata for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("Failed to set executable bit on {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}
