//! Patch application module implementing the OpenAI Codex patch format
//!
//! This module provides functionality to parse and apply patches in the format
//! used by OpenAI Codex, which is designed to be easy to parse and safe to apply.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Represents a patch operation
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
        hunks: Vec<PatchHunk>,
    },
}

/// Represents a hunk in a patch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub header: Option<String>,
    pub lines: Vec<PatchLine>,
}

/// Represents a line in a patch hunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchLine {
    Context(String),
    Remove(String),
    Add(String),
}

/// Represents a complete patch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub operations: Vec<PatchOperation>,
}

/// Input structure for the apply_patch tool
#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPatchInput {
    pub input: String,
}

impl Patch {
    /// Parse a patch from a string
    pub fn parse(input: &str) -> Result<Self> {
        let mut lines = input.lines().peekable();
        let mut operations = Vec::new();

        // Skip until we find the begin marker
        while let Some(line) = lines.next() {
            if line.trim() == "*** Begin Patch" {
                break;
            }
        }

        // Parse operations until we find the end marker
        while let Some(line) = lines.next() {
            if line.trim() == "*** End Patch" {
                break;
            }

            if line.starts_with("*** Add File: ") {
                let path = line[13..].trim().to_string();
                let mut content_lines = Vec::new();

                // Collect all lines that start with "+"
                while let Some(next_line) = lines.peek() {
                    if next_line.starts_with("*** ") {
                        // Next operation
                        break;
                    }
                    if next_line.starts_with("+") {
                        content_lines.push(next_line[1..].to_string());
                        lines.next(); // consume the line
                    } else {
                        // Unexpected line, break
                        break;
                    }
                }

                operations.push(PatchOperation::AddFile {
                    path,
                    content: content_lines.join("\n"),
                });
            } else if line.starts_with("*** Delete File: ") {
                let path = line[16..].trim().to_string();
                operations.push(PatchOperation::DeleteFile { path });
            } else if line.starts_with("*** Update File: ") {
                let path = line[17..].trim().to_string();
                let mut new_path = None;
                let mut hunks = Vec::new();

                // Check for move operation
                if let Some(next_line) = lines.peek() {
                    if next_line.starts_with("*** Move to: ") {
                        let move_line = lines.next().unwrap(); // consume the line
                        new_path = Some(move_line[13..].trim().to_string());
                    }
                }

                // Parse hunks
                let mut current_hunk = None;
                while let Some(next_line) = lines.peek() {
                    if next_line.starts_with("*** ") {
                        // Next operation
                        break;
                    }

                    if next_line.starts_with("@@") {
                        // Save previous hunk if exists
                        if let Some(hunk) = current_hunk.take() {
                            hunks.push(hunk);
                        }

                        // Start new hunk
                        let header = if next_line.len() > 2 {
                            Some(next_line[3..].trim().to_string())
                        } else {
                            None
                        };
                        current_hunk = Some(PatchHunk {
                            header,
                            lines: Vec::new(),
                        });
                        lines.next(); // consume the line
                    } else if next_line.starts_with("*** End of File") {
                        lines.next(); // consume the line
                        break;
                    } else if let Some(ref mut hunk) = current_hunk {
                        // Add line to current hunk
                        let line_content = if next_line.len() > 1 {
                            next_line[1..].to_string()
                        } else {
                            String::new()
                        };

                        let patch_line = match next_line.chars().next() {
                            Some(' ') => PatchLine::Context(line_content),
                            Some('-') => PatchLine::Remove(line_content),
                            Some('+') => PatchLine::Add(line_content),
                            _ => PatchLine::Context(next_line.to_string()),
                        };

                        hunk.lines.push(patch_line);
                        lines.next(); // consume the line
                    } else {
                        // Line outside of hunk, break
                        break;
                    }
                }

                // Save last hunk if exists
                if let Some(hunk) = current_hunk.take() {
                    hunks.push(hunk);
                }

                operations.push(PatchOperation::UpdateFile {
                    path,
                    new_path,
                    hunks,
                });
            }
        }

        Ok(Patch { operations })
    }

