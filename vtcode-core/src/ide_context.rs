use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use vtcode_config::IdeContextProviderFamily;

use crate::utils::common::{display_language_from_editor_language_id, display_language_from_path};

pub const IDE_CONTEXT_ENV_VAR: &str = "VT_IDE_CONTEXT_FILE";
pub const LEGACY_VSCODE_CONTEXT_ENV_VAR: &str = "VT_VSCODE_CONTEXT_FILE";
pub const IDE_CONTEXT_SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub struct EditorContextSnapshot {
    #[serde(default = "default_snapshot_version")]
    pub version: u32,
    #[serde(default)]
    pub provider_family: IdeContextProviderFamily,
    #[serde(default)]
    pub editor_name: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<PathBuf>,
    #[serde(default)]
    pub active_file: Option<EditorFileContext>,
    #[serde(default)]
    pub visible_editors: Vec<EditorFileContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EditorFileContext {
    pub path: String,
    #[serde(default)]
    pub language_id: Option<String>,
    #[serde(default)]
    pub line_range: Option<EditorLineRange>,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub selection: Option<EditorSelectionContext>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EditorLineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EditorSelectionRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EditorSelectionContext {
    pub range: EditorSelectionRange,
    #[serde(default)]
    pub text: Option<String>,
}

impl EditorContextSnapshot {
    pub fn read_from_env() -> Result<Option<Self>> {
        if let Some(path) = snapshot_path_from_env(IDE_CONTEXT_ENV_VAR) {
            return Self::read_json_file(&path);
        }

        if let Some(path) = snapshot_path_from_env(LEGACY_VSCODE_CONTEXT_ENV_VAR) {
            return Self::read_legacy_markdown_file(&path);
        }

        Ok(None)
    }

    pub fn read_json_file(path: &Path) -> Result<Option<Self>> {
        let Some(content) = read_snapshot_file(path)? else {
            return Ok(None);
        };
        let snapshot: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "failed to parse IDE context JSON snapshot at {}",
                path.display()
            )
        })?;
        Ok(Some(snapshot.normalized()))
    }

    pub fn read_legacy_markdown_file(path: &Path) -> Result<Option<Self>> {
        let Some(content) = read_snapshot_file(path)? else {
            return Ok(None);
        };
        Ok(parse_legacy_markdown_snapshot(&content))
    }

    pub fn normalized(mut self) -> Self {
        self.version = if self.version == 0 {
            IDE_CONTEXT_SNAPSHOT_VERSION
        } else {
            self.version
        };

        if let Some(editor_name) = self.editor_name.as_mut() {
            let trimmed = editor_name.trim();
            if trimmed.is_empty() {
                self.editor_name = None;
            } else if trimmed != editor_name {
                *editor_name = trimmed.to_string();
            }
        }

        if let Some(active_file) = self.active_file.as_mut() {
            active_file.normalize();
        }
        for editor in &mut self.visible_editors {
            editor.normalize();
        }
        self
    }

    pub fn active_display_language(&self) -> Option<String> {
        self.active_file
            .as_ref()
            .and_then(EditorFileContext::display_language)
    }

    pub fn has_explicit_selection(&self) -> bool {
        self.active_file
            .as_ref()
            .and_then(|file| file.selection.as_ref())
            .is_some_and(EditorSelectionContext::has_explicit_selection)
    }

    pub fn header_summary(&self, workspace_root: &Path) -> Option<String> {
        let file = self.active_file.as_ref()?;
        let mut parts = Vec::new();

        let path_label = file.display_path(workspace_root, self.workspace_root.as_deref());
        if !path_label.is_empty() {
            parts.push(format!("File: {}", path_label));
        }

        if let Some(language) = file.display_language() {
            if parts.is_empty() {
                parts.push(format!("Lang: {}", language));
            } else {
                parts.push(language);
            }
        }

        if let Some(selection) = file
            .selection
            .as_ref()
            .filter(|selection| selection.has_explicit_selection())
        {
            parts.push(format!(
                "Sel {}-{}",
                selection.range.start_line, selection.range.end_line
            ));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" · "))
        }
    }

    pub fn prompt_block(
        &self,
        workspace_root: &Path,
        include_selection_text: bool,
    ) -> Option<String> {
        let file = self.active_file.as_ref()?;
        let active_path = file.display_path(workspace_root, self.workspace_root.as_deref());
        let mut lines = Vec::new();
        lines.push("## Active Editor Context".to_string());
        lines.push(format!(
            "- IDE family: {}",
            provider_family_label(self.provider_family)
        ));
        lines.push(format!("- Active file: {}", active_path));

        if let Some(language) = file.display_language() {
            lines.push(format!("- Language: {}", language));
        }

        if let Some(line_range) = file.line_range {
            lines.push(format!("- Editor lines: {}", format_line_range(line_range)));
        }

        if file.dirty || file.truncated {
            let mut states = Vec::new();
            if file.dirty {
                states.push("unsaved changes");
            }
            if file.truncated {
                states.push("truncated");
            }
            lines.push(format!("- Buffer state: {}", states.join(", ")));
        }

        if let Some(selection) = file
            .selection
            .as_ref()
            .filter(|selection| selection.has_explicit_selection())
        {
            lines.push(format!(
                "- Selection: {}:{}-{}:{}",
                selection.range.start_line,
                selection.range.start_column,
                selection.range.end_line,
                selection.range.end_column
            ));

            if include_selection_text
                && let Some(text) = selection.text.as_deref().map(str::trim)
                && !text.is_empty()
            {
                let fence_language = file.language_id.as_deref().unwrap_or("text");
                lines.push("- Selected text:".to_string());
                lines.push(format!("```{}", fence_language));
                lines.push(text.to_string());
                lines.push("```".to_string());
            }
        }

        let mut seen_paths = HashSet::new();
        let open_files = self
            .visible_editors
            .iter()
            .map(|editor| editor.display_path(workspace_root, self.workspace_root.as_deref()))
            .filter(|path| !path.trim().is_empty())
            .filter(|path| path != &active_path)
            .filter(|path| seen_paths.insert(path.clone()))
            .collect::<Vec<_>>();
        if !open_files.is_empty() {
            lines.push("- Open files:".to_string());
            lines.extend(open_files.into_iter().map(|path| format!("  - {}", path)));
        }

        Some(lines.join("\n"))
    }
}

