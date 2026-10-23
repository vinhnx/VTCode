//! Bounded read-only workspace file listing and previews for the sidebar.

use std::fs;
use std::path::Path;

/// Hard bounds so one sidebar render stays cheap regardless of workspace size.
const MAX_ENTRIES: usize = 400;
const MAX_DEPTH: usize = 3;
/// Preview read bound; a `usize` so buffer sizing never truncates.
const MAX_PREVIEW_BYTES: usize = 64 * 1024;

/// Directories that never carry interesting source and can be enormous.
const SKIPPED_DIR_NAMES: &[&str] = &["target", "node_modules", ".git", ".venv", "dist", "build"];

/// One workspace entry for the sidebar listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileEntry {
    /// Path relative to the workspace root, using `/` separators.
    pub rel_path: String,
    /// Nesting depth below the root; the root itself is depth 0.
    pub depth: usize,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// A bounded file preview.
pub(super) struct FilePreview {
    /// Path relative to the workspace root.
    pub rel_path: String,
    /// Decoded preview text.
    pub text: String,
    /// Whether the file is larger than the preview bound.
    pub truncated: bool,
}

/// List workspace entries breadth-first, skipping dependency and VCS
/// directories, hidden entries, and anything past the entry or depth bounds.
pub(super) fn scan_workspace(root: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    let mut queue = std::collections::VecDeque::from(vec![(root.to_path_buf(), 0_usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if entries.len() >= MAX_ENTRIES {
            break;
        }
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<(fs::DirEntry, bool)> = read_dir.flatten().map(|entry| {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            (entry, is_dir)
        }).collect();
        children.sort_by_key(|(entry, is_dir)| (!is_dir, entry.file_name()));
        for (entry, is_dir) in children {
            if entries.len() >= MAX_ENTRIES {
                break;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') || (is_dir && SKIPPED_DIR_NAMES.contains(&name)) {
                continue;
            }
            let Some(rel) = entry.path().strip_prefix(root).ok().map(|path| path.display().to_string()) else {
                continue;
            };
            entries.push(FileEntry { rel_path: rel.replace('\\', "/"), depth, is_dir });
            if is_dir && depth + 1 < MAX_DEPTH {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    entries
}

/// Read a bounded text preview of one file. Returns `None` for missing,
/// oversized-binary, or non-UTF-8 files.
pub(super) fn read_preview(root: &Path, rel_path: &str) -> Option<FilePreview> {
    let path = root.join(rel_path);
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let truncated = metadata.len() > MAX_PREVIEW_BYTES as u64;
    let bytes = if truncated {
        let mut file = fs::File::open(&path).ok()?;
        let mut buffer = vec![0_u8; MAX_PREVIEW_BYTES];
        let read = std::io::Read::read(&mut file, &mut buffer).ok()?;
        buffer.truncate(read);
        buffer
    } else {
        fs::read(&path).ok()?
    };
    if bytes.contains(&0_u8) {
        return None;
    }
    Some(FilePreview {
        rel_path: rel_path.to_string(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    #[test]
    fn scan_skips_dependency_dirs_and_hidden_entries() {
        let workspace = temp_workspace();
        let root = workspace.path();
        for dir in ["src", "src/nested", "target/debug", ".git/objects", "node_modules/pkg"] {
            fs::create_dir_all(root.join(dir)).expect("create dir");
        }
        fs::write(root.join("src/main.rs"), b"fn main() {}").expect("write");
        fs::write(root.join("src/nested/mod.rs"), b"").expect("write");
        fs::write(root.join(".hidden"), b"").expect("write");
        fs::write(root.join("target/debug/artifact"), b"").expect("write");

        let entries = scan_workspace(root);
        let paths: Vec<&str> = entries.iter().map(|entry| entry.rel_path.as_str()).collect();
        assert!(paths.contains(&"src"));
        assert!(paths.contains(&"src/main.rs"));
        assert!(paths.contains(&"src/nested/mod.rs"), "nested source files are listed");
        assert!(!paths.iter().any(|path| path.starts_with("target") || path.starts_with(".git")));
        assert!(!paths.contains(&".hidden"));
        let src = entries.iter().find(|entry| entry.rel_path == "src").expect("src entry");
        assert!(src.is_dir);
        assert_eq!(src.depth, 0);
    }

    #[test]
    fn scan_is_bounded_in_count_and_depth() {
        let workspace = temp_workspace();
        let root = workspace.path();
        for index in 0..(MAX_ENTRIES + 50) {
            fs::write(root.join(format!("file-{index:03}.txt")), b"x").expect("write");
        }
        fs::create_dir_all(root.join("a/b/c/d")).expect("deep dir");
        fs::write(root.join("a/b/c/d/buried.txt"), b"x").expect("write");

        let entries = scan_workspace(root);
        assert_eq!(entries.len(), MAX_ENTRIES, "entry count is capped");
        assert!(
            !entries.iter().any(|entry| entry.rel_path == "a/b/c/d/buried.txt"),
            "entries deeper than the depth bound are not listed"
        );
    }

    #[test]
    fn preview_reads_text_and_truncates_oversized_files() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join("notes.md"), "short notes").expect("write");
        let large = "y".repeat(MAX_PREVIEW_BYTES + 10);
        fs::write(root.join("large.log"), large).expect("write");
        fs::write(root.join("binary.bin"), [0_u8, 1, 2]).expect("write");

        let preview = read_preview(root, "notes.md").expect("text preview");
        assert_eq!(preview.text, "short notes");
        assert!(!preview.truncated);

        let preview = read_preview(root, "large.log").expect("large preview");
        assert!(preview.truncated);
        assert_eq!(preview.text.len(), MAX_PREVIEW_BYTES);

        assert!(read_preview(root, "binary.bin").is_none(), "binary files have no text preview");
        assert!(read_preview(root, "missing.txt").is_none(), "missing files have no preview");
    }
}
