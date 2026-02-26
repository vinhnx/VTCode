//! Apply Patch Interception (from Codex)
//!
//! This module provides the ability to intercept shell commands that contain
//! apply_patch invocations, redirecting them to the proper patch application
//! flow with approval and sandbox handling.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::tool_handler::{ToolOutput, ToolSession, TurnContext};
use super::turn_diff_tracker::SharedTurnDiffTracker;
use crate::utils::file_utils::write_file_with_context;

/// The argument used to indicate apply_patch mode (from Codex)
pub const CODEX_APPLY_PATCH_ARG: &str = "--codex-run-as-apply-patch";

/// Apply patch request (from Codex)
#[derive(Clone, Debug)]
pub struct ApplyPatchRequest {
    /// The patch content
    pub patch: String,
    /// Working directory
    pub cwd: PathBuf,
    /// Timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Whether user explicitly approved
    pub user_explicitly_approved: bool,
    /// Path to codex executable (for self-invocation)
    pub codex_exe: Option<PathBuf>,
}

impl ApplyPatchRequest {
    pub fn new(patch: String, cwd: PathBuf) -> Self {
        Self {
            patch,
            cwd,
            timeout_ms: Some(30000),
            user_explicitly_approved: false,
            codex_exe: None,
        }
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn with_approval(mut self, approved: bool) -> Self {
        self.user_explicitly_approved = approved;
        self
    }

    pub fn with_codex_exe(mut self, exe: PathBuf) -> Self {
        self.codex_exe = Some(exe);
        self
    }
}

/// Check if a command contains an apply_patch invocation (from Codex)
///
/// Returns the patch content if found
pub fn maybe_parse_apply_patch_from_command(command: &[String]) -> Option<String> {
    // Check for self-invocation pattern: codex --codex-run-as-apply-patch
    if command.iter().any(|arg| arg == CODEX_APPLY_PATCH_ARG) {
        // The patch content is typically piped via stdin, not in args
        return None;
    }

    // Check for git apply pattern
    if command.len() >= 2 && command[0] == "git" && command[1] == "apply" {
        // Look for patch file or stdin indicator
        for arg in &command[2..] {
            if !arg.starts_with('-') && !arg.is_empty() {
                // This might be a patch file path
                return None; // Would need to read the file
            }
        }
    }

    // Check for patch command
    if !command.is_empty() && command[0] == "patch" {
        return None; // Would need to parse patch flags
    }

    None
}

/// Intercept apply_patch from shell command (from Codex)
///
/// This function checks if a shell command is attempting to apply a patch
/// and redirects it through the proper patch application flow.
#[allow(clippy::too_many_arguments)]
pub async fn intercept_apply_patch(
    command: &[String],
    cwd: &Path,
    timeout_ms: Option<u64>,
    _session: &dyn ToolSession,
    _turn: &TurnContext,
    tracker: Option<&SharedTurnDiffTracker>,
    _call_id: &str,
    _tool_name: &str,
) -> Result<Option<ToolOutput>, ApplyPatchError> {
    // Check if this is an apply_patch command
    if !is_apply_patch_command(command) {
        return Ok(None);
    }

    // Extract patch content from command
    let patch = extract_patch_from_command(command)?;

    // Create the request
    let req = ApplyPatchRequest::new(patch.clone(), cwd.to_path_buf())
        .with_timeout(timeout_ms.unwrap_or(30000));

    // Emit patch begin event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_begin(Default::default());
    }

    // For now, execute patch directly (would use ApplyPatchRuntime in production)
    let result = execute_patch(&req).await;

    // Emit patch end event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_end(result.is_ok());
    }

    match result {
        Ok(output) => Ok(Some(ToolOutput::simple(output))),
        Err(e) => Ok(Some(ToolOutput::error(e.to_string()))),
    }
}

/// Check if a command is an apply_patch command
fn is_apply_patch_command(command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }

    // Check for codex self-invocation
    if command.iter().any(|arg| arg == CODEX_APPLY_PATCH_ARG) {
        return true;
    }

    // Check for git apply
    if command.len() >= 2 && command[0] == "git" && command[1] == "apply" {
        return true;
    }

    // Check for patch command
    if command[0] == "patch" {
        return true;
    }

    false
}

