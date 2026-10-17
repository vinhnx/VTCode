use super::FileOpsTool;
use super::diff_preview::{build_diff_preview, diff_preview_error_skip, diff_preview_size_skip};
mod chunked;
mod fs_ops;
use crate::config::constants::diff;
use crate::config::constants::tools;
use crate::tools::builder::ToolResponseBuilder;
use crate::tools::edited_file_monitor::conflict_override_snapshot;
use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::traits::FileTool;
use crate::tools::types::WriteInput;
use crate::utils::file_utils::{ensure_dir_exists, read_file_with_context};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::future::Future;
use std::io::ErrorKind;

const MAX_WRITE_BYTES: usize = 64_000;

async fn write_text_file(path: &std::path::Path, content: &str) -> Result<()> {
    // Single `spawn_blocking` write (tokio `fs` tuning: prefer `write` over
    // `open` + `write_all` + `flush`, which costs 3 blocking-pool hops).
    tokio::fs::write(path, content.as_bytes())
        .await
        .with_context(|| format!("Failed to write file content: {}", path.display()))
}

async fn create_text_file(path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
    // Preserve `create_new` semantics in one blocking-pool hop: `std::fs`
    // inside `spawn_blocking` (tokio `fs` tuning for multi-step sequences).
    let path = path.to_path_buf();
    let bytes = content.as_bytes().to_vec();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        let mut file = options.open(&path)?;
        file.write_all(&bytes)?;
        file.flush()
    })
    .await
    .map_err(|err| std::io::Error::other(format!("write task join failed: {err}")))?
}

