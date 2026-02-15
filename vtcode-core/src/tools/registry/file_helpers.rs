//! File operation helpers and the edit_file tool
//!
//! This module provides convenience methods for common file operations and implements
//! the `edit_file` tool, which is optimized for small, surgical edits (≤800 chars, ≤40 lines).
//! For larger or multi-file changes, use `apply_patch` instead.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::fs;

use crate::utils::path::resolve_workspace_path;

use crate::config::constants::tools;
use crate::tools::grep_file::GrepSearchResult;
use crate::tools::types::EditInput;

use super::ToolRegistry;
use super::utils;

const EDIT_FILE_MAX_CHARS: usize = 800;
const EDIT_FILE_MAX_LINES: usize = 40;
const EDIT_FILE_MAX_BYTES: u64 = 1_048_576;

fn line_prefix_len(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut i = 0;
    if bytes[i] == b'L' {
        i += 1;
    }

    let start_digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    if i == start_digits || i >= bytes.len() || bytes[i] != b':' {
        return None;
    }

    i += 1;
    if i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }

    Some(i)
}

fn strip_line_prefixes(text: &str) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return (text.to_string(), false);
    }

    let mut has_prefix = false;
    let mut all_prefixed = true;

    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }

        if line_prefix_len(line).is_some() {
            has_prefix = true;
        } else {
            all_prefixed = false;
        }
    }

    if !has_prefix || !all_prefixed {
        return (text.to_string(), false);
    }

    let stripped = lines
        .iter()
        .map(|line| match line_prefix_len(line) {
            Some(prefix_len) => &line[prefix_len..],
            None => *line,
        })
        .collect::<Vec<_>>()
        .join("\n");

    (stripped, true)
}

impl ToolRegistry {
    pub async fn read_file(&self, args: Value) -> Result<Value> {
        self.execute_tool(tools::READ_FILE, args).await
    }

    pub async fn write_file(&self, args: Value) -> Result<Value> {
        self.execute_tool(tools::WRITE_FILE, args).await
    }

    pub async fn create_file(&self, args: Value) -> Result<Value> {
        self.execute_tool(tools::CREATE_FILE, args).await
    }

