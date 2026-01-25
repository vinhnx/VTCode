use super::diff_preview::{build_diff_preview, diff_preview_error_skip, diff_preview_size_skip};
use super::FileOpsTool;
use crate::config::constants::diff;
use crate::tools::traits::FileTool;
use crate::tools::types::{CopyInput, CreateInput, DeleteInput, MoveInput, WriteInput};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::path::Path;
use tracing::info;

const MAX_WRITE_BYTES: usize = 64_000;

impl FileOpsTool {
    /// Execute basic directory listing
    pub async fn create_file(&self, args: Value) -> Result<Value> {
        let input: CreateInput = serde_json::from_value(args.clone()).map_err(|error| {
            anyhow!(
                "Error: Invalid 'create_file' arguments. Provide an object like {{\"path\": \"src/lib.rs\", \"content\": \"fn main() {{}}\\n\"}} with optional \"encoding\". Deserialization failed: {}",
                error
            )
        })?;

        let CreateInput {
            path,
            content,
            encoding,
        } = input;

        let file_path = self.normalize_and_validate_user_path(&path).await?;

        if self.should_exclude(&file_path).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore",
                path
            ));
        }

        if tokio::fs::try_exists(&file_path).await? {
            return Err(anyhow!(
                "Error: File '{}' already exists. Use write_file with mode='overwrite' to replace it.",
                path
            ));
        }

        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut payload = json!({
            "path": path,
            "content": content,
            "mode": "overwrite"
        });

        if let Some(encoding) = encoding {
            payload["encoding"] = Value::String(encoding);
        }

        let mut result = self.write_file(payload).await?;

        if let Some(map) = result.as_object_mut() {
            map.insert("created".to_string(), Value::Bool(true));
        }

        Ok(result)
    }

    /// Delete a file or directory (with recursive flag).
    pub async fn delete_file(&self, args: Value) -> Result<Value> {
        let input: DeleteInput = serde_json::from_value(args).context(
            "Error: Invalid 'delete_file' arguments. Expected JSON object with: path (required, string). Optional: recursive (bool), force (bool). Example: {\"path\": \"src/lib.rs\"}",
        )?;

        let DeleteInput {
            path,
            recursive,
            force,
        } = input;

        let target_path = self.workspace_root.join(&path);

        let exists = tokio::fs::try_exists(&target_path)
            .await
            .with_context(|| format!("Failed to check if '{}' exists", path))?;

        if !exists {
            if force {
                return Ok(json!({
                    "success": true,
                    "deleted": false,
                    "skipped": true,
                    "reason": "not_found",
                    "path": path,
                }));
            }

            return Err(anyhow!(
                "Error: Path '{}' does not exist. Provide force=true to ignore missing files.",
                path
            ));
        }

        let canonical = tokio::fs::canonicalize(&target_path)
            .await
            .with_context(|| format!("Failed to resolve canonical path for '{}'", path))?;

        if !canonical.starts_with(self.canonical_workspace_root()) {
            return Err(anyhow!(
                "Error: Path '{}' resolves outside the workspace and cannot be deleted.",
                path
            ));
        }

        if self.should_exclude(&canonical).await {
            return Err(anyhow!(
                "Error: Path '{}' is excluded by .vtcodegitignore and cannot be deleted.",
                path
            ));
        }

        let metadata = tokio::fs::metadata(&canonical)
            .await
            .with_context(|| format!("Failed to read metadata for '{}'", path))?;

        let deleted_kind = if metadata.is_dir() {
            if !recursive {
                return Err(anyhow!(
                    "Error: '{}' is a directory. Pass recursive=true to remove directories.",
                    path
                ));
            }

            tokio::fs::remove_dir_all(&canonical)
                .await
                .with_context(|| format!("Failed to remove directory '{}'", path))?;
            "directory"
        } else {
            tokio::fs::remove_file(&canonical)
                .await
                .with_context(|| format!("Failed to remove file '{}'", path))?;
            "file"
        };

        Ok(json!({
            "success": true,
            "deleted": true,
            "path": self.workspace_relative_display(&canonical),
            "kind": deleted_kind,
        }))
    }

    /// Move a file or directory.
    pub async fn move_file(&self, args: Value) -> Result<Value> {
        let input: MoveInput = serde_json::from_value(args).context(
            "Error: Invalid 'move_file' arguments. Expected JSON object with: path (required, string), destination (required, string).",
        )?;

        let MoveInput {
            path,
            destination,
            force,
        } = input;

        let from_path = self.workspace_root.join(&path);
        let to_path = self.workspace_root.join(&destination);

        if !tokio::fs::try_exists(&from_path).await? {
            return Err(anyhow!("Source path '{}' does not exist", path));
        }

        if tokio::fs::try_exists(&to_path).await? && !force {
            return Err(anyhow!(
                "Destination path '{}' already exists. Use force=true to overwrite.",
                destination
            ));
        }

        // Ensure destination parent directory exists
        if let Some(parent) = to_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::rename(&from_path, &to_path)
            .await
            .with_context(|| format!("Failed to move '{}' to '{}'", path, destination))?;

        info!(from = %path, to = %destination, "Moved successfully");

        Ok(json!({
            "success": true,
            "from": path,
            "to": destination,
        }))
    }

    /// Copy a file or directory.
    pub async fn copy_file(&self, args: Value) -> Result<Value> {
        let input: CopyInput = serde_json::from_value(args).context(
            "Error: Invalid 'copy_file' arguments. Expected JSON object with: path (required, string), destination (required, string). Optional: recursive (bool).",
        )?;

        let CopyInput {
            path,
            destination,
            recursive,
        } = input;

        let from_path = self.workspace_root.join(&path);
        let to_path = self.workspace_root.join(&destination);

        if !tokio::fs::try_exists(&from_path).await? {
            return Err(anyhow!("Source path '{}' does not exist", path));
        }

        // Ensure destination parent directory exists
        if let Some(parent) = to_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let metadata = tokio::fs::metadata(&from_path).await?;
        if metadata.is_dir() {
            if !recursive {
                return Err(anyhow!(
                    "Path '{}' is a directory. Use recursive=true to copy directories.",
                    path
                ));
            }
            // Simple recursive copy using walkdir
            use walkdir::WalkDir;
            for entry in WalkDir::new(&from_path).into_iter().filter_map(|e| e.ok()) {
                let entry_path = entry.path();
                let relative = entry_path.strip_prefix(&from_path).unwrap_or(entry_path);
                let target = to_path.join(relative);

                if entry.file_type().is_dir() {
                    tokio::fs::create_dir_all(&target).await?;
                } else {
                    tokio::fs::copy(entry_path, &target)
                        .await
                        .with_context(|| format!("Failed to copy '{}'", entry_path.display()))?;
                }
            }
        } else {
            tokio::fs::copy(&from_path, &to_path)
                .await
                .with_context(|| format!("Failed to copy '{}' to '{}'", path, destination))?;
        }

        info!(from = %path, to = %destination, "Copied successfully");

        Ok(json!({
            "success": true,
            "from": path,
            "to": destination,
        }))
    }

    /// Write file with various modes and chunking support for large content
    pub async fn write_file(&self, args: Value) -> Result<Value> {
        let input: WriteInput = serde_json::from_value(args)
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
            tokio::fs::create_dir_all(parent).await?;
        }

        let file_exists = tokio::fs::try_exists(&file_path).await?;

        let mut existing_content: Option<String> = None;
        let mut diff_preview: Option<Value> = None;

        if file_exists {
            match tokio::fs::read_to_string(&file_path).await {
                Ok(content) => existing_content = Some(content),
                Err(error) => {
                    diff_preview = Some(diff_preview_error_skip(
                        "failed_to_read_existing_content",
                        Some(&format!("{:?}", error.kind())),
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
                tokio::fs::write(&file_path, &input.content).await?;
            }
            "append" => {
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?;
                file.write_all(input.content.as_bytes()).await?;
            }
            "skip_if_exists" => {
                if file_exists {
                    return Ok(json!({
                        "success": true,
                        "skipped": true,
                        "reason": "File already exists"
                    }));
                }
                tokio::fs::write(&file_path, &input.content).await?;
            }
            "fail_if_exists" => {
                if file_exists {
                    return Err(anyhow!(
                        "File '{}' exists. Use mode='overwrite' (or overwrite=true) to replace, or choose append/skip_if_exists.",
                        input.path
                    ));
                }
                tokio::fs::write(&file_path, &input.content).await?;
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

        let mut response = json!({
            "success": true,
            "path": self.workspace_relative_display(&file_path),
            "mode": effective_mode,
            "bytes_written": input.content.len(),
            "file_existed": file_exists,
        });

        if let Some(preview) = diff_preview
            && let Some(object) = response.as_object_mut()
        {
            object.insert("diff_preview".to_string(), preview);
        }

        Ok(response)
    }

    /// Write large file in chunks for atomicity and memory efficiency
    #[allow(dead_code)]
    async fn write_file_chunked(&self, file_path: &Path, input: &WriteInput) -> Result<Value> {
        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content_bytes = input.content.as_bytes();
        let chunk_size = crate::config::constants::chunking::WRITE_CHUNK_SIZE;
        let total_size = content_bytes.len();

        match input.mode.as_str() {
            "overwrite" => {
                // Write in chunks for large files
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "append" => {
                // Append in chunks
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file_path)
                    .await?;

                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            "skip_if_exists" => {
                if file_path.exists() {
                    return Ok(json!({
                        "success": true,
                        "skipped": true,
                        "reason": "File already exists"
                    }));
                }
                // Write in chunks for new file
                use tokio::io::AsyncWriteExt;
                let mut file = tokio::fs::File::create(file_path).await?;
                for chunk in content_bytes.chunks(chunk_size) {
                    file.write_all(chunk).await?;
                }
                file.flush().await?;
            }
            _ => {
                return Err(anyhow!(
                    "Error: Unsupported write mode '{}'. Allowed: overwrite, append, skip_if_exists.",
                    input.mode
                ));
            }
        }

        // Log chunked write operation
        self.log_write_operation(file_path, total_size, true)
            .await?;

        Ok(json!({
            "success": true,
            "path": self.workspace_relative_display(file_path),
            "mode": input.mode,
            "bytes_written": total_size,
            "chunked": true,
            "chunk_size": chunk_size,
            "chunks_written": total_size.div_ceil(chunk_size),
            "diff_preview": diff_preview_size_skip()
        }))
    }

    /// Log write operations for debugging
    async fn log_write_operation(
        &self,
        file_path: &Path,
        bytes_written: usize,
        chunked: bool,
    ) -> Result<()> {
        let log_entry = json!({
            "operation": if chunked { "write_file_chunked" } else { "write_file" },
            "file_path": file_path.to_string_lossy(),
            "bytes_written": bytes_written,
            "chunked": chunked,
            "chunk_size": if chunked { Some(crate::config::constants::chunking::WRITE_CHUNK_SIZE) } else { None },
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        info!(
            "File write operation: {}",
            serde_json::to_string(&log_entry)?
        );
        Ok(())
    }
}
