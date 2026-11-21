use anyhow::{Context, Result, anyhow};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use tempfile::Builder;

use vtcode_auth::AuthCredentialsStoreMode;
use vtcode_commons::provider::Provider;

use crate::api_keys::store_credential_with_mode;

/// Returns the workspace `.env` file path.
pub fn workspace_env_path(workspace: &Path) -> PathBuf {
    workspace.join(".env")
}

/// Display-friendly representation of the workspace `.env` path.
pub fn workspace_env_path_display(workspace: &Path) -> String {
    workspace_env_path(workspace).display().to_string()
}

pub fn read_workspace_env_value(workspace: &Path, env_key: &str) -> Result<Option<String>> {
    let env_path = workspace.join(".env");
    let iter = match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => iter,
        Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(err) => {
            return Err(anyhow!(err).context(format!("Failed to read {}", env_path.display())));
        }
    };

    for item in iter {
        let (key, value) = item
            .map_err(|err: dotenvy::Error| anyhow!(err))
            .with_context(|| format!("Failed to parse {}", env_path.display()))?;
        if key == env_key {
            if value.trim().is_empty() {
                return Ok(None);
            }
            return Ok(Some(value));
        }
    }

    Ok(None)
}

/// Read multiple env keys from workspace `.env` in a single pass.
///
/// Returns a map of key -> value for all found keys. Missing keys are
/// omitted from the map.
pub fn read_workspace_env_values(
    workspace: &Path,
    env_keys: &[&str],
) -> Result<std::collections::HashMap<String, String>> {
    let env_path = workspace.join(".env");
    let iter = match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => iter,
        Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(std::collections::HashMap::new());
        }
        Err(err) => {
            return Err(anyhow!(err).context(format!("Failed to read {}", env_path.display())));
        }
    };

    let wanted: std::collections::HashSet<String> = env_keys.iter().map(|&s| s.to_string()).collect();
    let mut found: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for item in iter {
        let (key, value) = item
            .map_err(|err: dotenvy::Error| anyhow!(err))
            .with_context(|| format!("Failed to parse {}", env_path.display()))?;
        if wanted.contains(&key) && !value.trim().is_empty() {
            found.insert(key, value);
        }
    }

    Ok(found)
}

pub fn remove_workspace_env_value(workspace: &Path, key: &str) -> Result<()> {
    let env_path = workspace.join(".env");
    let mut lines = read_existing_lines(&env_path)?;
    let length = lines.len();
    lines.retain(|line| {
        let Some((name, _)) = line.split_once('=') else {
            return true;
        };
        let name = name.trim();
        let name = name
            .strip_prefix("export")
            .filter(|suffix| suffix.starts_with(char::is_whitespace))
            .map(str::trim_start)
            .unwrap_or(name);
        name != key
    });
    if lines.len() == length {
        return Ok(());
    }

    let parent = env_path.parent().unwrap_or(workspace);
    fs::create_dir_all(parent).with_context(|| format!("Failed to create directory {}", parent.display()))?;

    let temp = Builder::new()
        .prefix(".env.")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

    set_private_permissions(temp.as_file(), temp.path())?;

    {
        let mut writer = BufWriter::new(temp.as_file());
        for line in &lines {
            writeln!(writer, "{line}").with_context(|| format!("Failed to write .env entry for {key}"))?;
        }
        writer
            .flush()
            .with_context(|| format!("Failed to flush temporary .env for {key}"))?;
    }

    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary .env for {key}"))?;

    let _persisted = temp
        .persist(&env_path)
        .with_context(|| format!("Failed to persist {}", env_path.display()))?;

    set_private_path_permissions(&env_path)?;
    Ok(())
}

pub fn write_workspace_env_value(workspace: &Path, key: &str, value: &str) -> Result<()> {
    let env_path = workspace.join(".env");
    let mut lines = read_existing_lines(&env_path)?;
    upsert_env_line(&mut lines, key, value);

    let parent = env_path.parent().unwrap_or(workspace);
    fs::create_dir_all(parent).with_context(|| format!("Failed to create directory {}", parent.display()))?;

    let temp = Builder::new()
        .prefix(".env.")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

    set_private_permissions(temp.as_file(), temp.path())?;

    {
        let mut writer = BufWriter::new(temp.as_file());
        for line in &lines {
            writeln!(writer, "{line}").with_context(|| format!("Failed to write .env entry for {key}"))?;
        }
        writer
            .flush()
            .with_context(|| format!("Failed to flush temporary .env for {key}"))?;
    }

    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary .env for {key}"))?;

    let _persisted = temp
        .persist(&env_path)
        .with_context(|| format!("Failed to persist {}", env_path.display()))?;

    set_private_path_permissions(&env_path)?;
    Ok(())
}

