use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::warn;
use vtcode_core::tools::dominant_workspace_language;
use vtcode_core::utils::common::{
    display_language_from_editor_language_id, display_language_from_path,
};

const IDE_CONTEXT_ENV_VAR: &str = "VT_VSCODE_CONTEXT_FILE";

pub(crate) struct IdeContextBridge {
    path: PathBuf,
    last_digest: Option<u64>,
}

impl IdeContextBridge {
    pub(crate) fn from_env() -> Option<Self> {
        Some(Self {
            path: ide_context_path_from_env()?,
            last_digest: None,
        })
    }

    pub(crate) fn snapshot(&mut self) -> Result<Option<String>> {
        let content = match read_ide_context_file(&self.path)? {
            Some(content) => content,
            None => {
                self.last_digest = None;
                return Ok(None);
            }
        };

        let digest = compute_digest(&content);
        if self.last_digest == Some(digest) {
            return Ok(None);
        }

        self.last_digest = Some(digest);
        Ok(Some(content))
    }
}

pub(crate) fn preferred_display_language_for_workspace(workspace: &Path) -> Option<String> {
    let ide_context = match read_current_ide_context() {
        Ok(ide_context) => ide_context,
        Err(error) => {
            warn!(
                workspace = %workspace.display(),
                error = ?error,
                "Failed to read IDE context while resolving active editor language"
            );
            None
        }
    };

    preferred_display_language_for_workspace_with_context(workspace, ide_context.as_deref())
}

fn preferred_display_language_for_workspace_with_context(
    workspace: &Path,
    ide_context: Option<&str>,
) -> Option<String> {
    ide_context
        .and_then(active_editor_language_from_markdown)
        .or_else(|| dominant_workspace_language(workspace))
}

fn read_current_ide_context() -> Result<Option<String>> {
    let Some(path) = ide_context_path_from_env() else {
        return Ok(None);
    };
    read_ide_context_file(&path)
}

fn ide_context_path_from_env() -> Option<PathBuf> {
    let raw = env::var(IDE_CONTEXT_ENV_VAR).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn read_ide_context_file(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read IDE context file at {}", path.display()));
        }
    };

    let normalized = content.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}

fn active_editor_language_from_markdown(markdown: &str) -> Option<String> {
    let section = active_editor_section(markdown)?;
    active_editor_language_from_label(section)
        .or_else(|| active_editor_code_fence_language(section))
        .map(ToOwned::to_owned)
}

fn active_editor_section(markdown: &str) -> Option<&str> {
    let start = markdown.find("### Active Editor:")?;
    let rest = &markdown[start..];
    let end = rest.find("\n### ").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn active_editor_language_from_label(section: &str) -> Option<&'static str> {
    let first_line = section.lines().next()?.trim();
    let label = first_line.strip_prefix("### Active Editor:")?.trim();
    let path_label = label
        .split_once(" (lines ")
        .map(|(path, _)| path)
        .unwrap_or(label)
        .trim();
    if path_label.is_empty() {
        return None;
    }
    display_language_from_path(Path::new(path_label))
}

fn active_editor_code_fence_language(section: &str) -> Option<&'static str> {
    section.lines().find_map(|line| {
        let language_id = line.trim().strip_prefix("```")?.trim();
        if language_id.is_empty() {
            return None;
        }
        display_language_from_editor_language_id(language_id)
    })
}

fn compute_digest(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{
        active_editor_language_from_markdown, preferred_display_language_for_workspace_with_context,
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn active_editor_language_prefers_file_extension() {
        let markdown = r#"
### Active Editor: src/app.tsx (lines 1-20)

```typescriptreact
export function App() {}
```
"#;

        assert_eq!(
            active_editor_language_from_markdown(markdown),
            Some("TypeScript".to_string())
        );
    }

    #[test]
    fn active_editor_language_uses_code_fence_for_untitled_buffers() {
        let markdown = r#"
### Active Editor: untitled:Scratch-1

```rust
fn main() {}
```
"#;

        assert_eq!(
            active_editor_language_from_markdown(markdown),
            Some("Rust".to_string())
        );
    }

    #[test]
    fn preferred_display_language_falls_back_to_workspace_language() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");

        assert_eq!(
            preferred_display_language_for_workspace_with_context(workspace.path(), None),
            Some("Rust".to_string())
        );
    }
}
