use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt};

use anyhow::{Context, Result, anyhow};
use glob::glob;
use serde::Serialize;
use tracing::warn;

use crate::utils::file_utils::canonicalize_with_context;

const AGENTS_FILENAME: &str = "AGENTS.md";
const AGENTS_OVERRIDE_FILENAME: &str = "AGENTS.override.md";
const GLOBAL_CONFIG_DIRECTORY: &str = ".config/vtcode";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum InstructionScope {
    Global,
    Workspace,
    Custom,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSource {
    pub path: PathBuf,
    pub scope: InstructionScope,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSegment {
    pub source: InstructionSource,
    pub contents: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionBundle {
    pub segments: Vec<InstructionSegment>,
    pub truncated: bool,
    pub bytes_read: usize,
}

impl InstructionBundle {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn combined_text(&self) -> String {
        let capacity = self
            .segments
            .iter()
            .map(|segment| segment.contents.len())
            .sum::<usize>()
            .saturating_add(self.segments.len().saturating_sub(1) * 2);
        let mut output = String::with_capacity(capacity);
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                output.push_str("\n\n");
            }

            output.push_str(&segment.contents);
        }
        output
    }
}

pub async fn discover_instruction_sources(
    current_dir: &Path,
    project_root: &Path,
    home_dir: Option<&Path>,
    extra_patterns: &[String],
) -> Result<Vec<InstructionSource>> {
    let mut sources = Vec::with_capacity(8); // Typical: 2-4 global + 2-4 workspace
    let mut seen_paths = HashSet::new();

    if let Some(home) = home_dir {
        for candidate in global_instruction_candidates(home) {
            if instruction_exists(&candidate).await? && seen_paths.insert(candidate.clone()) {
                sources.push(InstructionSource {
                    path: candidate,
                    scope: InstructionScope::Global,
                });
            }
        }
    }

    let extra_paths = expand_instruction_patterns(project_root, home_dir, extra_patterns).await?;
    for path in extra_paths {
        if seen_paths.insert(path.clone()) {
            sources.push(InstructionSource {
                path,
                scope: InstructionScope::Custom,
            });
        }
    }

    let root = canonicalize_with_context(project_root, "project root")?;

    let mut cursor = canonicalize_with_context(current_dir, "working directory")?;

    if !cursor.starts_with(&root) {
        cursor = root.clone();
    }

    let mut workspace_paths = Vec::with_capacity(4); // Typical directory depth
    loop {
        let override_candidate = cursor.join(AGENTS_OVERRIDE_FILENAME);
        let agents_candidate = cursor.join(AGENTS_FILENAME);

        let chosen = if instruction_exists(&override_candidate).await? {
            Some(override_candidate)
        } else if instruction_exists(&agents_candidate).await? {
            Some(agents_candidate)
        } else {
            None
        };

        if let Some(path) = chosen
            && seen_paths.insert(path.clone())
        {
            workspace_paths.push(InstructionSource {
                path,
                scope: InstructionScope::Workspace,
            });
        }

        if cursor == root {
            break;
        }

        cursor = cursor
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Reached filesystem root before encountering project root"))?;
    }

    workspace_paths.reverse();
    sources.extend(workspace_paths);

    Ok(sources)
}

pub async fn read_instruction_bundle(
    current_dir: &Path,
    project_root: &Path,
    home_dir: Option<&Path>,
    extra_patterns: &[String],
    max_bytes: usize,
) -> Result<Option<InstructionBundle>> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let sources =
        discover_instruction_sources(current_dir, project_root, home_dir, extra_patterns).await?;
    if sources.is_empty() {
        return Ok(None);
    }

    let mut remaining = max_bytes;
    let mut segments = Vec::with_capacity(sources.len()); // One segment per source
    let mut truncated = false;
    let mut bytes_read = 0usize;
    let mut truncation_warning_emitted = false;

    for source in sources {
        if remaining == 0 {
            truncated = true;
            break;
        }

        let file = match File::open(&source.path).await {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to open instruction file at {}",
                        source.path.display()
                    )
                });
            }
        };

        let metadata = file
            .metadata()
            .await
            .with_context(|| format!("Failed to read metadata for {}", source.path.display()))?;

        let mut reader = io::BufReader::new(file).take(remaining as u64);
        let mut data = Vec::with_capacity(remaining.min(metadata.len() as usize));
        reader.read_to_end(&mut data).await.with_context(|| {
            format!(
                "Failed to read instruction file from {}",
                source.path.display()
            )
        })?;

        if metadata.len() as usize > remaining {
            truncated = true;
            if !truncation_warning_emitted {
                warn!(
                    "Instruction file `{}` exceeds remaining budget ({} bytes) - truncating.",
                    source.path.display(),
                    remaining
                );
                truncation_warning_emitted = true;
            } else {
                tracing::debug!(
                    "Additional instruction file `{}` exceeds remaining budget ({} bytes); truncating.",
                    source.path.display(),
                    remaining
                );
            }
        }

        if data.iter().all(|byte| byte.is_ascii_whitespace()) {
            remaining = remaining.saturating_sub(data.len());
            continue;
        }

        let text = String::from_utf8_lossy(&data).to_string();
        if text.trim().is_empty() {
            remaining = remaining.saturating_sub(data.len());
            continue;
        }

        bytes_read += data.len();
        remaining = remaining.saturating_sub(data.len());
        segments.push(InstructionSegment {
            source,
            contents: text,
        });
    }

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(InstructionBundle {
            segments,
            truncated,
            bytes_read,
        }))
    }
}