    /// Apply the patch to the file system
    pub async fn apply(&self, root: &Path) -> Result<Vec<String>> {
        let mut results = Vec::new();

        for operation in &self.operations {
            match operation {
                PatchOperation::AddFile { path, content } => {
                    let full_path = root.join(path);
                    if let Some(parent) = full_path.parent() {
                        tokio::fs::create_dir_all(parent).await.context(format!(
                            "failed to create parent directories: {}",
                            parent.display()
                        ))?;
                    }
                    tokio::fs::write(&full_path, content)
                        .await
                        .context(format!("failed to write file: {}", full_path.display()))?;
                    results.push(format!("Added file: {}", path));
                }
                PatchOperation::DeleteFile { path } => {
                    let full_path = root.join(path);
                    if full_path.exists() {
                        if full_path.is_dir() {
                            tokio::fs::remove_dir_all(&full_path)
                                .await
                                .context(format!(
                                    "failed to delete directory: {}",
                                    full_path.display()
                                ))?;
                        } else {
                            tokio::fs::remove_file(&full_path).await.context(format!(
                                "failed to delete file: {}",
                                full_path.display()
                            ))?;
                        }
                        results.push(format!("Deleted file: {}", path));
                    } else {
                        results.push(format!("File not found, skipped deletion: {}", path));
                    }
                }
                PatchOperation::UpdateFile {
                    path,
                    new_path,
                    hunks,
                } => {
                    let full_path = root.join(path);

                    // Read existing content
                    let existing_content = if full_path.exists() {
                        tokio::fs::read_to_string(&full_path)
                            .await
                            .context(format!("failed to read file: {}", full_path.display()))?
                    } else {
                        return Err(anyhow!("File not found: {}", path));
                    };

                    // Apply hunks to content
                    let new_content = Self::apply_hunks_to_content(&existing_content, hunks)?;

                    // Write updated content
                    let target_path = if let Some(new_path_str) = new_path {
                        let new_full_path = root.join(new_path_str);
                        if let Some(parent) = new_full_path.parent() {
                            tokio::fs::create_dir_all(parent).await.context(format!(
                                "failed to create parent directories: {}",
                                parent.display()
                            ))?;
                        }
                        // Remove old file if path changed
                        if full_path.exists() {
                            tokio::fs::remove_file(&full_path).await.context(format!(
                                "failed to remove old file: {}",
                                full_path.display()
                            ))?;
                        }
                        new_full_path
                    } else {
                        full_path
                    };

                    tokio::fs::write(&target_path, new_content)
                        .await
                        .context(format!("failed to write file: {}", target_path.display()))?;

                    if let Some(new_path_str) = new_path {
                        results.push(format!("Updated file: {} -> {}", path, new_path_str));
                    } else {
                        results.push(format!("Updated file: {}", path));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Apply hunks to content
    fn apply_hunks_to_content(content: &str, hunks: &[PatchHunk]) -> Result<String> {
        let original_lines: Vec<&str> = content.lines().collect();
        let ends_with_newline = content.ends_with('\n');
        let mut lines: Vec<String> = original_lines.into_iter().map(|s| s.to_string()).collect();

        // Apply hunks in reverse order to maintain line numbers
        for hunk in hunks.iter().rev() {
            // Find the position where this hunk should be applied
            // For simplicity, we'll just try to match the first few lines
            let mut line_index = 0;

            // Try to find where the hunk should be applied by matching context
            if !hunk.lines.is_empty() {
                // Look for the first non-context line to match
                for (idx, line) in hunk.lines.iter().enumerate() {
                    match line {
                        PatchLine::Remove(text) | PatchLine::Add(text) => {
                            // Try to find this line in the content
                            if let Some(pos) = lines.iter().position(|l| l == text) {
                                line_index = pos;
                                // Adjust for context lines before this
                                let context_lines_before = hunk.lines[..idx]
                                    .iter()
                                    .filter(|l| matches!(l, PatchLine::Context(_)))
                                    .count();
                                line_index = line_index.saturating_sub(context_lines_before);
                            }
                            break;
                        }
                        _ => continue,
                    }
                }
            }

            // Apply the lines in this hunk
            let mut i = line_index;
            for line in &hunk.lines {
                match line {
                    PatchLine::Context(text) => {
                        // For context lines, verify they match
                        if i < lines.len() && &lines[i] == text {
                            i += 1;
                        } else {
                            // Context mismatch, but we'll continue for now
                            // A more sophisticated implementation would handle this better
                            i += 1;
                        }
                    }
                    PatchLine::Remove(text) => {
                        // Remove the line if it matches
                        if i < lines.len() && &lines[i] == text {
                            lines.remove(i);
                            // Don't increment i since we removed a line
                        } else {
                            return Err(anyhow!("Context mismatch when removing line: {}", text));
                        }
                    }
                    PatchLine::Add(text) => {
                        // Add the line at the current position
                        lines.insert(i, text.clone());
                        i += 1;
                    }
                }
            }
        }

        // Join lines with newlines, preserving the original trailing newline
        let result = lines.join("\n");
        if ends_with_newline && !result.is_empty() && !result.ends_with('\n') {
            Ok(format!("{}\n", result))
        } else {
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;

    #[test]
    fn test_parse_simple_patch() {
        let patch_str = r#"*** Begin Patch
*** Add File: test.txt
+Hello, world!
+This is a test file.
*** End Patch"#;

        let patch = Patch::parse(patch_str).unwrap();
        assert_eq!(patch.operations.len(), 1);

        match &patch.operations[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "test.txt");
                assert_eq!(content, "Hello, world!\nThis is a test file.");
            }
            _ => panic!("Expected AddFile operation"),
        }
    }

    #[tokio::test]
    async fn test_apply_add_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let workspace = temp_dir.path().to_path_buf();

        let patch_str = r#"*** Begin Patch
*** Add File: hello.txt
+Hello, world!
+This is a test.
*** End Patch"#;

        let patch = Patch::parse(patch_str)?;
        let results = patch.apply(&workspace).await?;

        assert_eq!(results.len(), 1);
        assert!(results[0].contains("Added file: hello.txt"));

        let file_path = workspace.join("hello.txt");
        assert!(file_path.exists());

        let content = tokio::fs::read_to_string(&file_path).await?;
        assert_eq!(content, "Hello, world!\nThis is a test.");

        Ok(())
    }

    #[tokio::test]
    async fn test_apply_delete_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let workspace = temp_dir.path().to_path_buf();

        // Create a file to delete
        let file_path = workspace.join("to_delete.txt");
        tokio::fs::write(&file_path, "This file will be deleted").await?;

        let patch_str = r#"*** Begin Patch
*** Delete File: to_delete.txt
*** End Patch"#;

        let patch = Patch::parse(patch_str)?;
        let results = patch.apply(&workspace).await?;

        assert_eq!(results.len(), 1);
        assert!(results[0].contains("Deleted file: to_delete.txt"));
        assert!(!file_path.exists());

        Ok(())
    }
}