/// Extract patch content from command arguments
fn extract_patch_from_command(command: &[String]) -> Result<String, ApplyPatchError> {
    // For git apply with inline patch (heredoc style)
    for arg in command.iter() {
        if arg == "-" {
            // Patch is expected from stdin
            return Err(ApplyPatchError::StdinPatchNotSupported);
        }
        if !arg.starts_with('-') && arg.ends_with(".patch") {
            // This is a patch file path - would need to read it
            return Err(ApplyPatchError::PatchFileNotSupported(arg.clone()));
        }
    }

    // Check if patch is embedded in a heredoc-style argument
    for arg in command {
        if arg.contains("<<<") || arg.contains("EOF") {
            // Heredoc pattern - extract content
            if let Some(content) = extract_heredoc_content(arg) {
                return Ok(content);
            }
        }
    }

    Err(ApplyPatchError::NoPatchContent)
}

/// Extract content from a heredoc-style string
fn extract_heredoc_content(input: &str) -> Option<String> {
    // Simple heredoc extraction
    if let Some(start) = input.find("<<<")
        && let Some(end_marker_start) = input[start + 3..].find('\n')
    {
        let content_start = start + 3 + end_marker_start + 1;
        if let Some(eof_pos) = input[content_start..].find("EOF") {
            return Some(input[content_start..content_start + eof_pos].to_string());
        }
    }
    None
}

/// Execute the patch (simplified implementation)
async fn execute_patch(req: &ApplyPatchRequest) -> Result<String, ApplyPatchError> {
    use tokio::process::Command;

    // Write patch to temp file
    let temp_file = std::env::temp_dir().join(format!("vtcode_patch_{}.patch", std::process::id()));
    write_file_with_context(&temp_file, &req.patch, "temporary patch file")
        .await
        .map_err(|e| ApplyPatchError::IoError(e.to_string()))?;

    // Apply using git apply
    let output = Command::new("git")
        .args(["apply", "--verbose"])
        .arg(&temp_file)
        .current_dir(&req.cwd)
        .output()
        .await
        .map_err(|e| ApplyPatchError::ExecutionFailed(e.to_string()))?;

    // Clean up temp file
    let _ = tokio::fs::remove_file(&temp_file).await;

    if output.status.success() {
        Ok(format!(
            "Patch applied successfully\n{}",
            String::from_utf8_lossy(&output.stdout)
        ))
    } else {
        Err(ApplyPatchError::PatchFailed(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ))
    }
}

/// Errors from apply_patch interception
#[derive(Debug, thiserror::Error)]
pub enum ApplyPatchError {
    #[error("Patch from stdin not supported in interception")]
    StdinPatchNotSupported,

    #[error("Patch file not supported in interception: {0}")]
    PatchFileNotSupported(String),

    #[error("No patch content found in command")]
    NoPatchContent,

    #[error("Failed to parse patch: {0}")]
    ParseError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Patch execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Patch application failed: {0}")]
    PatchFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_apply_patch_command() {
        assert!(is_apply_patch_command(&[
            "codex".to_string(),
            CODEX_APPLY_PATCH_ARG.to_string(),
        ]));
        assert!(is_apply_patch_command(&[
            "git".to_string(),
            "apply".to_string(),
            "test.patch".to_string(),
        ]));
        assert!(is_apply_patch_command(&[
            "patch".to_string(),
            "-p1".to_string(),
        ]));
        assert!(!is_apply_patch_command(&[
            "echo".to_string(),
            "hello".to_string(),
        ]));
    }

    #[test]
    fn test_extract_patch_stdin_error() {
        let command = vec!["git".to_string(), "apply".to_string(), "-".to_string()];
        let result = extract_patch_from_command(&command);
        assert!(matches!(
            result,
            Err(ApplyPatchError::StdinPatchNotSupported)
        ));
    }

    #[test]
    fn test_extract_patch_file_error() {
        let command = vec![
            "git".to_string(),
            "apply".to_string(),
            "changes.patch".to_string(),
        ];
        let result = extract_patch_from_command(&command);
        assert!(matches!(
            result,
            Err(ApplyPatchError::PatchFileNotSupported(_))
        ));
    }

    #[test]
    fn test_apply_patch_request_builder() {
        let req = ApplyPatchRequest::new("patch content".to_string(), PathBuf::from("/tmp"))
            .with_timeout(5000)
            .with_approval(true)
            .with_codex_exe(PathBuf::from("/usr/bin/codex"));

        assert_eq!(req.patch, "patch content");
        assert_eq!(req.timeout_ms, Some(5000));
        assert!(req.user_explicitly_approved);
        assert_eq!(req.codex_exe, Some(PathBuf::from("/usr/bin/codex")));
    }
}
