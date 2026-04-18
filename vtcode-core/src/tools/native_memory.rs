use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};

use crate::config::loader::VTCodeConfig;
use crate::config::PersistentMemoryConfig;
use crate::persistent_memory::{
    rebuild_generated_memory_files, resolve_persistent_memory_dir, scaffold_persistent_memory,
};

const MEMORIES_ROOT: &str = "/memories";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeMemoryCommand {
    View,
    Create,
    StrReplace,
    Insert,
    Delete,
    Rename,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeMemoryRequest {
    pub command: NativeMemoryCommand,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub old_path: Option<String>,
    #[serde(default)]
    pub new_path: Option<String>,
    #[serde(default)]
    pub file_text: Option<String>,
    #[serde(default)]
    pub old_str: Option<String>,
    #[serde(default)]
    pub new_str: Option<String>,
    #[serde(default)]
    pub insert_line: Option<usize>,
    #[serde(default)]
    pub insert_text: Option<String>,
}

pub fn parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "enum": ["view", "create", "str_replace", "insert", "delete", "rename"],
                "description": "Memory operation to perform."
            },
            "path": {
                "type": "string",
                "description": "Path under /memories for view/create/str_replace/insert/delete."
            },
            "old_path": {
                "type": "string",
                "description": "Existing path under /memories for rename."
            },
            "new_path": {
                "type": "string",
                "description": "Destination path under /memories for rename."
            },
            "file_text": {
                "type": "string",
                "description": "Full file contents for create."
            },
            "old_str": {
                "type": "string",
                "description": "Exact substring to replace for str_replace."
            },
            "new_str": {
                "type": "string",
                "description": "Replacement string for str_replace."
            },
            "insert_line": {
                "type": "integer",
                "minimum": 0,
                "description": "Zero-based line index for insert."
            },
            "insert_text": {
                "type": "string",
                "description": "Text to insert at insert_line."
            }
        },
        "required": ["command"],
        "additionalProperties": false
    })
}

pub async fn execute(
    workspace_root: &Path,
    config: &PersistentMemoryConfig,
    args: Value,
) -> Result<Value> {
    let request: NativeMemoryRequest =
        serde_json::from_value(args).context("Invalid memory tool arguments")?;
    let root = prepare_root(workspace_root, config).await?;
    let output = execute_request(&root, workspace_root, config, request).await?;
    Ok(Value::String(output))
}

pub async fn execute_with_vt_config(
    workspace_root: &Path,
    vt_cfg: &VTCodeConfig,
    args: Value,
) -> Result<Value> {
    if !vt_cfg.persistent_memory_enabled() {
        bail!(
            "Persistent memory is disabled. Enable features.memories and agent.persistent_memory.enabled to use /memories"
        );
    }

    execute(workspace_root, &vt_cfg.agent.persistent_memory, args).await
}

async fn prepare_root(workspace_root: &Path, config: &PersistentMemoryConfig) -> Result<PathBuf> {
    scaffold_persistent_memory(config, workspace_root)
        .await
        .context("Failed to scaffold persistent memory layout")?;
    resolve_persistent_memory_dir(config, workspace_root)?
        .ok_or_else(|| anyhow!("Persistent memory directory could not be resolved"))
}