impl EditorFileContext {
    fn normalize(&mut self) {
        self.path = self.path.trim().to_string();
        if let Some(language_id) = self.language_id.as_mut() {
            let trimmed = language_id.trim();
            if trimmed.is_empty() {
                self.language_id = None;
            } else if trimmed != language_id {
                *language_id = trimmed.to_string();
            }
        }

        if let Some(selection) = self.selection.as_mut()
            && let Some(text) = selection.text.as_mut()
        {
            let normalized = normalize_snapshot_content(text);
            if normalized.trim().is_empty() {
                selection.text = None;
            } else {
                *text = normalized;
            }
        }
    }

    pub fn display_language(&self) -> Option<String> {
        self.language_id
            .as_deref()
            .and_then(display_language_from_editor_language_id)
            .or_else(|| display_language_from_path(Path::new(self.path.as_str())))
            .map(ToOwned::to_owned)
    }

    pub fn display_path(
        &self,
        workspace_root: &Path,
        snapshot_workspace_root: Option<&Path>,
    ) -> String {
        let raw = self.path.trim();
        if raw.is_empty() {
            return String::new();
        }

        if raw.contains("://") || raw.starts_with("untitled:") {
            return raw.to_string();
        }

        let candidate = Path::new(raw);
        if candidate.is_relative() {
            return raw.to_string();
        }

        for root in [Some(workspace_root), snapshot_workspace_root] {
            if let Some(root) = root
                && let Ok(relative) = candidate.strip_prefix(root)
            {
                return relative.display().to_string();
            }
        }

        raw.to_string()
    }

    pub fn has_explicit_selection(&self) -> bool {
        self.selection
            .as_ref()
            .is_some_and(EditorSelectionContext::has_explicit_selection)
    }
}

impl EditorSelectionContext {
    pub fn has_explicit_selection(&self) -> bool {
        let range = self.range;
        range.start_line != range.end_line || range.start_column != range.end_column
    }
}

const fn default_snapshot_version() -> u32 {
    IDE_CONTEXT_SNAPSHOT_VERSION
}

fn snapshot_path_from_env(env_var: &str) -> Option<PathBuf> {
    env::var(env_var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_snapshot_file(path: &Path) -> Result<Option<String>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read IDE context snapshot {}", path.display())
            });
        }
    };

    let normalized = normalize_snapshot_content(&content);
    if normalized.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(normalized))
}

fn normalize_snapshot_content(content: &str) -> String {
    content.replace("\r\n", "\n")
}

