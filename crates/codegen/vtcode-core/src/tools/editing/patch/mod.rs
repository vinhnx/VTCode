use std::path::Path;

use anyhow::anyhow;

mod applicator;
mod error;
mod matcher;
mod parser;
mod path;
mod semantic;

pub use error::PatchError;
#[doc(hidden)]
pub use semantic::{AstGrepBinaryOverrideGuard, set_ast_grep_binary_override_for_tests};
#[doc(hidden)]
pub(crate) use semantic::{is_binary_override_missing, resolve_ast_grep_binary_path};

/// Represents a single diff line inside a patch hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchLine {
    Context(String),
    Addition(String),
    Removal(String),
}

impl PatchLine {
    pub fn as_str(&self) -> &str {
        match self {
            PatchLine::Context(text) | PatchLine::Addition(text) | PatchLine::Removal(text) => text,
        }
    }
}

/// Represents a chunk of changes within an update operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchChunk {
    pub change_context: Option<String>,
    pub lines: Vec<PatchLine>,
    pub is_end_of_file: bool,
}

impl PatchChunk {
    pub fn lines(&self) -> &[PatchLine] {
        &self.lines
    }

    pub fn change_context(&self) -> Option<&str> {
        self.change_context.as_deref()
    }

    pub fn is_end_of_file(&self) -> bool {
        self.is_end_of_file
    }

    pub(crate) fn to_segments(&self) -> (Vec<String>, Vec<String>) {
        let cap = self.lines.len();
        let mut old_lines = Vec::with_capacity(cap);
        let mut new_lines = Vec::with_capacity(cap);

        for line in &self.lines {
            match line {
                PatchLine::Context(text) => {
                    old_lines.push(text.clone());
                    new_lines.push(text.clone());
                }
                PatchLine::Addition(text) => {
                    new_lines.push(text.clone());
                }
                PatchLine::Removal(text) => {
                    old_lines.push(text.clone());
                }
            }
        }

        (old_lines, new_lines)
    }

    pub(crate) fn has_old_lines(&self) -> bool {
        self.lines
            .iter()
            .any(|line| matches!(line, PatchLine::Context(_) | PatchLine::Removal(_)))
    }

    pub fn parse_line_number(&self) -> Option<usize> {
        let ctx = self.change_context()?;
        // Format is typically: -old_start,old_count +new_start,new_count @@
        let parts: Vec<&str> = ctx.split_whitespace().collect();
        let old_part = if !parts.is_empty() && parts[0].starts_with('-') {
            Some(parts[0])
        } else if parts.len() >= 2 && parts[1].starts_with('-') {
            Some(parts[1])
        } else {
            None
        }?;

        let range_str = old_part.strip_prefix('-')?;
        let range_parts: Vec<&str> = range_str.split(',').collect();
        let start_str = range_parts.first()?;
        start_str.parse::<usize>().ok()
    }
}

pub type PatchHunk = PatchChunk;

/// Represents a patch operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOperation {
    AddFile {
        path: String,
        content: String,
    },
    DeleteFile {
        path: String,
    },
    UpdateFile {
        path: String,
        new_path: Option<String>,
        chunks: Vec<PatchChunk>,
    },
}

/// Represents a complete patch comprised of multiple operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    operations: Vec<PatchOperation>,
}

impl Patch {
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let operations = parser::parse(input).map_err(|err| anyhow!(err))?;
        Ok(Self { operations })
    }

    pub fn operations(&self) -> &[PatchOperation] {
        &self.operations
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn into_operations(self) -> Vec<PatchOperation> {
        self.operations
    }

    pub async fn apply(&self, root: &Path) -> anyhow::Result<Vec<String>> {
        applicator::apply(root, &self.operations).await.map_err(|err| anyhow!(err))
    }
}

pub async fn render_patch_update_content(
    source_path: &Path,
    content: &str,
    chunks: &[PatchChunk],
    path: &str,
) -> anyhow::Result<String> {
    applicator::render_updated_content(source_path, content, chunks, path)
        .await
        .map_err(|err| anyhow!(err))
}

// ---------------------------------------------------------------------------
// Shape detection primitives — single source of truth for "is this a VTE
// patch?" / "is this a unified diff?" Used by tool routing, source-field
// selection, and parse-error guidance. Replaces the previously duplicated
// private `looks_like_patch_text` closures in `tool_intent` and `file_ops`.
// ---------------------------------------------------------------------------