async fn execute_request(
    root: &Path,
    workspace_root: &Path,
    config: &PersistentMemoryConfig,
    request: NativeMemoryRequest,
) -> Result<String> {
    match request.command {
        NativeMemoryCommand::View => {
            let path = request.path.as_deref().unwrap_or(MEMORIES_ROOT);
            view(root, path).await
        }
        NativeMemoryCommand::Create => {
            let path = required_text(request.path.as_deref(), "path")?;
            let file_text = required_text(request.file_text.as_deref(), "file_text")?;
            let resolved = resolve_virtual_path(root, path)?;
            ensure_writable_path(&resolved.relative, path)?;
            if let Some(parent) = resolved.absolute.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            tokio::fs::write(&resolved.absolute, file_text)
                .await
                .with_context(|| format!("Failed to write {}", path))?;
            rebuild_generated_memory_files(config, workspace_root).await?;
            Ok(format!("Created {path}"))
        }
        NativeMemoryCommand::StrReplace => {
            let path = required_text(request.path.as_deref(), "path")?;
            let old_str = required_text(request.old_str.as_deref(), "old_str")?;
            let new_str = request.new_str.as_deref().unwrap_or_default();
            if old_str.is_empty() {
                bail!("old_str must not be empty");
            }
            let resolved = resolve_virtual_path(root, path)?;
            ensure_writable_path(&resolved.relative, path)?;
            let content = tokio::fs::read_to_string(&resolved.absolute)
                .await
                .with_context(|| format!("Failed to read {}", path))?;
            let matches = content.matches(old_str).count();
            if matches == 0 {
                bail!("old_str not found in {path}");
            }
            if matches > 1 {
                bail!("old_str appears {matches} times in {path}; be more specific");
            }
            tokio::fs::write(&resolved.absolute, content.replacen(old_str, new_str, 1))
                .await
                .with_context(|| format!("Failed to write {}", path))?;
            rebuild_generated_memory_files(config, workspace_root).await?;
            Ok(format!("Replaced in {path}"))
        }
        NativeMemoryCommand::Insert => {
            let path = required_text(request.path.as_deref(), "path")?;
            let insert_line = request
                .insert_line
                .ok_or_else(|| anyhow!("insert_line is required"))?;
            let insert_text = required_text(request.insert_text.as_deref(), "insert_text")?;
            let resolved = resolve_virtual_path(root, path)?;
            ensure_writable_path(&resolved.relative, path)?;
            let content = tokio::fs::read_to_string(&resolved.absolute)
                .await
                .with_context(|| format!("Failed to read {}", path))?;
            let mut lines = content
                .split('\n')
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            if insert_line > lines.len() {
                bail!("insert_line {} is out of bounds for {}", insert_line, path);
            }
            lines.insert(insert_line, insert_text.to_string());
            tokio::fs::write(&resolved.absolute, lines.join("\n"))
                .await
                .with_context(|| format!("Failed to write {}", path))?;
            rebuild_generated_memory_files(config, workspace_root).await?;
            Ok(format!("Inserted at line {insert_line} in {path}"))
        }
        NativeMemoryCommand::Delete => {
            let path = required_text(request.path.as_deref(), "path")?;
            let resolved = resolve_virtual_path(root, path)?;
            ensure_writable_path(&resolved.relative, path)?;
            let metadata = tokio::fs::metadata(&resolved.absolute)
                .await
                .with_context(|| format!("Failed to stat {}", path))?;
            if metadata.is_dir() {
                tokio::fs::remove_dir_all(&resolved.absolute)
                    .await
                    .with_context(|| format!("Failed to delete {}", path))?;
            } else {
                tokio::fs::remove_file(&resolved.absolute)
                    .await
                    .with_context(|| format!("Failed to delete {}", path))?;
            }
            rebuild_generated_memory_files(config, workspace_root).await?;
            Ok(format!("Deleted {path}"))
        }
        NativeMemoryCommand::Rename => {
            let old_path = required_text(request.old_path.as_deref(), "old_path")?;
            let new_path = required_text(request.new_path.as_deref(), "new_path")?;
            let old_resolved = resolve_virtual_path(root, old_path)?;
            let new_resolved = resolve_virtual_path(root, new_path)?;
            ensure_writable_path(&old_resolved.relative, old_path)?;
            ensure_writable_path(&new_resolved.relative, new_path)?;
            if let Some(parent) = new_resolved.absolute.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            tokio::fs::rename(&old_resolved.absolute, &new_resolved.absolute)
                .await
                .with_context(|| format!("Failed to rename {old_path} to {new_path}"))?;
            rebuild_generated_memory_files(config, workspace_root).await?;
            Ok(format!("Renamed {old_path} -> {new_path}"))
        }
    }
}

async fn view(root: &Path, path: &str) -> Result<String> {
    let resolved = resolve_virtual_path(root, path)?;
    if !resolved.absolute.exists() {
        if path == MEMORIES_ROOT {
            return Ok("Directory /memories is empty.".to_string());
        }
        bail!("{path} does not exist");
    }

    let metadata = tokio::fs::metadata(&resolved.absolute)
        .await
        .with_context(|| format!("Failed to stat {}", path))?;
    if metadata.is_dir() {
        let mut entries = tokio::fs::read_dir(&resolved.absolute)
            .await
            .with_context(|| format!("Failed to list {}", path))?;
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let suffix = if entry_path.is_dir() { "/" } else { "" };
            names.push(format!("{file_name}{suffix}"));
        }
        names.sort();
        if names.is_empty() {
            return Ok("(empty directory)".to_string());
        }
        return Ok(names.join("\n"));
    }

    let content = tokio::fs::read_to_string(&resolved.absolute)
        .await
        .with_context(|| format!("Failed to read {}", path))?;
    Ok(content
        .split('\n')
        .enumerate()
        .map(|(idx, line)| format!("{:4}\t{}", idx + 1, line))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn required_text<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{field} is required"))
}

struct ResolvedMemoryPath {
    absolute: PathBuf,
    relative: PathBuf,
}

fn resolve_virtual_path(root: &Path, virtual_path: &str) -> Result<ResolvedMemoryPath> {
    let trimmed = virtual_path.trim();
    if !trimmed.starts_with(MEMORIES_ROOT) {
        bail!("memory paths must stay under {MEMORIES_ROOT}");
    }

    let relative_raw = trimmed
        .strip_prefix(MEMORIES_ROOT)
        .unwrap_or_default()
        .trim_start_matches('/');
    let relative = if relative_raw.is_empty() {
        PathBuf::new()
    } else {
        sanitize_relative_path(relative_raw)?
    };

    Ok(ResolvedMemoryPath {
        absolute: if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&relative)
        },
        relative,
    })
}

fn sanitize_relative_path(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => sanitized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("memory paths may not escape {MEMORIES_ROOT}");
            }
        }
    }
    Ok(sanitized)
}

