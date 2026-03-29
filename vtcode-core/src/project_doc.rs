use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::instructions::{
    InstructionBundle, InstructionDiscoveryOptions, InstructionSegment,
    extract_instruction_highlights, format_instruction_path, read_instruction_bundle,
    render_instruction_summary_markdown,
};
use crate::persistent_memory::{PersistentMemoryExcerpt, read_persistent_memory_excerpt};
use crate::skills::model::SkillMetadata;
use crate::utils::file_utils::canonicalize_with_context;
use vtcode_config::core::AgentConfig;

pub const PROJECT_DOC_SEPARATOR: &str = "\n\n--- project-doc ---\n\n";
pub const PERSISTENT_MEMORY_SEPARATOR: &str = "\n\n--- persistent-memory ---\n\n";
const PROJECT_DOC_SUMMARY_TITLE: &str = "PROJECT DOCUMENTATION";
const PROJECT_DOC_TRUNCATION_NOTE: &str = "Some instruction files exceeded the configured prompt budget and were indexed instead of fully inlined.";
const PERSISTENT_MEMORY_TRUNCATION_NOTE: &str =
    "Persistent memory was truncated to the configured startup excerpt budget.";

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
    pub exclude_patterns: &'a [String],
    pub match_paths: &'a [PathBuf],
    pub import_max_depth: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionAppendixBundle {
    pub contents: String,
    pub project_doc: Option<ProjectDocBundle>,
    pub persistent_memory: Option<PersistentMemoryExcerpt>,
    pub project_root: PathBuf,
    pub home_dir: Option<PathBuf>,
}

pub async fn read_project_doc_with_options(
    options: &ProjectDocOptions<'_>,
) -> Result<Option<ProjectDocBundle>> {
    if options.max_bytes == 0 {
        return Ok(None);
    }

    match read_instruction_bundle(
        &InstructionDiscoveryOptions {
            current_dir: options.current_dir,
            project_root: options.project_root,
            home_dir: options.home_dir,
            extra_patterns: options.extra_instruction_files,
            fallback_filenames: options.fallback_filenames,
            exclude_patterns: options.exclude_patterns,
            match_paths: options.match_paths,
            import_max_depth: options.import_max_depth,
        },
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
        exclude_patterns: &[],
        match_paths: &[],
        import_max_depth: 5,
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
    build_instruction_appendix_with_context(config, active_dir, &[]).await
}

pub async fn build_instruction_appendix_with_context(
    config: &AgentConfig,
    active_dir: &Path,
    match_paths: &[PathBuf],
) -> Option<String> {
    load_instruction_appendix(config, active_dir, match_paths)
        .await
        .map(|bundle| bundle.contents)
}

pub async fn load_instruction_appendix(
    config: &AgentConfig,
    active_dir: &Path,
    match_paths: &[PathBuf],
) -> Option<InstructionAppendixBundle> {
    let project_root =
        resolve_project_root(active_dir).unwrap_or_else(|_| active_dir.to_path_buf());
    let home_dir = dirs::home_dir();
    let bundle = read_project_doc_with_options(&ProjectDocOptions {
        current_dir: active_dir,
        project_root: &project_root,
        home_dir: home_dir.as_deref(),
        extra_instruction_files: &config.instruction_files,
        fallback_filenames: &config.project_doc_fallback_filenames,
        exclude_patterns: &config.instruction_excludes,
        match_paths,
        import_max_depth: config.instruction_import_max_depth,
        max_bytes: config.instruction_max_bytes,
    })
    .await
    .ok()
    .flatten();
    let persistent_memory =
        read_persistent_memory_excerpt(&config.persistent_memory, &project_root)
            .await
            .ok()
            .flatten();

    let contents = render_instruction_appendix(
        config.user_instructions.as_deref(),
        bundle.as_ref(),
        persistent_memory.as_ref(),
        &project_root,
        home_dir.as_deref(),
    )?;

    Some(InstructionAppendixBundle {
        contents,
        project_doc: bundle,
        persistent_memory,
        project_root,
        home_dir,
    })
}

pub fn render_instruction_appendix(
    user_instructions: Option<&str>,
    bundle: Option<&ProjectDocBundle>,
    persistent_memory: Option<&PersistentMemoryExcerpt>,
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

        if bundle.truncated {
            section.push_str(
                render_instruction_summary_markdown(
                    PROJECT_DOC_SUMMARY_TITLE,
                    &bundle.segments,
                    true,
                    project_root,
                    home_dir,
                    12,
                    PROJECT_DOC_TRUNCATION_NOTE,
                )
                .trim_end(),
            );
        } else {
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
        }
    }

    if let Some(memory) = persistent_memory
        && !memory.contents.trim().is_empty()
    {
        if !section.is_empty() {
            section.push_str(PERSISTENT_MEMORY_SEPARATOR);
        }

        section.push_str("## PERSISTENT MEMORY\n\n");
        section.push_str(memory.contents.trim());
        if memory.truncated {
            section.push_str("\n\n_");
            section.push_str(PERSISTENT_MEMORY_TRUNCATION_NOTE);
            section.push('_');
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
    use crate::instructions::{InstructionScope, InstructionSource, InstructionSourceKind};
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
            exclude_patterns: &[],
            match_paths: &[],
            import_max_depth: 5,
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

        let appendix = build_instruction_appendix_with_context(
            &config,
            &nested,
            &[repo.path().join("nested/sub/file.rs")],
        )
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
        write_doc(
            repo.path(),
            "- Root summary\n\nThis detail should stay out of the prompt appendix.\n",
        )
        .expect("write doc");

        let config = AgentConfig {
            instruction_max_bytes: 16,
            ..Default::default()
        };

        let appendix = build_instruction_appendix(&config, repo.path())
            .await
            .expect("instruction appendix");

        assert!(appendix.contains("## PROJECT DOCUMENTATION"));
        assert!(appendix.contains("### Instruction map"));
        assert!(appendix.contains("### On-demand loading"));
        assert!(appendix.contains("Some instruction files exceeded the configured prompt budget"));
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
            exclude_patterns: &[],
            match_paths: &[],
            import_max_depth: 5,
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
                    kind: InstructionSourceKind::Agents,
                    matched: false,
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

    #[tokio::test]
    async fn instruction_appendix_includes_persistent_memory_after_authored_guidance() {
        let repo = tempdir().expect("repo");
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
        std::fs::write(repo.path().join(".vtcode-project"), "repo").expect("project name");
        write_doc(repo.path(), "root doc").expect("write root doc");

        let memory_dir = repo.path().join(".memory-root");
        let config = AgentConfig {
            persistent_memory: vtcode_config::core::PersistentMemoryConfig {
                enabled: true,
                directory_override: Some(memory_dir.display().to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let project_memory_dir = memory_dir.join("projects").join("repo").join("memory");
        std::fs::create_dir_all(&project_memory_dir).expect("memory dir");
        std::fs::write(
            project_memory_dir.join("memory_summary.md"),
            "# VT Code Memory Summary\n\n- remembered detail\n",
        )
        .expect("write memory summary");

        let appendix = build_instruction_appendix(&config, repo.path())
            .await
            .expect("instruction appendix");

        let project_doc_idx = appendix.find("root doc").expect("project doc");
        let memory_idx = appendix.find("remembered detail").expect("memory detail");
        assert!(project_doc_idx < memory_idx);
        assert!(appendix.contains("--- persistent-memory ---"));
    }
}