/// Cheap shape check: does this text begin with the VT Code patch envelope
/// (`*** Begin Patch`) or a bare file-operation header (`*** Update File:`,
/// `*** Add File:`, `*** Delete File:`)? Used by tool routing and
/// source-field selection to distinguish a real patch from raw file contents
/// or a unified diff. Intentionally cheap (prefix-only) — it does **not**
/// validate the full patch.
#[must_use]
pub fn looks_like_vte_patch(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("*** Begin Patch")
        || trimmed.starts_with("*** Update File:")
        || trimmed.starts_with("*** Add File:")
        || trimmed.starts_with("*** Delete File:")
}

/// Cheap shape check: does this text look like a standard unified diff
/// (`diff --git` header, or a paired `--- `/`+++ ` file-header block)?
/// Used to produce actionable error guidance when a model submits a
/// git-style diff to `apply_patch` (which requires the `*** Begin Patch`
/// envelope). VT Code patch format never uses `--- `/`+++ ` markers, so
/// this will not false-positive on valid VTE patches.
#[must_use]
pub fn looks_like_unified_diff(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.starts_with("diff --git") {
        return true;
    }
    // Unified diffs always have a paired `--- ` / `+++ ` file-header block.
    // Scan the first dozen lines; the pair usually appears within the first 2-3.
    let mut saw_old = false;
    for line in trimmed.lines().take(12) {
        let lt = line.trim_start();
        if lt.starts_with("--- ") {
            saw_old = true;
        } else if saw_old && lt.starts_with("+++ ") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_add_file() {
        let patch = Patch::parse("*** Begin Patch\n*** Add File: hello.txt\n+hello\n*** End Patch")
            .unwrap();
        assert_eq!(patch.operations().len(), 1);
        matches!(patch.operations()[0], PatchOperation::AddFile { .. });
    }

    #[tokio::test]
    async fn apply_add_file() {
        let temp_dir = TempDir::new().unwrap();
        let patch =
            Patch::parse("*** Begin Patch\n*** Add File: file.txt\n+content\n*** End Patch")
                .unwrap();

        let result = patch.apply(temp_dir.path()).await.unwrap();
        assert_eq!(result, vec!["[1/1] Added file: file.txt (8 bytes)".to_string()]);
        let written = tokio::fs::read_to_string(temp_dir.path().join("file.txt")).await.unwrap();
        assert_eq!(written, "content\n");
    }

    // -----------------------------------------------------------------------
    // Shape-detection primitives
    // -----------------------------------------------------------------------

    #[test]
    fn looks_like_vte_patch_detects_envelope_and_bare_headers() {
        assert!(looks_like_vte_patch("*** Begin Patch\n*** End Patch"));
        assert!(looks_like_vte_patch("*** Update File: src/main.rs\n@@\n+new\n"));
        assert!(looks_like_vte_patch("*** Add File: new.txt\n+content\n"));
        assert!(looks_like_vte_patch("*** Delete File: old.txt"));
        // Leading whitespace is tolerated.
        assert!(looks_like_vte_patch("  \n*** Begin Patch\n*** End Patch"));
    }

    #[test]
    fn looks_like_vte_patch_rejects_non_patch_shapes() {
        assert!(!looks_like_vte_patch("diff --git a/f b/f\n"));
        assert!(!looks_like_vte_patch("--- a/f\n+++ b/f\n"));
        assert!(!looks_like_vte_patch("fn main() { println!(\"hi\"); }"));
        assert!(!looks_like_vte_patch(""));
    }

    #[test]
    fn looks_like_unified_diff_detects_git_diff_header() {
        assert!(looks_like_unified_diff(
            "diff --git a/src/main.rs b/src/main.rs\nindex abc..def 100644\n"
        ));
    }

    #[test]
    fn looks_like_unified_diff_detects_paired_file_headers() {
        assert!(looks_like_unified_diff(
            "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n-old\n+new\n"
        ));
    }

    #[test]
    fn looks_like_unified_diff_rejects_vte_patch() {
        // A valid VTE patch must never be mistaken for a unified diff.
        assert!(!looks_like_unified_diff(
            "*** Begin Patch\n*** Update File: f.rs\n@@\n-old\n+new\n*** End Patch"
        ));
        assert!(!looks_like_unified_diff(""));
        assert!(!looks_like_unified_diff("fn main() {}"));
    }
}
