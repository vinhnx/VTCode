use super::FileOpsTool;
use crate::tools::error_helpers::{with_file_context, with_path_context};
use crate::tools::traits::FileTool;
use crate::tools::types::{CopyInput, CreateInput, DeleteInput, MoveInput};
use crate::utils::file_utils::ensure_dir_exists;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tracing::info;

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
            ensure_dir_exists(parent).await?;
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

        let exists = with_path_context(
            tokio::fs::try_exists(&target_path).await,
            "check if exists",
            &path,
        )?;

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

        let canonical = with_path_context(
            tokio::fs::canonicalize(&target_path).await,
            "resolve canonical path for",
            &path,
        )?;

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

        let metadata = with_path_context(
            tokio::fs::metadata(&canonical).await,
            "read metadata for",
            &path,
        )?;

        let deleted_kind = if metadata.is_dir() {
            if !recursive {
                return Err(anyhow!(
                    "Error: '{}' is a directory. Pass recursive=true to remove directories.",
                    path
                ));
            }

            with_path_context(
                tokio::fs::remove_dir_all(&canonical).await,
                "remove directory",
                &path,
            )?;
            "directory"
        } else {
            with_path_context(
                tokio::fs::remove_file(&canonical).await,
                "remove file",
                &path,
            )?;
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
            ensure_dir_exists(parent).await?;
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
            ensure_dir_exists(parent).await?;
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
                    ensure_dir_exists(&target).await?;
                } else {
                    with_file_context(
                        tokio::fs::copy(entry_path, &target).await,
                        "copy",
                        entry_path,
                    )?;
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
}