fn global_instruction_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(AGENTS_FILENAME),
        home.join(".vtcode").join(AGENTS_FILENAME),
        home.join(GLOBAL_CONFIG_DIRECTORY).join(AGENTS_FILENAME),
    ]
}

async fn expand_instruction_patterns(
    project_root: &Path,
    home_dir: Option<&Path>,
    patterns: &[String],
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for pattern in patterns {
        let resolved = resolve_pattern(pattern, project_root, home_dir)?;
        let glob_matches: Vec<PathBuf> = glob(&resolved)
            .with_context(|| format!("Failed to expand instruction pattern `{pattern}`"))?
            .filter_map(|entry| match entry {
                Ok(path) => Some(path),
                Err(err) => {
                    warn!("Ignoring malformed instruction path for pattern `{pattern}`: {err}");
                    None
                }
            })
            .collect();

        // Filter asynchronously
        let mut matches = Vec::new();
        for path in glob_matches {
            match instruction_exists(&path).await {
                Ok(true) => matches.push(path),
                Ok(false) => {}
                Err(err) => {
                    warn!(
                        "Failed to inspect potential instruction `{}`: {err:#}",
                        path.display()
                    );
                }
            }
        }

        if matches.is_empty() {
            warn!("Instruction pattern `{pattern}` did not match any files");
        } else {
            matches.sort();
            paths.extend(matches);
        }
    }

    Ok(paths)
}

fn resolve_pattern(pattern: &str, project_root: &Path, home_dir: Option<&Path>) -> Result<String> {
    if let Some(stripped) = pattern.strip_prefix("~/") {
        let home = home_dir.ok_or_else(|| {
            anyhow!("Cannot expand `~` in instruction pattern `{pattern}` without a home directory")
        })?;
        return Ok(home.join(stripped).to_string_lossy().into_owned());
    }

    let candidate = Path::new(pattern);
    let full_path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        project_root.join(candidate)
    };

    Ok(full_path.to_string_lossy().into_owned())
}

