use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tools::grep_file::GrepSearchResult;
use crate::tools::types::EditInput;

use super::ToolRegistry;
use super::utils;

const EDIT_FILE_MAX_CHARS: usize = 800;
const EDIT_FILE_MAX_LINES: usize = 40;

impl ToolRegistry {
    pub async fn read_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::READ_FILE, args).await
    }

    pub async fn write_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::WRITE_FILE, args).await
    }

    pub async fn create_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::CREATE_FILE, args).await
    }

    pub async fn edit_file(&mut self, args: Value) -> Result<Value> {
        let input: EditInput = serde_json::from_value(args).context("invalid edit_file args")?;

        let old_len = input.old_str.len();
        let new_len = input.new_str.len();
        let old_lines = input.old_str.lines().count();
        let new_lines = input.new_str.lines().count();

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

        let read_args = json!({
            "path": input.path,
            "max_lines": 1000000
        });

        let read_result = self.file_ops_tool().read_file(read_args).await?;
        let current_content = read_result["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Failed to read file content"))?;

        let mut replacement_occurred = false;
        let mut new_content = current_content.to_string();

        if current_content.contains(&input.old_str) {
            new_content = current_content.replace(&input.old_str, &input.new_str);
            replacement_occurred = new_content != current_content;
        }

        if !replacement_occurred {
            let old_lines: Vec<&str> = input.old_str.lines().collect();
            let content_lines: Vec<&str> = current_content.lines().collect();

            // Try multiple matching strategies with increasing leniency
            // Strategy 1: Exact line-by-line match with trim()
            'outer: for i in 0..=(content_lines.len().saturating_sub(old_lines.len())) {
                let window = &content_lines[i..i + old_lines.len()];
                if utils::lines_match(window, &old_lines) {
                    let before = content_lines[..i].join("\n");
                    let after = content_lines[i + old_lines.len()..].join("\n");
                    let replacement_lines: Vec<&str> = input.new_str.lines().collect();

                    new_content =
                        format!("{}\n{}\n{}", before, replacement_lines.join("\n"), after);
                    replacement_occurred = true;
                    break 'outer;
                }
            }

            // Strategy 2: If still not found, try matching with normalized whitespace
            // (collapse multiple spaces, ignore leading/trailing whitespace)
            if !replacement_occurred {
                for i in 0..=(content_lines.len().saturating_sub(old_lines.len())) {
                    let window = &content_lines[i..i + old_lines.len()];
                    let window_normalized: Vec<String> = window
                        .iter()
                        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect();
                    let old_normalized: Vec<String> = old_lines
                        .iter()
                        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
                        .collect();

                    if window_normalized == old_normalized {
                        let before = content_lines[..i].join("\n");
                        let after = content_lines[i + old_lines.len()..].join("\n");
                        let replacement_lines: Vec<&str> = input.new_str.lines().collect();

                        new_content =
                            format!("{}\n{}\n{}", before, replacement_lines.join("\n"), after);
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
                current_content.to_string()
            };

            return Err(anyhow!(
                "Could not find text to replace in file.\n\nExpected to replace:\n{}\n\nFile content preview:\n{}",
                input.old_str,
                content_preview
            ));
        }

        let write_args = json!({
            "path": input.path,
            "content": new_content,
            "mode": "overwrite"
        });

        self.file_ops_tool().write_file(write_args).await
    }

    pub async fn delete_file(&mut self, _args: Value) -> Result<Value> {
        self.execute_tool(tools::DELETE_FILE, _args).await
    }

    pub async fn grep_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::GREP_FILE, args).await
    }

    pub fn last_grep_file_result(&self) -> Option<GrepSearchResult> {
        self.grep_file_manager().last_result()
    }

    pub async fn list_files(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::LIST_FILES, args).await
    }
}
