use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::instructions::{
    InstructionBundle, InstructionSegment, extract_instruction_highlights, format_instruction_path,
    read_instruction_bundle,
};
use crate::skills::model::SkillMetadata;
use crate::utils::file_utils::canonicalize_with_context;
use vtcode_config::core::AgentConfig;

pub const PROJECT_DOC_SEPARATOR: &str = "\n\n--- project-doc ---\n\n";

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDocBundle {
    pub contents: String,
    pub sources: Vec<PathBuf>,
    pub segments: Vec<InstructionSegment>,
    pub truncated: bool,
    pub bytes_read: usize,
}

impl ProjectDocBundle {
    pub fn highlights(&self, limit: usize) -> Vec<String> {
        extract_instruction_highlights(&self.segments, limit)
    }
}

pub struct ProjectDocOptions<'a> {
    pub current_dir: &'a Path,
    pub project_root: &'a Path,
    pub home_dir: Option<&'a Path>,
    pub extra_instruction_files: &'a [String],
    pub fallback_filenames: &'a [String],
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
        options.fallback_filenames,
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
        fallback_filenames: &[],
        max_bytes,
    })
    .await
}

pub async fn get_user_instructions(
    config: &AgentConfig,
    active_dir: &Path,
    _skills: Option<&[SkillMetadata]>,
) -> Option<String> {
    build_instruction_appendix(config, active_dir).await
}

pub async fn build_instruction_appendix(config: &AgentConfig, active_dir: &Path) -> Option<String> {
    let project_root =
        resolve_project_root(active_dir).unwrap_or_else(|_| active_dir.to_path_buf());
    let home_dir = dirs::home_dir();
    let bundle = read_project_doc_with_options(&ProjectDocOptions {
        current_dir: active_dir,
        project_root: &project_root,
        home_dir: home_dir.as_deref(),
        extra_instruction_files: &config.instruction_files,
        fallback_filenames: &config.project_doc_fallback_filenames,
        max_bytes: config.instruction_max_bytes,
    })
    .await
    .ok()
    .flatten();

    render_instruction_appendix(
        config.user_instructions.as_deref(),
        bundle.as_ref(),
        &project_root,
        home_dir.as_deref(),
    )
}

pub fn render_instruction_appendix(
    user_instructions: Option<&str>,
    bundle: Option<&ProjectDocBundle>,
    project_root: &Path,
    home_dir: Option<&Path>,
) -> Option<String> {
    let mut section = String::with_capacity(1024);

    if let Some(user_inst) = user_instructions.map(str::trim)
        && !user_inst.is_empty()
    {
        section.push_str(user_inst);
    }

    if let Some(bundle) = bundle
        && !bundle.segments.is_empty()
    {
        if !section.is_empty() {
            section.push_str(PROJECT_DOC_SEPARATOR);
        }

        let multiple_sources = bundle.segments.len() > 1;
        for (index, segment) in bundle.segments.iter().enumerate() {
            if index > 0 {
                section.push_str("\n\n---\n\n");
            }

            if multiple_sources {
                section.push('[');
                section.push_str(&format_instruction_path(
                    &segment.source.path,
                    project_root,
                    home_dir,
                ));
                section.push_str("]\n");
            }

            section.push_str(segment.contents.trim());
        }

        if bundle.truncated {
            section.push_str("\n\n[project-doc truncated]");
        }
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
    let segments = bundle.segments;
    let sources = segments
        .iter()
        .map(|segment| segment.source.path.clone())
        .collect::<Vec<_>>();

    ProjectDocBundle {
        contents,
        sources,
        segments,
        truncated: bundle.truncated,
        bytes_read: bundle.bytes_read,
    }
}

fn resolve_project_root(cwd: &Path) -> Result<PathBuf> {
    let mut cursor = canonicalize_with_context(cwd, "working directory")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instructions::{InstructionScope, InstructionSource};
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
            fallback_filenames: &[],
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
    async fn instruction_appendix_uses_instruction_hierarchy_scope_and_budget() {
        let repo = tempdir().expect("repo");
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").expect("write git");
        write_doc(repo.path(), "root doc").expect("write root doc");

        let nested = repo.path().join("nested/sub");
        std::fs::create_dir_all(&nested).expect("create nested");
        write_doc(&nested, "nested doc").expect("write nested doc");

        let extra_dir = repo.path().join("docs");
        std::fs::create_dir_all(&extra_dir).expect("create docs");
        std::fs::write(extra_dir.join("guidelines.md"), "extra doc").expect("write extra doc");

        let config = AgentConfig {
            user_instructions: Some("user note".to_string()),
            instruction_files: vec!["docs/*.md".to_string()],
            instruction_max_bytes: 4096,
            project_doc_max_bytes: 1,
            ..Default::default()
        };

        let appendix = build_instruction_appendix(&config, &nested)
            .await
            .expect("instruction appendix");

        assert!(appendix.starts_with("user note"));
        assert!(appendix.contains("--- project-doc ---"));
        assert!(appendix.contains("[AGENTS.md]"));
        assert!(appendix.contains("[docs/guidelines.md]"));
        assert!(appendix.contains("[nested/sub/AGENTS.md]"));
        assert!(appendix.contains("root doc"));
        assert!(appendix.contains("extra doc"));
        assert!(appendix.contains("nested doc"));
    }

    #[tokio::test]
    async fn instruction_appendix_returns_none_when_empty() {
        let tmp = tempdir().expect("tmp");
        let appendix = build_instruction_appendix(&AgentConfig::default(), tmp.path()).await;
        assert!(appendix.is_none());
    }

    #[tokio::test]
    async fn instruction_appendix_marks_truncation() {
        let repo = tempdir().expect("repo");
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").expect("write git");
        write_doc(repo.path(), &"A".repeat(128)).expect("write doc");

        let config = AgentConfig {
            instruction_max_bytes: 16,
            ..Default::default()
        };

        let appendix = build_instruction_appendix(&config, repo.path())
            .await
            .expect("instruction appendix");

        assert!(appendix.contains("[project-doc truncated]"));
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
            fallback_filenames: &[],
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
            segments: vec![InstructionSegment {
                source: InstructionSource {
                    path: PathBuf::from("AGENTS.md"),
                    scope: InstructionScope::Workspace,
                },
                contents: "- First\n- Second\n".to_owned(),
            }],
            truncated: false,
            bytes_read: 0,
        };
        let highlights = bundle.highlights(1);
        assert_eq!(highlights, vec!["First".to_owned()]);
    }

    #[tokio::test]
    async fn renders_compact_instruction_appendix() {
        let repo = tempdir().expect("failed to unwrap");
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").expect("failed to unwrap");
        write_doc(
            repo.path(),
            "- Root summary\n\nFollow the repository-level guidance first.\n",
        )
        .expect("write doc");

        let nested = repo.path().join("nested/sub");
        std::fs::create_dir_all(&nested).expect("failed to unwrap");
        write_doc(
            &nested,
            "- Nested summary\n\nFollow the nested guidance last.\n",
        )
        .expect("write doc");

        let instructions = get_user_instructions(&AgentConfig::default(), &nested, None)
            .await
            .expect("expected instructions");

        assert!(instructions.contains("[AGENTS.md]"));
        assert!(instructions.contains("[nested/sub/AGENTS.md]"));
        assert!(instructions.contains("Root summary"));
        assert!(instructions.contains("Nested summary"));
        assert!(!instructions.contains("### Instruction map"));
        assert!(!instructions.contains("### Key points"));
        assert!(!instructions.contains("--- project-doc ---"));
    }
}
