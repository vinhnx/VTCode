use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::instructions::{InstructionBundle, read_instruction_bundle};

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

        self.contents
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with('-') {
                    let highlight = trimmed.trim_start_matches('-').trim();
                    if !highlight.is_empty() {
                        return Some(highlight.to_string());
                    }
                }
                None
            })
            .take(limit)
            .collect()
    }
}

pub struct ProjectDocOptions<'a> {
    pub current_dir: &'a Path,
    pub project_root: &'a Path,
    pub home_dir: Option<&'a Path>,
    pub extra_instruction_files: &'a [String],
    pub max_bytes: usize,
}

pub fn read_project_doc_with_options(
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
    )? {
        Some(bundle) => Ok(Some(convert_bundle(bundle))),
        None => Ok(None),
    }
}

pub fn read_project_doc(cwd: &Path, max_bytes: usize) -> Result<Option<ProjectDocBundle>> {
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
}

fn convert_bundle(bundle: InstructionBundle) -> ProjectDocBundle {
    let contents = bundle.combined_text();
    let sources = bundle
        .segments
        .iter()
        .map(|segment| segment.source.path.clone())
        .collect();

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

    fn write_doc(dir: &Path, content: &str) {
        std::fs::write(dir.join("AGENTS.md"), content).unwrap();
    }

    #[test]
    fn returns_none_when_no_docs_present() {
        let tmp = tempdir().unwrap();
        let result = read_project_doc(tmp.path(), 4096).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn reads_doc_within_limit() {
        let tmp = tempdir().unwrap();
        write_doc(tmp.path(), "hello world");

        let result = read_project_doc(tmp.path(), 4096).unwrap().unwrap();
        assert_eq!(result.contents, "hello world");
        assert_eq!(result.bytes_read, "hello world".len());
    }

    #[test]
    fn truncates_when_limit_exceeded() {
        let tmp = tempdir().unwrap();
        let content = "A".repeat(64);
        write_doc(tmp.path(), &content);

        let result = read_project_doc(tmp.path(), 16).unwrap().unwrap();
        assert!(result.truncated);
        assert_eq!(result.contents.len(), 16);
    }

    #[test]
    fn reads_docs_from_repo_root_downwards() {
        let repo = tempdir().unwrap();
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").unwrap();
        write_doc(repo.path(), "root doc");

        let nested = repo.path().join("nested/sub");
        std::fs::create_dir_all(&nested).unwrap();
        write_doc(&nested, "nested doc");

        let bundle = read_project_doc_with_options(&ProjectDocOptions {
            current_dir: &nested,
            project_root: repo.path(),
            home_dir: None,
            extra_instruction_files: &[],
            max_bytes: 4096,
        })
        .unwrap()
        .unwrap();
        assert!(bundle.contents.contains("root doc"));
        assert!(bundle.contents.contains("nested doc"));
        assert_eq!(bundle.sources.len(), 2);
    }

    #[test]
    fn includes_extra_instruction_files() {
        let repo = tempdir().unwrap();
        write_doc(repo.path(), "root doc");
        let docs = repo.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let extra = docs.join("guidelines.md");
        std::fs::write(&extra, "extra doc").unwrap();

        let bundle = read_project_doc_with_options(&ProjectDocOptions {
            current_dir: repo.path(),
            project_root: repo.path(),
            home_dir: None,
            extra_instruction_files: &["docs/*.md".to_string()],
            max_bytes: 4096,
        })
        .unwrap()
        .unwrap();

        assert!(bundle.contents.contains("root doc"));
        assert!(bundle.contents.contains("extra doc"));
        assert_eq!(bundle.sources.len(), 2);
    }

    #[test]
    fn highlights_extract_bullets() {
        let bundle = ProjectDocBundle {
            contents: "- First\n- Second\n".to_string(),
            sources: Vec::new(),
            truncated: false,
            bytes_read: 0,
        };
        let highlights = bundle.highlights(1);
        assert_eq!(highlights, vec!["First".to_string()]);
    }
}
