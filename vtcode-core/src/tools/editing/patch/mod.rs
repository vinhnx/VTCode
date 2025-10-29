use std::path::Path;

use anyhow::anyhow;

mod applicator;
mod error;
mod matcher;
mod parser;
mod path;

pub use error::PatchError;

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
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();

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
        applicator::apply(root, &self.operations)
            .await
            .map_err(|err| anyhow!(err))
    }
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
        assert_eq!(result, vec!["Added file: file.txt".to_string()]);
        let written = tokio::fs::read_to_string(temp_dir.path().join("file.txt"))
            .await
            .unwrap();
        assert_eq!(written, "content");
    }
}