fn read_existing_lines(env_path: &Path) -> Result<Vec<String>> {
    if !env_path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(env_path).with_context(|| format!("Failed to read {}", env_path.display()))?;
    Ok(contents.lines().map(|line| line.to_string()).collect())
}

fn upsert_env_line(lines: &mut Vec<String>, key: &str, value: &str) {
    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some((existing_key, _)) = line.split_once('=')
            && existing_key.trim() == key
        {
            *line = format!("{key}={value}");
            replaced = true;
        }
    }

    if !replaced {
        lines.push(format!("{key}={value}"));
    }
}

#[cfg(unix)]
fn set_private_permissions(file: &File, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_file: &File, _path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_path_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_path_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[derive(Debug, Default)]
pub struct MigrationSummary {
    pub migrated: u32,
    pub skipped: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum MigrationOutcome {
    Migrated,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy)]
pub enum SkipReason {
    LocalProvider,
    ManagedAuth,
    NoEnvKeyDefined,
    NotFoundInEnv,
    EmptyValue,
}

/// Migrate a single provider's API key from workspace `.env` into secure storage.
///
/// Returns the outcome so callers can render progress in their own UI. The
/// provider is skipped if it is local, uses managed auth, has no env key
/// defined, or has no value in `.env`.
pub fn migrate_single_env_key(
    workspace: &Path,
    provider: Provider,
    store_mode: AuthCredentialsStoreMode,
    value: Option<&str>,
) -> Result<MigrationOutcome> {
    if provider.is_local() || provider.uses_managed_auth() {
        return Ok(MigrationOutcome::Skipped);
    }

    let env_key = provider.default_api_key_env();
    if env_key.is_empty() {
        return Ok(MigrationOutcome::Skipped);
    }

    let raw: String = match value {
        Some(v) => v.to_owned(),
        None => match read_workspace_env_value(workspace, env_key) {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(MigrationOutcome::Skipped),
            Err(err) => {
                tracing::warn!("Failed to read {} from {}: {}", env_key, workspace.join(".env").display(), err);
                return Ok(MigrationOutcome::Failed);
            }
        },
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(MigrationOutcome::Skipped);
    }

    match store_credential_with_mode(provider.as_ref(), env_key, trimmed, store_mode) {
        Ok(Some(_)) => match remove_workspace_env_value(workspace, env_key) {
            Ok(()) => Ok(MigrationOutcome::Migrated),
            Err(err) => {
                tracing::warn!("Stored {} in keyring but failed to remove from .env: {}", env_key, err);
                Ok(MigrationOutcome::Failed)
            }
        },
        Ok(None) => Ok(MigrationOutcome::Skipped),
        Err(err) => {
            tracing::warn!("Failed to store API key for {}: {}", provider.label(), err);
            Ok(MigrationOutcome::Failed)
        }
    }
}

/// Migrate API keys for multiple providers from workspace `.env` into secure
/// storage.
///
/// This is the batch-optimized path: the `.env` file is read once, all
/// providers are processed in memory, and the file is rewritten at most once.
pub fn migrate_workspace_env_keys(
    workspace: &Path,
    providers: &[Provider],
    store_mode: AuthCredentialsStoreMode,
) -> Result<(MigrationSummary, Vec<(Provider, MigrationOutcome)>)> {
    let env_path = workspace.join(".env");
    if !env_path.exists() {
        return Ok((MigrationSummary::default(), Vec::new()));
    }

    let mut lines = read_existing_lines(&env_path)?;
    let mut line_map: std::collections::HashMap<&str, usize> = std::collections::HashMap::with_capacity(lines.len());
    for (idx, line) in lines.iter().enumerate() {
        if let Some((key, _)) = line.split_once('=') {
            line_map.insert(key.trim(), idx);
        }
    }

    let mut summary = MigrationSummary::default();
    let mut outcomes = Vec::with_capacity(providers.len());
    let mut removed = std::collections::HashSet::new();

    for &provider in providers {
        if provider.is_local() || provider.uses_managed_auth() {
            summary.skipped += 1;
            outcomes.push((provider, MigrationOutcome::Skipped));
            continue;
        }

        let env_key = provider.default_api_key_env();
        if env_key.is_empty() {
            summary.skipped += 1;
            outcomes.push((provider, MigrationOutcome::Skipped));
            continue;
        }

        let value = match line_map
            .get(env_key)
            .and_then(|&idx| lines.get(idx))
            .and_then(|line| line.split_once('='))
            .map(|(_, v)| v.trim())
        {
            Some(v) if !v.is_empty() => v,
            _ => {
                summary.skipped += 1;
                outcomes.push((provider, MigrationOutcome::Skipped));
                continue;
            }
        };

        match store_credential_with_mode(provider.as_ref(), env_key, value, store_mode) {
            Ok(Some(_)) => {
                if let Some(&idx) = line_map.get(env_key) {
                    removed.insert(idx);
                }
                summary.migrated += 1;
                outcomes.push((provider, MigrationOutcome::Migrated));
            }
            Ok(None) => {
                summary.skipped += 1;
                outcomes.push((provider, MigrationOutcome::Skipped));
            }
            Err(err) => {
                tracing::warn!("Failed to store API key for {}: {}", provider.label(), err);
                summary.failed += 1;
                outcomes.push((provider, MigrationOutcome::Failed));
            }
        }
    }

    if !removed.is_empty() {
        let mut new_lines = Vec::with_capacity(lines.len() - removed.len());
        for (idx, line) in lines.into_iter().enumerate() {
            if !removed.contains(&idx) {
                new_lines.push(line);
            }
        }
        lines = new_lines;

        let parent = env_path.parent().unwrap_or(workspace);
        fs::create_dir_all(parent).with_context(|| format!("Failed to create directory {}", parent.display()))?;

        let temp = Builder::new()
            .prefix(".env.")
            .suffix(".tmp")
            .tempfile_in(parent)
            .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

        set_private_permissions(temp.as_file(), temp.path())?;

        {
            let mut writer = BufWriter::new(temp.as_file());
            for line in &lines {
                writeln!(writer, "{line}").with_context(|| "Failed to write .env entry for migration")?;
            }
            writer.flush().with_context(|| "Failed to flush temporary .env for migration")?;
        }

        temp.as_file()
            .sync_all()
            .with_context(|| "Failed to sync temporary .env for migration")?;

        let _persisted = temp
            .persist(&env_path)
            .with_context(|| format!("Failed to persist {}", env_path.display()))?;

        set_private_path_permissions(&env_path)?;
    }

    Ok((summary, outcomes))
}

#[cfg(test)]
mod tests {
    use super::{read_workspace_env_value, remove_workspace_env_value, write_workspace_env_value};
    use anyhow::Result;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn read_returns_value_when_present() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "OPENAI_API_KEY=sk-test\n")?;

        let value = read_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        assert_eq!(value, Some("sk-test".to_string()));
        Ok(())
    }

    #[test]
    fn read_returns_none_when_missing() -> Result<()> {
        let dir = tempdir()?;

        let value = read_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        assert_eq!(value, None);
        Ok(())
    }

    #[test]
    fn write_adds_new_key() -> Result<()> {
        let dir = tempdir()?;

        write_workspace_env_value(dir.path(), "OPENAI_API_KEY", "sk-test")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OPENAI_API_KEY=sk-test\n");
        Ok(())
    }

    #[test]
    fn write_replaces_existing_key() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "OPENAI_API_KEY=old-value\nOTHER_KEY=value\n")?;

        write_workspace_env_value(dir.path(), "OPENAI_API_KEY", "new-value")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OPENAI_API_KEY=new-value\nOTHER_KEY=value\n");
        Ok(())
    }

    #[test]
    fn remove_deletes_existing_key() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "export OPENAI_API_KEY=sk-test\nOTHER_KEY=value\n")?;

        remove_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OTHER_KEY=value\n");
        Ok(())
    }

    #[test]
    fn remove_is_idempotent_when_key_absent() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "OTHER_KEY=value\n")?;

        remove_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OTHER_KEY=value\n");
        Ok(())
    }

    #[test]
    fn remove_cleans_up_empty_file() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "OPENAI_API_KEY=sk-test\n")?;

        remove_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "");
        Ok(())
    }
}
