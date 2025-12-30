use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::instructions::{InstructionBundle, read_instruction_bundle};
use crate::skills::model::SkillMetadata;
use crate::skills::render::render_skills_section;
use vtcode_config::core::AgentConfig;

pub const PROJECT_DOC_SEPARATOR: &str = "\n\n--- project-doc ---\n\n";

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDocBundle {
    pub contents: String,
    pub sources: Vec<PathBuf>,
    pub truncated: bool,
    pub bytes_read: usize,
}

impl ProjectDocBundle {
    pub fn highlights(&self, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        let mut highlights = Vec::with_capacity(limit);
        for line in self.contents.lines() {
            if highlights.len() >= limit {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('-') {
                let highlight = trimmed.trim_start_matches('-').trim();
                if !highlight.is_empty() {
                    highlights.push(highlight.to_string());
                }
            }
        }

        highlights
    }
}

pub struct ProjectDocOptions<'a> {
    pub current_dir: &'a Path,
    pub project_root: &'a Path,
    pub home_dir: Option<&'a Path>,
    pub extra_instruction_files: &'a [String],
    pub max_bytes: usize,
}

pub async fn read_project_doc_with_options(
    options: &ProjectDocOptions<'_>,
) -> Result<Option<ProjectDocBundle>> {
    if options.max_bytes == 0 {
        return Ok(None);
    }

    match read_instruction_bundle(
        options.current_dir,
        options.project_root,
        options.home_dir,
        options.extra_instruction_files,
        options.max_bytes,
    )
    .await?
    {
        Some(bundle) => Ok(Some(convert_bundle(bundle))),
        None => Ok(None),
    }
}

pub async fn read_project_doc(cwd: &Path, max_bytes: usize) -> Result<Option<ProjectDocBundle>> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let project_root = resolve_project_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let home_dir = dirs::home_dir();

    read_project_doc_with_options(&ProjectDocOptions {
        current_dir: cwd,
        project_root: &project_root,
        home_dir: home_dir.as_deref(),
        extra_instruction_files: &[],
        max_bytes,
    })
    .await
}

pub async fn get_user_instructions(
    config: &AgentConfig,
    cwd: &Path,
    skills: Option<&[SkillMetadata]>,
) -> Option<String> {
    let bundle = read_project_doc(cwd, config.project_doc_max_bytes)
        .await
        .ok()
        .flatten();

    let mut section = String::new();

    if let Some(user_inst) = &config.user_instructions {
        section.push_str("## USER INSTRUCTIONS\n");
        section.push_str(user_inst);
        section.push_str("\n\n");
    }

    if let Some(bundle) = bundle {
        section.push_str("## PROJECT DOCUMENTATION\n");
        section.push_str("Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n");
        
        for (i, segment) in bundle.sources.iter().enumerate() {
            let display_path = segment.to_string_lossy();
            let _ = std::fmt::Write::write_fmt(&mut section, format_args!("### {}. {}\n", i + 1, display_path));
            // We need the actual content here, but ProjectDocBundle already has it concatenated.
            // For a single comprehensive block, we can just use the bundle.contents if we don't need per-file headers,
            // or we could refactor to keep them separate.
        }
        section.push_str(&bundle.contents);
        section.push_str("\n\n");
    }

    if let Some(skills_text) = skills.and_then(render_skills_section) {
        section.push_str(&skills_text);
    }

    if section.is_empty() {
        None
    } else {
        Some(section)
    }
}

pub fn merge_project_docs_with_skills(
    project_doc: Option<String>,
    skills_section: Option<String>,
) -> Option<String> {
    match (project_doc, skills_section) {
        (Some(doc), Some(skills)) => Some(format!("{}\n\n{}", doc, skills)),
        (Some(doc), None) => Some(doc),
        (None, Some(skills)) => Some(skills),
        (None, None) => None,
    }
}

fn convert_bundle(bundle: InstructionBundle) -> ProjectDocBundle {
    let contents = bundle.combined_text();
    let sources = bundle
        .segments
        .iter()
        .map(|segment| segment.source.path.clone())
        .collect::<Vec<_>>();

    ProjectDocBundle {
        contents,
        sources,
        truncated: bundle.truncated,
        bytes_read: bundle.bytes_read,
    }
}