async fn instruction_exists(path: &Path) -> Result<bool> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(metadata.file_type().is_file() || metadata.file_type().is_symlink()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err)
            .with_context(|| format!("Failed to inspect instruction candidate {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn collects_sources_with_precedence_and_patterns() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let nested = project_root.join("src");
        std::fs::create_dir_all(&nested)?;

        let global_home = tempdir()?;
        let global_rule = global_home.path().join(".vtcode").join(AGENTS_FILENAME);
        std::fs::create_dir_all(global_rule.parent().unwrap())?;
        std::fs::write(&global_rule, "# Global Rules\n- Global applies")?;

        let root_rule = project_root.join(AGENTS_FILENAME);
        std::fs::write(&root_rule, "# Workspace Rules\n- Root applies")?;

        let nested_rule = nested.join(AGENTS_FILENAME);
        std::fs::write(&nested_rule, "# Nested Rules\n- Nested applies")?;

        let extra_dir = project_root.join("docs");
        std::fs::create_dir_all(&extra_dir)?;
        let extra_file = extra_dir.join("guidelines.md");
        std::fs::write(&extra_file, "# Extra Instructions\n- Extra applies")?;

        let global_rule_canon = std::fs::canonicalize(&global_rule)?;
        let root_rule_canon = std::fs::canonicalize(&root_rule)?;
        let nested_rule_canon = std::fs::canonicalize(&nested_rule)?;
        let extra_file_canon = std::fs::canonicalize(&extra_file)?;

        let canonical = |path: &PathBuf| {
            std::fs::canonicalize(path).expect("failed to canonicalize instruction path")
        };

        let patterns = vec!["docs/*.md".to_owned()];
        let sources = discover_instruction_sources(
            &nested,
            project_root,
            Some(global_home.path()),
            &patterns,
        )
        .await?;
        assert_eq!(sources.len(), 4);
        assert!(matches!(sources[0].scope, InstructionScope::Global));
        assert_eq!(canonical(&sources[0].path), global_rule_canon);
        assert!(matches!(sources[1].scope, InstructionScope::Custom));
        assert_eq!(canonical(&sources[1].path), extra_file_canon);
        assert!(matches!(sources[2].scope, InstructionScope::Workspace));
        assert_eq!(canonical(&sources[2].path), root_rule_canon);
        assert_eq!(canonical(&sources[3].path), nested_rule_canon);

        let bundle = read_instruction_bundle(
            &nested,
            project_root,
            Some(global_home.path()),
            &patterns,
            16 * 1024,
        )
        .await?
        .expect("expected instruction bundle");
        assert_eq!(bundle.segments.len(), 4);
        assert!(bundle.bytes_read > 0);
        assert!(!bundle.truncated);

        Ok(())
    }

    #[tokio::test]
    async fn handles_missing_instructions_gracefully() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let nested = project_root.join("src");
        std::fs::create_dir_all(&nested)?;

        let sources = discover_instruction_sources(&nested, project_root, None, &[]).await?;
        assert!(sources.is_empty());

        let bundle = read_instruction_bundle(&nested, project_root, None, &[], 4 * 1024).await?;
        assert!(bundle.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn enforces_byte_budget() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let root_rule = project_root.join(AGENTS_FILENAME);
        std::fs::write(&root_rule, "A".repeat(4096))?;

        let bundle = read_instruction_bundle(project_root, project_root, None, &[], 1024)
            .await?
            .expect("expected truncated bundle");
        assert!(bundle.truncated);
        assert!(bundle.bytes_read <= 1024);

        Ok(())
    }

    #[tokio::test]
    async fn expands_home_patterns() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();
        let home = tempdir()?;
        let personal = home.path().join("notes.md");
        std::fs::write(&personal, "# Personal instructions")?;

        let sources = discover_instruction_sources(
            project_root,
            project_root,
            Some(home.path()),
            &["~/notes.md".to_owned()],
        )
        .await?;
        assert_eq!(sources.len(), 1);
        assert!(matches!(sources[0].scope, InstructionScope::Custom));
        assert_eq!(sources[0].path, personal);

        Ok(())
    }

    #[tokio::test]
    async fn prefers_override_over_base() -> Result<()> {
        let workspace = tempdir()?;
        let project_root = workspace.path();

        let base_rule = project_root.join(AGENTS_FILENAME);
        std::fs::write(&base_rule, "base content")?;

        let override_rule = project_root.join(AGENTS_OVERRIDE_FILENAME);
        std::fs::write(&override_rule, "override content")?;

        let sources = discover_instruction_sources(project_root, project_root, None, &[]).await?;
        assert_eq!(sources.len(), 1);
        let override_rule_canon = std::fs::canonicalize(&override_rule)?;
        assert_eq!(sources[0].path, override_rule_canon);

        let bundle = read_instruction_bundle(project_root, project_root, None, &[], 1024)
            .await?
            .expect("expected bundle");
        assert_eq!(bundle.combined_text(), "override content");

        Ok(())
    }
}
