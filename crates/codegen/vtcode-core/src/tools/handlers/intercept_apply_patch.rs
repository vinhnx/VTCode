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
use vtcode_commons::paths::ensure_path_within_workspace_resolved;

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
    let decoded = match decode_apply_patch_input(&args).map_err(|e| ApplyPatchError::ParseError(e.to_string()))? {
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
    if let Err(reason) = ensure_path_within_workspace_resolved(cwd, workspace_root).await {
        return Err(ApplyPatchError::ParseError(format!(
            "intercept_apply_patch rejected cwd '{}' (workspace='{}'): {}",
            cwd.display(),
            workspace_root.display(),
            reason
        )));
    }

    // Create the request
    let req = ApplyPatchRequest::new(decoded.text.clone(), cwd.to_path_buf()).with_timeout(timeout_ms.unwrap_or(30000));

    // Emit patch begin event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_begin(Default::default());
    }

    // Execute the patch through the same `Patch::parse` + `apply` pipeline that the
    // registry's `apply_patch` tool uses, but with the safety checks above applied.
    // The result is funneled through the turn diff tracker so the diff is recorded
    // the same way as a model-originated `apply_patch` call.
    let fallback = crate::tools::source_inspection::SourceInspection::default();
    let inspection = session.source_inspection().unwrap_or(&fallback);
    let parsed = Patch::parse(&req.patch).map_err(|error| ApplyPatchError::ParseError(error.to_string()))?;
    let _leases = if let Some(monitor) = session.edited_file_monitor() {
        crate::tools::source_inspection::SourceInspection::acquire_patch_leases(monitor, &parsed, &req.cwd)
            .await
            .map_err(|error| ApplyPatchError::PatchFailed(error.to_string()))?
    } else {
        Vec::new()
    };
    let inspection_plan = inspection
        .prepare_patch(&parsed, &req.cwd)
        .await
        .map_err(|error| ApplyPatchError::PatchFailed(error.to_string()))?;
    let result = execute_patch(&req).await;

    // Emit patch end event
    if let Some(tracker) = tracker {
        let mut t = tracker.write().await;
        t.on_patch_end(result.is_ok());
    }

    match result {
        Ok(output) => {
            inspection.commit_patch(
                inspection_plan,
                crate::tools::source_inspection::call_key(crate::config::constants::tools::APPLY_PATCH, &args),
            );
            Ok(Some(ToolOutput::simple(output)))
        }
        Err(e) => Ok(Some(ToolOutput::error(format!("{e} (call_id={call_id}, tool_name={tool_name})")))),
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
    use crate::tools::handlers::Constrained;
    use crate::tools::handlers::adapter::DefaultToolSession;
    use crate::tools::handlers::tool_handler::ShellEnvironmentPolicy;

    fn test_turn_context(cwd: PathBuf) -> TurnContext {
        TurnContext {
            cwd,
            turn_id: "test-turn".to_string(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::default(),
            approval_policy: Constrained::default(),
            linux_sandbox_launcher: None,
            sandbox_policy: Constrained::default(),
        }
    }

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
            maybe_parse_apply_patch_from_command(&["codex".to_string(), CODEX_APPLY_PATCH_ARG.to_string(),]),
            None
        );
        assert_eq!(
            maybe_parse_apply_patch_from_command(&["git".to_string(), "apply".to_string(), "test.patch".to_string(),]),
            None
        );
        assert_eq!(maybe_parse_apply_patch_from_command(&["patch".to_string(), "-p1".to_string(),]), None);
        assert_eq!(maybe_parse_apply_patch_from_command(&["echo".to_string(), "hello".to_string(),]), None);
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

    #[cfg(unix)]
    #[tokio::test]
    async fn intercept_rejects_symlink_escaped_cwd_before_outside_mutation() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::TempDir::new().expect("workspace should be created");
        let outside = tempfile::TempDir::new().expect("outside directory should be created");
        let escaped_cwd = workspace.path().join("escape");
        symlink(outside.path(), &escaped_cwd).expect("workspace symlink should be created");

        let session = DefaultToolSession::with_workspace(escaped_cwd.clone(), workspace.path().to_path_buf());
        let turn = test_turn_context(escaped_cwd.clone());
        let command = vec![
            "apply_patch".to_string(),
            "*** Begin Patch\n*** Add File: outside.txt\n+must not exist\n*** End Patch\n".to_string(),
        ];

        let error = intercept_apply_patch(&command, &escaped_cwd, None, &session, &turn, None, "call-1", "apply_patch")
            .await
            .expect_err("symlink-escaped cwd must be rejected");

        assert!(matches!(error, ApplyPatchError::ParseError(_)));
        assert!(!outside.path().join("outside.txt").exists(), "rejection must precede mutation");
    }
}