impl FileOpsTool {
    /// Write file with various modes and chunking support for large content.
    /// Inline-delegating wrapper that returns the inner future directly to
    /// avoid an extra coroutine state machine (audit section 16).
    pub fn write_file(&self, args: Value) -> impl Future<Output = Result<Value>> + '_ {
        self.write_file_internal(args, true)
    }

    pub(crate) async fn write_file_internal(&self, args: Value, acquire_mutation: bool) -> Result<Value> {
        let input: WriteInput = deserialize_tool_args(&args, "write_file")?;
        let override_snapshot = conflict_override_snapshot(&args);

        let file_path = self.normalize_and_validate_user_path(&input.path).await?;

        if self.should_exclude(&file_path).await {
            return Err(anyhow!("Error: Path '{}' is excluded by .vtcodegitignore", input.path));
        }

        let content_size = input.content.len();
        if content_size > MAX_WRITE_BYTES {
            return Err(anyhow!(
                "Content exceeds safe write limit ({MAX_WRITE_BYTES} bytes, got {content_size} bytes). \
                 Use bounded apply_patch operations to write large files."
            ));
        }

        let _mutation_lease = if acquire_mutation {
            Some(self.edited_file_monitor.acquire_mutation(&file_path).await)
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
                    diff_preview =
                        Some(diff_preview_error_skip("failed_to_read_existing_content", Some(&error.to_string())));
                }
            }
        }

        let effective_mode = if input.overwrite && input.mode != "overwrite" && input.mode != "fail_if_exists" {
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
            return Ok(ToolResponseBuilder::new(tools::WRITE_FILE)
                .success()
                .message("File already exists")
                .field("skipped", json!(true))
                .field("reason", json!("File already exists"))
                .field("diff", json!([]))
                .build_json());
        }
        if effective_mode == "fail_if_exists" && file_exists {
            return Err(anyhow!(
                "File '{}' exists. Use mode='overwrite' (or overwrite=true) to replace, or choose append/skip_if_exists.",
                input.path
            ));
        }

        if acquire_mutation && let Some(content) = existing_content.as_deref() {
            if effective_mode == "append" {
                let line_count = content.lines().count();
                self.edited_file_monitor.source_inspection().require(
                    &file_path,
                    content,
                    std::slice::from_ref(&(line_count.saturating_sub(1)..line_count)),
                )?;
            } else {
                self.edited_file_monitor.source_inspection().require_full(&file_path, content)?;
            }
        } else if file_exists && existing_content.is_none() {
            return Err(anyhow!("Existing content must be readable before replacement"));
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
            "append" => intended_content.clone().unwrap_or_else(|| input.content.clone()),
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
                // One blocking-pool hop via `std::fs` (tokio `fs` tuning for
                // open + write + flush sequences instead of 3 async hops).
                let path = file_path.clone();
                let bytes = input.content.as_bytes().to_vec();
                tokio::task::spawn_blocking(move || {
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
                    file.write_all(&bytes)?;
                    file.flush()
                })
                .await
                .map_err(|err| anyhow!("append task join failed: {err}"))?
                .with_context(|| format!("Failed to append file content: {}", file_path.display()))?;
            }
            "skip_if_exists" => {
                if let Err(err) = create_text_file(&file_path, &input.content).await {
                    if err.kind() == ErrorKind::AlreadyExists {
                        return Ok(ToolResponseBuilder::new(tools::WRITE_FILE)
                            .success()
                            .message("File already exists")
                            .field("skipped", json!(true))
                            .field("reason", json!("File already exists"))
                            .field("diff", json!([]))
                            .build_json());
                    }
                    return Err(err).with_context(|| format!("Failed to create file content: {}", file_path.display()));
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
                    return Err(err).with_context(|| format!("Failed to create file content: {}", file_path.display()));
                }
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{effective_mode}'. Allowed: overwrite, append, skip_if_exists, fail_if_exists."
                ));
            }
        }

        if acquire_mutation {
            let support = crate::tools::source_inspection::call_key(tools::WRITE_FILE, &args);
            if let Some(before) = &existing_content {
                self.edited_file_monitor.source_inspection().record_edit(
                    &file_path,
                    before,
                    &final_written_content,
                    std::slice::from_ref(&if effective_mode == "append" {
                        let removed = usize::from(!before.is_empty() && !before.ends_with('\n'));
                        let start = before.lines().count().saturating_sub(removed);
                        crate::tools::source_inspection::mutations::InspectedReplacement {
                            start,
                            removed,
                            added: final_written_content.lines().count().saturating_sub(start),
                        }
                    } else {
                        crate::tools::source_inspection::mutations::InspectedReplacement {
                            start: 0,
                            removed: before.lines().count(),
                            added: final_written_content.lines().count(),
                        }
                    }),
                    support,
                );
            } else {
                self.edited_file_monitor.source_inspection().record_created(
                    &file_path,
                    &final_written_content,
                    support,
                );
            }
        }

        // Log write operation
        self.log_write_operation(&file_path, content_size, false).await?;
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
                existing_snapshot.map(|content| content.len()).unwrap_or_default() + input.content.len()
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

                diff_preview = Some(build_diff_preview(&input.path, existing_snapshot, final_snapshot.as_ref()));
            }
        }

        let mut builder = ToolResponseBuilder::new(tools::WRITE_FILE)
            .success()
            .message(format!("Successfully wrote file {}", self.workspace_relative_display(&file_path)))
            .field("path", json!(self.workspace_relative_display(&file_path)))
            .field("mode", json!(effective_mode))
            .field("bytes_written", json!(input.content.len()))
            .field("file_existed", json!(file_exists));

        if let Some(preview) = diff_preview {
            let mut canonical_preview = preview.clone();
            if let Some(fields) = canonical_preview.as_object_mut() {
                fields.insert("path".to_string(), json!(self.workspace_relative_display(&file_path)));
                fields.insert("operation".to_string(), json!(if file_exists { "updated" } else { "created" }));
            }
            builder = builder
                .field("created", json!(!file_exists))
                .field("diff_preview", preview)
                .field("diff", json!([canonical_preview]));
        }

        Ok(builder.build_json())
    }
}