fn parse_legacy_markdown_snapshot(markdown: &str) -> Option<EditorContextSnapshot> {
    let mut active_file = None;
    let mut visible_editors = Vec::new();

    for section in markdown
        .split("\n### ")
        .filter(|section| !section.trim().is_empty())
    {
        let normalized = if section.starts_with("### ") {
            section.to_string()
        } else {
            format!("### {}", section)
        };

        if let Some(file) = parse_legacy_editor_section(&normalized, "### Active Editor:") {
            active_file = Some(file);
            continue;
        }

        if let Some(file) = parse_legacy_editor_section(&normalized, "### Editor:") {
            visible_editors.push(file);
        }
    }

    if active_file.is_none() && visible_editors.is_empty() {
        return None;
    }

    Some(EditorContextSnapshot {
        version: IDE_CONTEXT_SNAPSHOT_VERSION,
        provider_family: IdeContextProviderFamily::VscodeCompatible,
        editor_name: Some("VS Code".to_string()),
        workspace_root: None,
        active_file,
        visible_editors,
    })
}

fn parse_legacy_editor_section(section: &str, prefix: &str) -> Option<EditorFileContext> {
    let first_line = section.lines().next()?.trim();
    let heading = first_line.strip_prefix(prefix)?.trim();
    let (path, details) = split_heading_label_and_details(heading);
    let language_id = parse_legacy_fence_language(section);

    Some(EditorFileContext {
        path: path.to_string(),
        language_id,
        line_range: details.and_then(parse_line_range_from_details),
        dirty: details.is_some_and(|detail| detail.contains("unsaved changes")),
        truncated: details.is_some_and(|detail| detail.contains("truncated")),
        selection: None,
    })
}

fn split_heading_label_and_details(heading: &str) -> (&str, Option<&str>) {
    let trimmed = heading.trim();
    if let Some((path, details)) = trimmed.rsplit_once(" (")
        && let Some(details) = details.strip_suffix(')')
    {
        return (path.trim(), Some(details));
    }

    (trimmed, None)
}

