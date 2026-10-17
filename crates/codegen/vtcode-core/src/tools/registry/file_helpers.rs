//! File operation helpers and the edit_file tool
//!
//! This module provides convenience methods for common file operations and implements
//! the `edit_file` tool, which is optimized for small, surgical edits (≤800 chars, ≤40 lines).
//! For larger or multi-file changes, use `apply_patch` instead.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::future::Future;
use std::path::PathBuf;
use tokio::fs;

use crate::utils::path::resolve_workspace_path;

use crate::config::constants::tools;
use crate::tools::edited_file_monitor::{FILE_CONFLICT_OVERRIDE_ARG, conflict_override_snapshot};
use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::grep_file::GrepSearchResult;
use crate::tools::traits::Tool;
use crate::tools::types::EditInput;

use super::ToolRegistry;
mod replacement;
use replacement::ReplacementPlan;

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

    let stripped = text
        .split_inclusive('\n')
        .map(|line| match line_prefix_len(line) {
            Some(prefix_len) => &line[prefix_len..],
            None => line,
        })
        .collect::<String>();

    (stripped, true)
}

impl ToolRegistry {
    /// Inline-delegating wrapper that returns the inner future directly to
    /// avoid an extra coroutine state machine (audit section 16).
    pub fn read_file(&self, args: Value) -> impl Future<Output = Result<Value>> + '_ {
        self.execute_tool(tools::READ_FILE, args)
    }

    /// Inline-delegating wrapper. See [`Self::read_file`].
    pub fn write_file(&self, args: Value) -> impl Future<Output = Result<Value>> + '_ {
        self.execute_tool(tools::WRITE_FILE, args)
    }

    /// Inline-delegating wrapper. See [`Self::read_file`].
    pub fn create_file(&self, args: Value) -> impl Future<Output = Result<Value>> + '_ {
        self.execute_tool(tools::CREATE_FILE, args)
    }

    pub async fn edit_file(&self, args: Value) -> Result<Value> {
        let input: EditInput = deserialize_tool_args(&args, "edit_file")?;
        let override_snapshot = conflict_override_snapshot(&args);

        let (effective_old_str, _) = strip_line_prefixes(&input.old_str);
        let (effective_new_str, _) = strip_line_prefixes(&input.new_str);

        if effective_old_str.is_empty() {
            return Err(anyhow!("edit_file requires non-empty original source; use apply_patch for insertions"));
        }
        let old_len = effective_old_str.len();
        let new_len = effective_new_str.len();
        let old_lines = effective_old_str.lines().count();
        let new_lines = effective_new_str.lines().count();

        if old_len > EDIT_FILE_MAX_CHARS
            || new_len > EDIT_FILE_MAX_CHARS
            || old_lines > EDIT_FILE_MAX_LINES
            || new_lines > EDIT_FILE_MAX_LINES
        {
            let guidance = crate::tools::error_helpers::USE_PATCH_FOR_LARGE_EDITS;
            return Err(anyhow!(
                "edit_file is limited to small literal replacements (≤ {EDIT_FILE_MAX_LINES} lines or ≤ {EDIT_FILE_MAX_CHARS} characters). {guidance}",
            ));
        }

        let requested_path = PathBuf::from(&input.path);
        let canonical_path = resolve_workspace_path(self.workspace_root(), &requested_path)
            .with_context(|| format!("Failed to resolve path: {}", requested_path.display()))?;
        let _mutation_lease = self.edited_file_monitor_ref().acquire_mutation(&canonical_path).await;

        let metadata = fs::metadata(&canonical_path)
            .await
            .with_context(|| format!("Cannot read file metadata: {}", canonical_path.display()))?;
        if metadata.len() > EDIT_FILE_MAX_BYTES {
            let actual = metadata.len();
            let max = EDIT_FILE_MAX_BYTES;
            let guidance = crate::tools::error_helpers::USE_PATCH_OR_WRITE_FOR_LARGE_FILES;
            return Err(anyhow!("File too large for edit_file: {actual} bytes (max: {max} bytes). {guidance}",));
        }

        let intended_content = self
            .edited_file_monitor_ref()
            .tracked_read_text(&canonical_path)
            .await
            .and_then(|content| {
                ReplacementPlan::build(&canonical_path, &content, &effective_old_str, &effective_new_str)
                    .ok()
                    .map(|plan| plan.apply(&content))
            });

        if let Some(conflict) = self
            .edited_file_monitor_ref()
            .detect_conflict(&canonical_path, intended_content, override_snapshot.clone())
            .await?
        {
            return Ok(conflict.to_tool_output(self.workspace_root()));
        }

        let current_content = fs::read_to_string(&canonical_path)
            .await
            .with_context(|| format!("Cannot read file: {}", canonical_path.display()))?;
        let plan = ReplacementPlan::build(&canonical_path, &current_content, &effective_old_str, &effective_new_str)?;
        let new_content = plan.apply(&current_content);
        let range = &plan.line_range;
        tracing::debug!(match_kind = ?plan.match_kind, "Planned literal edit");
        self.edited_file_monitor_ref().source_inspection().require(
            &canonical_path,
            &current_content,
            std::slice::from_ref(range),
        )?;
        let added = range
            .len()
            .saturating_add(new_content.lines().count())
            .saturating_sub(current_content.lines().count());
        let replacement = crate::tools::source_inspection::mutations::InspectedReplacement {
            start: range.start,
            removed: range.len(),
            added,
        };
        let after = new_content.clone();
        let mut write_args = json!({
            "path": input.path,
            "content": new_content,
            "mode": "overwrite"
        });
        if let Some(snapshot) = args.get(FILE_CONFLICT_OVERRIDE_ARG) {
            write_args[FILE_CONFLICT_OVERRIDE_ARG] = snapshot.clone();
        }

        let mut result = self.file_ops_tool().write_file_internal(write_args, false).await?;
        if result.get("conflict_detected").and_then(Value::as_bool) != Some(true) {
            self.edited_file_monitor_ref().source_inspection().record_edit(
                &canonical_path,
                &current_content,
                &after,
                &[replacement],
                crate::tools::source_inspection::call_key(tools::EDIT_FILE, &args),
            );
            if let Some(fields) = result.as_object_mut() {
                let modified_paths = if current_content == after {
                    Vec::new()
                } else {
                    vec![canonical_path]
                };
                fields.insert("modified_files".into(), json!(modified_paths));
            }
        }
        Ok(result)
    }

    /// Inline-delegating wrapper. See [`Self::read_file`].
    pub fn delete_file(&self, args: Value) -> impl Future<Output = Result<Value>> + '_ {
        self.execute_tool(tools::DELETE_FILE, args)
    }

    pub async fn grep_file(&self, args: Value) -> Result<Value> {
        let mut payload = args;
        if let Some(obj) = payload.as_object_mut() {
            let has_path = obj
                .get("path")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .is_some_and(|path| !path.is_empty());
            if !has_path {
                obj.insert("path".to_string(), json!("."));
            }
        }

        let input = serde_json::from_value(payload)?;
        self.grep_file_manager().perform_search(input).await.map(|result| json!(result))
    }

    pub fn last_grep_file_result(&self) -> Option<GrepSearchResult> {
        self.grep_file_manager().last_result()
    }

    pub async fn list_files(&self, args: Value) -> Result<Value> {
        // Dispatch directly to the underlying list implementation so a top-level
        // `list_files` call can safely run alongside `code_search` in the same batch.
        let tool = self.inventory.file_ops_tool().clone();
        tool.execute(args).await
    }
}
