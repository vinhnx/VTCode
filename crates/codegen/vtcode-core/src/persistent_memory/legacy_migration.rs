use super::*;

pub(crate) fn persistent_memory_base_dir(config: &PersistentMemoryConfig) -> Result<PathBuf> {
    if let Some(override_dir) = config.directory_override.as_deref() {
        if let Some(stripped) = override_dir.strip_prefix("~/") {
            return Ok(dirs::home_dir()
                .context("Could not resolve home directory")?
                .join(stripped));
        }
        return Ok(PathBuf::from(override_dir));
    }
    dirs::home_dir()
        .map(|home| home.join(".vtcode"))
        .context("Could not resolve VT Code home directory")
}

pub(crate) fn persistent_memory_project_name(workspace_root: &Path) -> String {
    ConfigManager::current_project_name(workspace_root)
        .or_else(|| workspace_root.file_name().and_then(|v| v.to_str()).map(|v| v.to_string()))
        .unwrap_or_else(|| "workspace".to_string())
}

pub(crate) fn migrate_legacy_persistent_memory_dir_if_needed(
    config: &PersistentMemoryConfig,
    project_name: &str,
    target_dir: &Path,
) -> Result<()> {
    if config.directory_override.is_some() {
        return Ok(());
    }
    let Some(legacy_dir) = legacy_persistent_memory_dir(project_name)? else {
        return Ok(());
    };
    if legacy_dir == target_dir || !legacy_dir.exists() {
        return Ok(());
    }
    migrate_legacy_memory_dir(&legacy_dir, target_dir)
}

pub(super) fn migrate_legacy_memory_dir(legacy_dir: &Path, target_dir: &Path) -> Result<()> {
    if target_dir.exists() && memory_directory_has_stored_content(target_dir)? {
        if !memory_directory_has_stored_content(legacy_dir)? {
            remove_empty_legacy_memory_hierarchy(legacy_dir)?;
        }
        return Ok(());
    }
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)
            .with_context(|| format!("Failed to clear {}", target_dir.display()))?;
    }
    let target_parent =
        target_dir.parent().context("Persistent memory directory is missing a parent")?;
    std::fs::create_dir_all(target_parent)
        .with_context(|| format!("Failed to create {}", target_parent.display()))?;
    std::fs::rename(legacy_dir, target_dir).with_context(|| {
        format!(
            "Failed to migrate persistent memory from {} to {}",
            legacy_dir.display(),
            target_dir.display()
        )
    })?;
    remove_empty_legacy_memory_hierarchy(legacy_dir)?;
    Ok(())
}

fn legacy_persistent_memory_dir(project_name: &str) -> Result<Option<PathBuf>> {
    let Some(legacy_base) = get_config_dir() else {
        return Ok(None);
    };
    let current_base = dirs::home_dir()
        .map(|home| home.join(".vtcode"))
        .context("Could not resolve VT Code home directory")?;
    if legacy_base == current_base {
        return Ok(None);
    }
    Ok(Some(
        legacy_base
            .join("projects")
            .join(sanitize_project_name(project_name))
            .join("memory"),
    ))
}

fn memory_directory_has_stored_content(directory: &Path) -> Result<bool> {
    if !directory.exists() {
        return Ok(false);
    }
    for path in [
        directory.join(PREFERENCES_FILENAME),
        directory.join(REPOSITORY_FACTS_FILENAME),
    ] {
        if !path.exists() {
            continue;
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() {
            return Ok(true);
        }
    }
    let rollout_dir = directory.join(ROLLOUT_SUMMARIES_DIRNAME);
    if !rollout_dir.exists() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(&rollout_dir)
        .with_context(|| format!("Failed to list {}", rollout_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if !parse_topic_file(&contents).is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remove_empty_legacy_memory_hierarchy(legacy_memory_dir: &Path) -> Result<()> {
    let mut current = legacy_memory_dir.parent();
    for _ in 0..3 {
        let Some(path) = current else { break };
        match std::fs::remove_dir(path) {
            Ok(()) => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => current = path.parent(),
            Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(err) => {
                return Err(err).with_context(|| format!("Failed to remove {}", path.display()));
            }
        }
    }
    Ok(())
}

#[cold]
pub(crate) fn sanitize_project_name(project_name: &str) -> String {
    let sanitized: String = project_name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '_',
            other => other,
        })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}