    pub async fn edit_file(&self, args: Value) -> Result<Value> {
        let input: EditInput = serde_json::from_value(args).context("invalid edit_file args")?;

        let (effective_old_str, stripped_old) = strip_line_prefixes(&input.old_str);
        let (effective_new_str, stripped_new) = strip_line_prefixes(&input.new_str);

        let old_len = effective_old_str.len();
        let new_len = effective_new_str.len();
        let old_lines = effective_old_str.lines().count();
        let new_lines = effective_new_str.lines().count();

        if old_len > EDIT_FILE_MAX_CHARS
            || new_len > EDIT_FILE_MAX_CHARS
            || old_lines > EDIT_FILE_MAX_LINES
            || new_lines > EDIT_FILE_MAX_LINES
        {
            return Err(anyhow!(
                "edit_file is limited to small literal replacements (≤ {lines} lines or ≤ {chars} characters). Use apply_patch for larger or multi-file edits.",
                lines = EDIT_FILE_MAX_LINES,
                chars = EDIT_FILE_MAX_CHARS,
            ));
        }

        let requested_path = PathBuf::from(&input.path);
        let canonical_path = resolve_workspace_path(self.workspace_root(), &requested_path)
            .with_context(|| format!("Failed to resolve path: {}", requested_path.display()))?;

        let metadata = fs::metadata(&canonical_path)
            .await
            .with_context(|| format!("Cannot read file metadata: {}", canonical_path.display()))?;
        if metadata.len() > EDIT_FILE_MAX_BYTES {
            return Err(anyhow!(
                "File too large for edit_file: {} bytes (max: {} bytes)",
                metadata.len(),
                EDIT_FILE_MAX_BYTES
            ));
        }

        let current_content = fs::read_to_string(&canonical_path)
            .await
            .with_context(|| format!("Cannot read file: {}", canonical_path.display()))?;

        // Track whether the original file had a trailing newline (Unix convention)
        let had_trailing_newline = current_content.ends_with('\n');

        let mut replacement_occurred = false;
        let mut new_content = current_content.to_owned();

        if current_content.contains(&effective_old_str) {
            new_content = current_content.replace(&effective_old_str, &effective_new_str);
            replacement_occurred = new_content != current_content;
        }

        if !replacement_occurred {
            let old_lines: Vec<&str> = effective_old_str.lines().collect();
            let content_lines: Vec<&str> = current_content.lines().collect();

            // Try multiple matching strategies with increasing leniency
            // Strategy 1: Exact line-by-line match with trim()
            'outer: for (i, window) in content_lines.windows(old_lines.len()).enumerate() {
                if utils::lines_match(window, &old_lines) {
                    let replacement_lines: Vec<&str> = effective_new_str.lines().collect();

                    // Build new content by replacing the matched window
                    let mut result_lines = Vec::with_capacity(
                        i + replacement_lines.len()
                            + content_lines.len().saturating_sub(i + old_lines.len()),
                    );
                    result_lines.extend_from_slice(&content_lines[..i]);
                    result_lines.extend_from_slice(&replacement_lines);
                    result_lines.extend_from_slice(&content_lines[i + old_lines.len()..]);

                    new_content = result_lines.join("\n");
                    replacement_occurred = true;
                    break 'outer;
                }
            }

            // Strategy 2: If still not found, try matching with normalized whitespace
            // (collapse multiple spaces, ignore leading/trailing whitespace)
            if !replacement_occurred {
                for (i, window) in content_lines.windows(old_lines.len()).enumerate() {
                    let window_normalized: Vec<String> = window
                        .iter()
                        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect();
                    let old_normalized: Vec<String> = old_lines
                        .iter()
                        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect();

                    if window_normalized == old_normalized {
                        let replacement_lines: Vec<&str> = effective_new_str.lines().collect();

                        // Build new content by replacing the matched window
                        let mut result_lines = Vec::with_capacity(
                            i + replacement_lines.len()
                                + content_lines.len().saturating_sub(i + old_lines.len()),
                        );
                        result_lines.extend_from_slice(&content_lines[..i]);
                        result_lines.extend_from_slice(&replacement_lines);
                        result_lines.extend_from_slice(&content_lines[i + old_lines.len()..]);

                        new_content = result_lines.join("\n");
                        replacement_occurred = true;
                        break;
                    }
                }
            }
        }

        if !replacement_occurred {
            let content_preview = if current_content.len() > 500 {
                format!(
                    "{}...{}",
                    &current_content[..250],
                    &current_content[current_content.len().saturating_sub(250)..]
                )
            } else {
                current_content.to_owned()
            };

            let numbering_note = if stripped_old || stripped_new {
                "\n\nNote: line-number prefixes were stripped before matching."
            } else {
                ""
            };

            return Err(anyhow!(
                "Could not find text to replace in file.\n\nExpected to replace:\n{}\n\nFile content preview:\n{}\n\nFix: The old_str must EXACTLY match the file content including all whitespace and newlines. Use read_file first to get the exact text, then copy it precisely into old_str. Do NOT add extra newlines or change indentation.{}",
                effective_old_str,
                content_preview,
                numbering_note
            ));
        }

        // Preserve trailing newline if original file had one (Unix convention)
        if had_trailing_newline && !new_content.ends_with('\n') {
            new_content.push('\n');
        }

        let write_args = json!({
            "path": input.path,
            "content": new_content,
            "mode": "overwrite"
        });

        self.file_ops_tool().write_file(write_args).await
    }

    pub async fn delete_file(&self, _args: Value) -> Result<Value> {
        self.execute_tool(tools::DELETE_FILE, _args).await
    }

    pub async fn grep_file(&self, args: Value) -> Result<Value> {
        self.execute_tool(tools::GREP_FILE, args).await
    }

    pub fn last_grep_file_result(&self) -> Option<GrepSearchResult> {
        self.grep_file_manager().last_result()
    }

    pub async fn list_files(&self, args: Value) -> Result<Value> {
        self.execute_tool(tools::LIST_FILES, args).await
    }
}