fn resolve_project_root(cwd: &Path) -> Result<PathBuf> {
    let mut cursor = canonicalize_dir(cwd)
        .with_context(|| format!("Failed to canonicalize working directory {}", cwd.display()))?;

    loop {
        let git_marker = cursor.join(".git");
        match std::fs::metadata(&git_marker) {
            Ok(_) => return Ok(cursor),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to inspect potential git root {}",
                        git_marker.display()
                    )
                });
            }
        }

        match cursor.parent() {
            Some(parent) => {
                cursor = parent.to_path_buf();
            }
            None => return Ok(cursor),
        }
    }
}

fn canonicalize_dir(path: &Path) -> Result<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Ok(canonical),
        Err(err) => {
            Err(err).with_context(|| format!("Failed to canonicalize path {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_doc(dir: &Path, content: &str) -> Result<()> {
        std::fs::write(dir.join("AGENTS.md"), content).context("write AGENTS.md")?;
        Ok(())
    }

    #[tokio::test]
    async fn returns_none_when_no_docs_present() {
        let tmp = tempdir().expect("failed to unwrap");
        let result = read_project_doc(tmp.path(), 4096)
            .await
            .expect("failed to unwrap");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn reads_doc_within_limit() {
        let tmp = tempdir().expect("failed to unwrap");
        write_doc(tmp.path(), "hello world").expect("write doc");

        let result = read_project_doc(tmp.path(), 4096)
            .await
            .expect("failed to unwrap")
            .expect("failed to unwrap");
        assert_eq!(result.contents, "hello world");
        assert_eq!(result.bytes_read, "hello world".len());
    }

    #[tokio::test]
    async fn truncates_when_limit_exceeded() {
        let tmp = tempdir().expect("failed to unwrap");
        let content = "A".repeat(64);
        write_doc(tmp.path(), &content).expect("write doc");

        let result = read_project_doc(tmp.path(), 16)
            .await
            .expect("failed to unwrap")
            .expect("failed to unwrap");
        assert!(result.truncated);
        assert_eq!(result.contents.len(), 16);
    }

    #[tokio::test]
    async fn reads_docs_from_repo_root_downwards() {
        let repo = tempdir().expect("failed to unwrap");
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").expect("failed to unwrap");
        write_doc(repo.path(), "root doc").expect("write doc");

        let nested = repo.path().join("nested/sub");
        std::fs::create_dir_all(&nested).expect("failed to unwrap");
        write_doc(&nested, "nested doc").expect("write doc");

        let bundle = read_project_doc_with_options(&ProjectDocOptions {
            current_dir: &nested,
            project_root: repo.path(),
            home_dir: None,
            extra_instruction_files: &[],
            max_bytes: 4096,
        })
        .await
        .expect("failed to unwrap")
        .expect("failed to unwrap");
        assert!(bundle.contents.contains("root doc"));
        assert!(bundle.contents.contains("nested doc"));
        assert_eq!(bundle.sources.len(), 2);
    }

    #[tokio::test]
    async fn includes_extra_instruction_files() {
        let repo = tempdir().expect("failed to unwrap");
        write_doc(repo.path(), "root doc").expect("write doc");
        let docs = repo.path().join("docs");
        std::fs::create_dir_all(&docs).expect("failed to unwrap");
        let extra = docs.join("guidelines.md");
        std::fs::write(&extra, "extra doc").expect("failed to unwrap");

        let bundle = read_project_doc_with_options(&ProjectDocOptions {
            current_dir: repo.path(),
            project_root: repo.path(),
            home_dir: None,
            extra_instruction_files: &["docs/*.md".to_owned()],
            max_bytes: 4096,
        })
        .await
        .expect("failed to unwrap")
        .expect("failed to unwrap");

        assert!(bundle.contents.contains("root doc"));
        assert!(bundle.contents.contains("extra doc"));
        assert_eq!(bundle.sources.len(), 2);
    }

    #[test]
    fn highlights_extract_bullets() {
        let bundle = ProjectDocBundle {
            contents: "- First\n- Second\n".to_owned(),
            sources: Vec::new(),
            truncated: false,
            bytes_read: 0,
        };
        let highlights = bundle.highlights(1);
        assert_eq!(highlights, vec!["First".to_owned()]);
    }
}