fn parse_legacy_fence_language(section: &str) -> Option<String> {
    section.lines().find_map(|line| {
        line.trim()
            .strip_prefix("```")
            .map(str::trim)
            .filter(|language| !language.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn parse_line_range_from_details(details: &str) -> Option<EditorLineRange> {
    let marker = "lines ";
    let line_token = details
        .split('•')
        .map(str::trim)
        .find_map(|entry| entry.strip_prefix(marker))?;

    parse_line_range(line_token)
}

fn parse_line_range(text: &str) -> Option<EditorLineRange> {
    let trimmed = text.trim();
    let (start, end) = trimmed
        .split_once('-')
        .map(|(start, end)| (start.trim(), end.trim()))
        .unwrap_or((trimmed, trimmed));

    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    Some(EditorLineRange { start, end })
}

fn provider_family_label(family: IdeContextProviderFamily) -> &'static str {
    match family {
        IdeContextProviderFamily::VscodeCompatible => "vscode_compatible",
        IdeContextProviderFamily::Zed => "zed",
        IdeContextProviderFamily::Generic => "generic",
    }
}

fn format_line_range(range: EditorLineRange) -> String {
    if range.start == range.end {
        range.start.to_string()
    } else {
        format!("{}-{}", range.start, range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EditorContextSnapshot, EditorFileContext, EditorLineRange, EditorSelectionContext,
        EditorSelectionRange, IDE_CONTEXT_SNAPSHOT_VERSION,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use vtcode_config::IdeContextProviderFamily;

    #[test]
    fn parses_json_snapshot_file() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("snapshot.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "provider_family": "zed",
                "editor_name": " VS Code ",
                "workspace_root": "/workspace",
                "active_file": {
                    "path": "/workspace/src/main.rs",
                    "language_id": "rust",
                    "line_range": { "start": 10, "end": 24 },
                    "dirty": true,
                    "truncated": false,
                    "selection": {
                        "range": {
                            "start_line": 12,
                            "start_column": 1,
                            "end_line": 18,
                            "end_column": 4
                        },
                        "text": "fn main() {}\n"
                    }
                }
            }"#,
        )
        .expect("write snapshot");

        let snapshot = EditorContextSnapshot::read_json_file(&path)
            .expect("read snapshot")
            .expect("snapshot");

        assert_eq!(snapshot.provider_family, IdeContextProviderFamily::Zed);
        assert_eq!(snapshot.editor_name.as_deref(), Some("VS Code"));
        assert_eq!(snapshot.active_display_language().as_deref(), Some("Rust"));
        assert_eq!(
            snapshot.header_summary(Path::new("/workspace")).as_deref(),
            Some("File: src/main.rs · Rust · Sel 12-18")
        );
    }

    #[test]
    fn parses_legacy_markdown_snapshot() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("snapshot.md");
        fs::write(
            &path,
            r#"
## VS Code Context

### Active Editor: src/app.tsx (lines 12-18 • unsaved changes • truncated)

```typescriptreact
export function App() {}
```

### Editor: src/lib.ts (lines 1-4)

```typescript
export const value = 1;
```
"#,
        )
        .expect("write snapshot");

        let snapshot = EditorContextSnapshot::read_legacy_markdown_file(&path)
            .expect("read snapshot")
            .expect("snapshot");

        let active = snapshot.active_file.expect("active file");
        assert_eq!(snapshot.editor_name.as_deref(), Some("VS Code"));
        assert_eq!(active.path, "src/app.tsx");
        assert_eq!(active.language_id.as_deref(), Some("typescriptreact"));
        assert_eq!(
            active.line_range,
            Some(EditorLineRange { start: 12, end: 18 })
        );
        assert!(active.dirty);
        assert!(active.truncated);
        assert_eq!(snapshot.visible_editors.len(), 1);
    }

    #[test]
    fn prompt_block_includes_selection_text_when_requested() {
        let snapshot = EditorContextSnapshot {
            version: IDE_CONTEXT_SNAPSHOT_VERSION,
            provider_family: IdeContextProviderFamily::Generic,
            editor_name: None,
            workspace_root: Some(PathBuf::from("/workspace")),
            active_file: Some(EditorFileContext {
                path: "/workspace/src/main.rs".to_string(),
                language_id: Some("rust".to_string()),
                line_range: Some(EditorLineRange { start: 1, end: 20 }),
                dirty: false,
                truncated: false,
                selection: Some(EditorSelectionContext {
                    range: EditorSelectionRange {
                        start_line: 4,
                        start_column: 1,
                        end_line: 6,
                        end_column: 2,
                    },
                    text: Some("fn main() {}\n".to_string()),
                }),
            }),
            visible_editors: vec![
                EditorFileContext {
                    path: "/workspace/src/main.rs".to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: Some(EditorLineRange { start: 1, end: 20 }),
                    dirty: false,
                    truncated: false,
                    selection: None,
                },
                EditorFileContext {
                    path: "/workspace/src/lib.rs".to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: Some(EditorLineRange { start: 1, end: 80 }),
                    dirty: false,
                    truncated: false,
                    selection: None,
                },
                EditorFileContext {
                    path: "/workspace/src/lib.rs".to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: Some(EditorLineRange { start: 1, end: 80 }),
                    dirty: false,
                    truncated: false,
                    selection: None,
                },
            ],
        };

        let prompt = snapshot
            .prompt_block(Path::new("/workspace"), true)
            .expect("prompt");

        assert!(prompt.contains("## Active Editor Context"));
        assert!(prompt.contains("- Active file: src/main.rs"));
        assert!(prompt.contains("- Selection: 4:1-6:2"));
        assert!(prompt.contains("```rust"));
        assert!(prompt.contains("- Open files:"));
        assert!(!prompt.contains("  - src/main.rs"));
        assert!(prompt.contains("  - src/lib.rs"));
        assert_eq!(prompt.matches("  - src/lib.rs").count(), 1);
    }

    #[test]
    fn collapsed_selection_is_not_rendered_in_header_or_prompt() {
        let snapshot = EditorContextSnapshot {
            version: IDE_CONTEXT_SNAPSHOT_VERSION,
            provider_family: IdeContextProviderFamily::VscodeCompatible,
            editor_name: None,
            workspace_root: Some(PathBuf::from("/workspace")),
            active_file: Some(EditorFileContext {
                path: "/workspace/src/main.rs".to_string(),
                language_id: Some("rust".to_string()),
                line_range: Some(EditorLineRange { start: 12, end: 18 }),
                dirty: false,
                truncated: false,
                selection: Some(EditorSelectionContext {
                    range: EditorSelectionRange {
                        start_line: 12,
                        start_column: 4,
                        end_line: 12,
                        end_column: 4,
                    },
                    text: Some(String::new()),
                }),
            }),
            visible_editors: Vec::new(),
        };

        let header = snapshot
            .header_summary(Path::new("/workspace"))
            .expect("header summary");
        let prompt = snapshot
            .prompt_block(Path::new("/workspace"), true)
            .expect("prompt block");

        assert!(!header.contains("Sel "));
        assert!(!prompt.contains("- Selection:"));
        assert!(!prompt.contains("- Selected text:"));
    }
}