fn ensure_writable_path(relative: &Path, original: &str) -> Result<()> {
    if is_writable_relative_path(relative) {
        return Ok(());
    }

    bail!(
        "{original} is read-only; writable paths are /memories/preferences.md, /memories/repository-facts.md, and /memories/notes/**"
    );
}

fn is_writable_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();

    matches!(
        components.as_slice(),
        [single] if single == "preferences.md" || single == "repository-facts.md"
    ) || matches!(components.as_slice(), [first, ..] if first == "notes" && components.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::{MEMORIES_ROOT, execute, parameter_schema};
    use crate::config::PersistentMemoryConfig;
    use crate::persistent_memory::{
        MEMORY_FILENAME, MEMORY_SUMMARY_FILENAME, ROLLOUT_SUMMARIES_DIRNAME,
        resolve_persistent_memory_dir,
    };
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn parameter_schema_lists_supported_commands() {
        let schema = parameter_schema();
        assert_eq!(
            schema["properties"]["command"]["enum"],
            json!([
                "view",
                "create",
                "str_replace",
                "insert",
                "delete",
                "rename"
            ])
        );
    }

    #[tokio::test]
    async fn execute_supports_crud_and_rebuilds_generated_files() {
        let workspace = tempdir().expect("workspace");
        let config = PersistentMemoryConfig {
            enabled: true,
            directory_override: Some(workspace.path().join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        };

        execute(
            workspace.path(),
            &config,
            json!({
                "command": "create",
                "path": "/memories/notes/research.md",
                "file_text": "# Notes\n\n- First finding"
            }),
        )
        .await
        .expect("create");

        let view = execute(
            workspace.path(),
            &config,
            json!({
                "command": "view",
                "path": "/memories/notes/research.md"
            }),
        )
        .await
        .expect("view");
        assert!(view.as_str().expect("string").contains("First finding"));

        execute(
            workspace.path(),
            &config,
            json!({
                "command": "str_replace",
                "path": "/memories/notes/research.md",
                "old_str": "First finding",
                "new_str": "Updated finding"
            }),
        )
        .await
        .expect("replace");
        execute(
            workspace.path(),
            &config,
            json!({
                "command": "insert",
                "path": "/memories/notes/research.md",
                "insert_line": 2,
                "insert_text": "- Follow-up"
            }),
        )
        .await
        .expect("insert");
        execute(
            workspace.path(),
            &config,
            json!({
                "command": "rename",
                "old_path": "/memories/notes/research.md",
                "new_path": "/memories/notes/archive/research.md"
            }),
        )
        .await
        .expect("rename");

        let memory_dir = resolve_persistent_memory_dir(&config, workspace.path())
            .expect("dir")
            .expect("resolved");
        let summary =
            std::fs::read_to_string(memory_dir.join(MEMORY_SUMMARY_FILENAME)).expect("summary");
        assert!(summary.contains("Updated finding") || summary.contains("Follow-up"));

        execute(
            workspace.path(),
            &config,
            json!({
                "command": "delete",
                "path": "/memories/notes/archive/research.md"
            }),
        )
        .await
        .expect("delete");
        let recreated_summary =
            std::fs::read_to_string(memory_dir.join(MEMORY_SUMMARY_FILENAME)).expect("summary");
        assert!(!recreated_summary.contains("Updated finding"));
        assert!(!recreated_summary.contains("Follow-up"));
    }

    #[tokio::test]
    async fn execute_blocks_path_traversal_and_generated_file_writes() {
        let workspace = tempdir().expect("workspace");
        let config = PersistentMemoryConfig {
            enabled: true,
            directory_override: Some(workspace.path().join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        };

        let traversal = execute(
            workspace.path(),
            &config,
            json!({
                "command": "create",
                "path": "/memories/notes/../../escape.md",
                "file_text": "bad"
            }),
        )
        .await;
        assert!(traversal.is_err());

        let readonly = execute(
            workspace.path(),
            &config,
            json!({
                "command": "create",
                "path": format!("{MEMORIES_ROOT}/{MEMORY_FILENAME}"),
                "file_text": "bad"
            }),
        )
        .await;
        assert!(readonly.is_err());
    }

    #[tokio::test]
    async fn execute_views_root_and_rollout_directories_read_only() {
        let workspace = tempdir().expect("workspace");
        let config = PersistentMemoryConfig {
            enabled: true,
            directory_override: Some(workspace.path().join(".memory").display().to_string()),
            ..PersistentMemoryConfig::default()
        };

        let root_listing = execute(
            workspace.path(),
            &config,
            json!({
                "command": "view",
                "path": MEMORIES_ROOT
            }),
        )
        .await
        .expect("view root");
        let root_listing = root_listing.as_str().expect("string");
        assert!(root_listing.contains("notes/"));
        assert!(root_listing.contains(&format!("{ROLLOUT_SUMMARIES_DIRNAME}/")));

        let rollout_write = execute(
            workspace.path(),
            &config,
            json!({
                "command": "create",
                "path": format!("{MEMORIES_ROOT}/{ROLLOUT_SUMMARIES_DIRNAME}/bad.md"),
                "file_text": "bad"
            }),
        )
        .await;
        assert!(rollout_write.is_err());
    }
}
