use super::FileOpsTool;
use super::diff_preview::{build_diff_preview, diff_preview_error_skip, diff_preview_size_skip};
mod chunked;
mod fs_ops;
use crate::config::constants::diff;
use crate::tools::edited_file_monitor::conflict_override_snapshot;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::traits::FileTool;
use crate::tools::types::WriteInput;
use crate::utils::file_utils::{ensure_dir_exists, read_file_with_context};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::io::ErrorKind;
use tokio::io::AsyncWriteExt;

const MAX_WRITE_BYTES: usize = 64_000;

async fn write_text_file(path: &std::path::Path, content: &str) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
        .with_context(|| format!("Failed to open file for writing: {}", path.display()))?;
    file.write_all(content.as_bytes())
        .await
        .with_context(|| format!("Failed to write file content: {}", path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("Failed to flush file content: {}", path.display()))
}

async fn create_text_file(path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    file.flush().await
}

impl FileOpsTool {
    /// Write file with various modes and chunking support for large content
    pub async fn write_file(&self, args: Value) -> Result<Value> {
        self.write_file_internal(args, true).await
    }

    pub(crate) async fn write_file_internal(
        &self,
        args: Value,
        acquire_mutation: bool,
    ) -> Result<Value> {
        let input: WriteInput = serde_json::from_value(args.clone())
            .context("Error: Invalid 'write_file' arguments. Expected JSON object with: path (required, string), content (required, string). Optional: mode (string, one of: overwrite, append, skip_if_exists). Example: {\"path\": \"README.md\", \"content\": \"Hello\", \"mode\": \"overwrite\"}")?;
        let override_snapshot = conflict_override_snapshot(&args);

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

        let _mutation_lease = if acquire_mutation {
            Some(
                self.edited_file_monitor
                    .acquire_mutation(&file_path)
                    .await,
            )
        } else {
            None
        };

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

        if effective_mode == "skip_if_exists" && file_exists {
            return Ok(ToolResponseBuilder::new("write_file")
                .success()
                .message("File already exists")
                .field("skipped", json!(true))
                .field("reason", json!("File already exists"))
                .build_json());
        }
        if effective_mode == "fail_if_exists" && file_exists {
            return Err(anyhow!(
                "File '{}' exists. Use mode='overwrite' (or overwrite=true) to replace, or choose append/skip_if_exists.",
                input.path
            ));
        }

        let intended_content = match effective_mode {
            "overwrite" => Some(input.content.clone()),
            "append" => existing_content
                .as_ref()
                .map(|content| format!("{content}{}", input.content))
                .or_else(|| Some(input.content.clone())),
            "skip_if_exists" | "fail_if_exists" => Some(input.content.clone()),
            _ => None,
        };

        if let Some(conflict) = self
            .edited_file_monitor
            .detect_conflict(&file_path, intended_content.clone(), override_snapshot.clone())
            .await?
        {
            return Ok(conflict.to_tool_output(&self.workspace_root));
        }

        let final_written_content = match effective_mode {
            "append" => intended_content
                .clone()
                .unwrap_or_else(|| input.content.clone()),
            _ => input.content.clone(),
        };

        if matches!(effective_mode, "overwrite" | "append")
            && let Some(conflict) = self
                .edited_file_monitor
                .detect_conflict(&file_path, intended_content.clone(), override_snapshot)
                .await?
        {
            return Ok(conflict.to_tool_output(&self.workspace_root));
        }

        match effective_mode {
            "overwrite" => {
                write_text_file(&file_path, &input.content).await?;
            }
            "append" => {
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?;
                file.write_all(input.content.as_bytes()).await?;
                file.flush().await?;
            }
            "skip_if_exists" => {
                if let Err(err) = create_text_file(&file_path, &input.content).await {
                    if err.kind() == ErrorKind::AlreadyExists {
                        return Ok(ToolResponseBuilder::new("write_file")
                            .success()
                            .message("File already exists")
                            .field("skipped", json!(true))
                            .field("reason", json!("File already exists"))
                            .build_json());
                    }
                    return Err(err).with_context(|| {
                        format!("Failed to create file content: {}", file_path.display())
                    });
                }
            }
            "fail_if_exists" => {
                if let Err(err) = create_text_file(&file_path, &input.content).await {
                    if err.kind() == ErrorKind::AlreadyExists {
                        return Err(anyhow!(
                            "File '{}' exists. Use mode='overwrite' (or overwrite=true) to replace, or choose append/skip_if_exists.",
                            input.path
                        ));
                    }
                    return Err(err).with_context(|| {
                        format!("Failed to create file content: {}", file_path.display())
                    });
                }
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
        if let Err(err) = self
            .edited_file_monitor
            .record_agent_write_text(&file_path, &final_written_content)
        {
            tracing::warn!(
                path = %file_path.display(),
                error = %err,
                "Failed to refresh edited-file snapshot after write"
            );
        }

        if diff_preview.is_none() {
            let existing_snapshot = existing_content.as_deref();
            let total_len = if effective_mode == "append" {
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
                let final_snapshot: Cow<'_, str> = if effective_mode == "append" {
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
