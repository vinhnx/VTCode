//! Apply Patch Interception (from Codex)
//!
//! This module provides the ability to intercept shell commands that contain
//! apply_patch invocations, redirecting them to the proper patch application
//! flow with approval and sandbox handling.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::apply_patch_handler::parse_apply_patch_command;
use super::tool_handler::{ToolOutput, ToolSession, TurnContext};
use super::turn_diff_tracker::SharedTurnDiffTracker;
use crate::tools::apply_patch::decode_apply_patch_input;
use crate::tools::editing::Patch;
use serde_json::json;
use vtcode_commons::paths::ensure_path_within_workspace;

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
    let (is_apply_patch, patch_content) = parse_apply_patch_command(command);
    is_apply_patch.then_some(patch_content).flatten()
}

/// Intercept apply_patch from shell command (from Codex)
///
/// This function checks if a shell command is attempting to apply a patch
/// and redirects it through the proper patch application flow.
pub async fn intercept_apply_patch(
    command: &[String],
    cwd: &Path,
    timeout_ms: Option<u64>,
    session: &dyn ToolSession,
    _turn: &TurnContext,
    tracker: Option<&SharedTurnDiffTracker>,
    call_id: &str,
    tool_name: &str,
) -> Result<Option<ToolOutput>, ApplyPatchError> {
    let Some(patch) = maybe_parse_apply_patch_from_command(command) else {
        return Ok(None);
    };

    // Safety gate: decode the patch through the same path the registry uses, so the
    // post-decode size cap and the env-var override apply uniformly. This prevents
    // an oversized or base64-decompression patch from bypassing the preflight cap
    // when it is delivered via a shell command.
    let args = json!({ "input": &patch });
    let decoded = match decode_apply_patch_input(&args)
        .map_err(|e| ApplyPatchError::ParseError(e.to_string()))?
    {
        Some(decoded) => decoded,
        None => return Ok(None),
    };
    if decoded.text.is_empty() {
        return Ok(None);
    }

    // Ensure the working directory is contained within the session workspace.
    // The session workspace is owned by the harness kernel and exposed via the tool
    // session trait. This prevents intercepting a patch whose target is outside the
    // workspace sandbox.
    let workspace_root = session.workspace_root();
    if let Err(reason) = ensure_path_within_workspace(cwd, workspace_root) {
        return Err(ApplyPatchError::ParseError(format!(
            "intercept_apply_patch rejected cwd '{}' (workspace='{}'): {}",
            cwd.display(),
            workspace_root.display(),
            reason
        )));
    }

    // Create the request
    let req = ApplyPatchRequest::new(decoded.text.clone(), cwd.to_path_buf())
        .with_timeout(timeout_ms.unwrap_or(30000));

    // Emit patch begin event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_begin(Default::default());
    }

    // Execute the patch through the same `Patch::parse` + `apply` pipeline that the
    // registry's `apply_patch` tool uses, but with the safety checks above applied.
    // The result is funneled through the turn diff tracker so the diff is recorded
    // the same way as a model-originated `apply_patch` call.
    let result = execute_patch(&req).await;

    // Emit patch end event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_end(result.is_ok());
    }

    match result {
        Ok(output) => Ok(Some(ToolOutput::simple(output))),
        Err(e) => Ok(Some(ToolOutput::error(format!(
            "{e} (call_id={call_id}, tool_name={tool_name})"
        )))),
    }
}

async fn execute_patch(req: &ApplyPatchRequest) -> Result<String, ApplyPatchError> {
    let patch = Patch::parse(&req.patch).map_err(|e| ApplyPatchError::ParseError(e.to_string()))?;
    if patch.is_empty() {
        return Ok("Patch is empty, no changes applied".to_string());
    }

    let results = patch
        .apply(&req.cwd)
        .await
        .map_err(|e| ApplyPatchError::PatchFailed(e.to_string()))?;
    Ok(results.join("\n"))
}

/// Errors from apply_patch interception
#[derive(Debug, thiserror::Error)]
pub enum ApplyPatchError {
    #[error("Failed to parse patch: {0}")]
    ParseError(String),

    #[error("Patch application failed: {0}")]
    PatchFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maybe_parse_apply_patch_detects_direct_invocation() {
        assert_eq!(
            maybe_parse_apply_patch_from_command(&[
                "apply_patch".to_string(),
                "*** Begin Patch\n*** End Patch".to_string(),
            ]),
            Some("*** Begin Patch\n*** End Patch".to_string())
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&[
                "applypatch".to_string(),
                "*** Begin Patch\n*** End Patch".to_string(),
            ]),
            Some("*** Begin Patch\n*** End Patch".to_string())
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&[
                "codex".to_string(),
                CODEX_APPLY_PATCH_ARG.to_string(),
            ]),
            None
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&[
                "git".to_string(),
                "apply".to_string(),
                "test.patch".to_string(),
            ]),
            None
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&["patch".to_string(), "-p1".to_string(),]),
            None
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&["echo".to_string(), "hello".to_string(),]),
            None
        );
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
