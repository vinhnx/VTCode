use super::FileOpsTool;
use super::diff_preview::{build_diff_preview, diff_preview_error_skip, diff_preview_size_skip};
mod chunked;
mod fs_ops;
use crate::config::constants::diff;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::traits::FileTool;
use crate::tools::types::WriteInput;
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::borrow::Cow;

const MAX_WRITE_BYTES: usize = 64_000;

impl FileOpsTool {
    /// Write file with various modes and chunking support for large content
    pub async fn write_file(&self, args: Value) -> Result<Value> {
        let input: WriteInput = serde_json::from_value(args.clone())
            .context("Error: Invalid 'write_file' arguments. Expected JSON object with: path (required, string), content (required, string). Optional: mode (string, one of: overwrite, append, skip_if_exists). Example: {\"path\": \"README.md\", \"content\": \"Hello\", \"mode\": \"overwrite\"}")?;

        let file_path = self.normalize_and_validate_user_path(&input.path).await?;

        if self.should_exclude(&file_path).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore",
                input.path
            ));
        }

        let content_size = input.content.len();
        if content_size > MAX_WRITE_BYTES {
            return Err(anyhow!(
                "Content exceeds safe write limit ({} bytes). Use search_replace or apply_patch for large edits.",
                MAX_WRITE_BYTES
            ));
        }

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            ensure_dir_exists(parent).await?;
        }

        let file_exists = tokio::fs::try_exists(&file_path).await?;

        let mut existing_content: Option<String> = None;
        let mut diff_preview: Option<Value> = None;

        if file_exists {
            match read_file_with_context(&file_path, "existing file content").await {
                Ok(content) => existing_content = Some(content),
                Err(error) => {
                    diff_preview = Some(diff_preview_error_skip(
                        "failed_to_read_existing_content",
                        Some(&error.to_string()),
                    ));
                }
            }
        }

        let effective_mode = if input.overwrite
            && input.mode != "overwrite"
            && input.mode != "fail_if_exists"
        {
            return Err(anyhow!(
                "Conflicting parameters: overwrite=true but mode='{}'. Use mode='overwrite' or omit overwrite.",
                input.mode
            ));
        } else if input.overwrite {
            "overwrite"
        } else {
            input.mode.as_str()
        };

        match effective_mode {
            "overwrite" => {
                write_file_with_context(&file_path, &input.content, "file content").await?;
            }
            "append" => {
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?;
                file.write_all(input.content.as_bytes()).await?;
                file.flush().await?;
            }
            "skip_if_exists" => {
                if file_exists {
                    return Ok(ToolResponseBuilder::new("write_file")
                        .success()
                        .message("File already exists")
                        .field("skipped", json!(true))
                        .field("reason", json!("File already exists"))
                        .build_json());
                }
                write_file_with_context(&file_path, &input.content, "file content").await?;
            }
            "fail_if_exists" => {
                if file_exists {
                    return Err(anyhow!(
                        "File '{}' exists. Use mode='overwrite' (or overwrite=true) to replace, or choose append/skip_if_exists.",
                        input.path
                    ));
                }
                write_file_with_context(&file_path, &input.content, "file content").await?;
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{}'. Allowed: overwrite, append, skip_if_exists, fail_if_exists.",
                    effective_mode
                ));
            }
        }

        // Log write operation
        self.log_write_operation(&file_path, content_size, false)
            .await?;

        if diff_preview.is_none() {
            let existing_snapshot = existing_content.as_deref();
            let total_len = if input.mode.as_str() == "append" {
                existing_snapshot
                    .map(|content| content.len())
                    .unwrap_or_default()
                    + input.content.len()
            } else {
                input.content.len()
            };

            if total_len > diff::MAX_PREVIEW_BYTES
                || existing_snapshot
                    .map(|content| content.len() > diff::MAX_PREVIEW_BYTES)
                    .unwrap_or(false)
            {
                diff_preview = Some(diff_preview_size_skip());
            } else {
                let final_snapshot: Cow<'_, str> = if input.mode.as_str() == "append" {
                    if let Some(existing) = existing_snapshot {
                        Cow::Owned(format!("{existing}{}", input.content))
                    } else {
                        Cow::Borrowed(input.content.as_str())
                    }
                } else {
                    Cow::Borrowed(input.content.as_str())
                };

                diff_preview = Some(build_diff_preview(
                    &input.path,
                    existing_snapshot,
                    final_snapshot.as_ref(),
                ));
            }
        }

        let mut builder = ToolResponseBuilder::new("write_file")
            .success()
            .message(format!(
                "Successfully wrote file {}",
                self.workspace_relative_display(&file_path)
            ))
            .field("path", json!(self.workspace_relative_display(&file_path)))
            .field("mode", json!(effective_mode))
            .field("bytes_written", json!(input.content.len()))
            .field("file_existed", json!(file_exists));

        if let Some(preview) = diff_preview {
            builder = builder.field("diff_preview", preview);
        }

        Ok(builder.build_json())
    }
}
