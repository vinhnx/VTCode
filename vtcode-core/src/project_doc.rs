use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use tracing::warn;

const DOC_FILENAME: &str = "AGENTS.md";
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

pub fn read_project_doc(cwd: &Path, max_bytes: usize) -> Result<Option<ProjectDocBundle>> {
    if max_bytes == 0 {
        return Ok(None);
    }

    let paths = discover_project_doc_paths(cwd)?;
    if paths.is_empty() {
        return Ok(None);
    }

    let mut remaining = max_bytes;
    let mut truncated = false;
    let mut parts: Vec<String> = Vec::new();
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut total_bytes = 0usize;

    for path in paths {
        if remaining == 0 {
            truncated = true;
            break;
        }

        let file = match File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Failed to open project documentation at {}", path.display())
                });
            }
        };

        let metadata = file
            .metadata()
            .with_context(|| format!("Failed to read metadata for {}", path.display()))?;

        let mut reader = io::BufReader::new(file).take(remaining as u64);
        let mut data = Vec::new();
        reader.read_to_end(&mut data).with_context(|| {
            format!(
                "Failed to read project documentation from {}",
                path.display()
            )
        })?;

        if metadata.len() as usize > remaining {
            truncated = true;
            warn!(
                "Project doc `{}` exceeds remaining budget ({} bytes) - truncating.",
                path.display(),
                remaining
            );
        }

        if data.iter().all(|byte| byte.is_ascii_whitespace()) {
            remaining = remaining.saturating_sub(data.len());
            continue;
        }

        let text = String::from_utf8_lossy(&data).to_string();
        if !text.trim().is_empty() {
            total_bytes += data.len();
            remaining = remaining.saturating_sub(data.len());
            sources.push(path);
            parts.push(text);
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        let contents = parts.join("\n\n");
        Ok(Some(ProjectDocBundle {
            contents,
            sources,
            truncated,
            bytes_read: total_bytes,
        }))
    }
}

pub fn discover_project_doc_paths(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut dir = cwd.to_path_buf();
    if let Ok(canonical) = dir.canonicalize() {
        dir = canonical;
    }

    let mut chain: Vec<PathBuf> = vec![dir.clone()];
    let mut git_root: Option<PathBuf> = None;
    let mut cursor = dir.clone();

    while let Some(parent) = cursor.parent() {
        let git_marker = cursor.join(".git");
        match std::fs::metadata(&git_marker) {
            Ok(_) => {
                git_root = Some(cursor.clone());
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to inspect potential git root {}",
                        git_marker.display()
                    )
                });
            }
        }

        chain.push(parent.to_path_buf());
        cursor = parent.to_path_buf();
    }

    let search_dirs: Vec<PathBuf> = if let Some(root) = git_root {
        let mut dirs = Vec::new();
        let mut saw_root = false;
        for path in chain.iter().rev() {
            if !saw_root {
                if path == &root {
                    saw_root = true;
                } else {
                    continue;
                }
            }
            dirs.push(path.clone());
        }
        dirs
    } else {
        vec![dir]
    };

    let mut found = Vec::new();
    for directory in search_dirs {
        let candidate = directory.join(DOC_FILENAME);
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                let kind = metadata.file_type();
                if kind.is_file() || kind.is_symlink() {
                    found.push(candidate);
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to inspect project doc candidate {}",
                        candidate.display()
                    )
                });
            }
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_doc(dir: &Path, content: &str) {
        std::fs::write(dir.join(DOC_FILENAME), content).unwrap();
    }

    #[test]
    fn returns_none_when_no_docs_present() {
        let tmp = TempDir::new().unwrap();
        let result = read_project_doc(tmp.path(), 4096).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn reads_doc_within_limit() {
        let tmp = TempDir::new().unwrap();
        write_doc(tmp.path(), "hello world");

        let result = read_project_doc(tmp.path(), 4096).unwrap().unwrap();
        assert_eq!(result.contents, "hello world");
        assert_eq!(result.bytes_read, "hello world".len());
    }

    #[test]
    fn truncates_when_limit_exceeded() {
        let tmp = TempDir::new().unwrap();
        let content = "A".repeat(64);
        write_doc(tmp.path(), &content);

        let result = read_project_doc(tmp.path(), 16).unwrap().unwrap();
        assert!(result.truncated);
        assert_eq!(result.contents.len(), 16);
    }

    #[test]
    fn reads_docs_from_repo_root_downwards() {
        let repo = TempDir::new().unwrap();
        std::fs::write(repo.path().join(".git"), "gitdir: /tmp/git").unwrap();
        write_doc(repo.path(), "root doc");

        let nested = repo.path().join("nested/sub");
        std::fs::create_dir_all(&nested).unwrap();
        write_doc(&nested, "nested doc");

        let bundle = read_project_doc(&nested, 4096).unwrap().unwrap();
        assert!(bundle.contents.contains("root doc"));
        assert!(bundle.contents.contains("nested doc"));
        assert_eq!(bundle.sources.len(), 2);
    }

    #[test]
    fn extracts_highlights() {
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
