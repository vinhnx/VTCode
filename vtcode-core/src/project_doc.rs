use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::instructions::{
    InstructionBundle, InstructionSegment, extract_instruction_highlights, read_instruction_bundle,
    render_instruction_markdown,
};
use crate::skills::model::SkillMetadata;
use crate::skills::render::render_skills_section;
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
    cwd: &Path,
    skills: Option<&[SkillMetadata]>,
) -> Option<String> {
    let project_root = resolve_project_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let home_dir = dirs::home_dir();
    let bundle = read_project_doc_with_options(&ProjectDocOptions {
        current_dir: cwd,
        project_root: &project_root,
        home_dir: home_dir.as_deref(),
        extra_instruction_files: &[],
        fallback_filenames: &config.project_doc_fallback_filenames,
        max_bytes: config.project_doc_max_bytes,
    })
    .await
    .ok()
    .flatten();

    let mut section = String::with_capacity(1024);

    if let Some(user_inst) = &config.user_instructions {
        section.push_str("## USER INSTRUCTIONS\n");
        section.push_str(user_inst);
        section.push_str("\n\n");
    }

    if let Some(bundle) = bundle {
        section.push_str(&render_instruction_markdown(
            "PROJECT DOCUMENTATION",
            &bundle.segments,
            bundle.truncated,
            &project_root,
            home_dir.as_deref(),
            3,
            "project documentation was truncated due to size limits. Review the source files for full details.",
        ));
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
    async fn renders_instruction_map_and_segment_headers() {
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

        assert!(instructions.contains("### Instruction map"));
        assert!(instructions.contains("- 1. AGENTS.md (workspace)"));
        assert!(instructions.contains("- 2. nested/sub/AGENTS.md (workspace)"));
        assert!(instructions.contains("### Key points"));
        assert!(instructions.contains("### 1. AGENTS.md (workspace)"));
        assert!(instructions.contains("### 2. nested/sub/AGENTS.md (workspace)"));
        assert!(instructions.contains("Root summary"));
        assert!(instructions.contains("Nested summary"));
    }
}
