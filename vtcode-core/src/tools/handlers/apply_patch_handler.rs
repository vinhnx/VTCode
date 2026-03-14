//! Apply patch handler (from Codex)
//!
//! Implements the apply_patch tool using the Codex-style handler pattern.
//! Supports both freeform and JSON function call formats.

use hashbrown::HashMap;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::events::{ToolEmitter, ToolEventCtx};
use super::sandboxing::{
    Approvable, ApprovalCtx, AskForApproval, BoxFuture, ExecApprovalRequirement,
    ExecToolCallOutput, ReviewDecision, SandboxAttempt, Sandboxable, SandboxablePreference,
    ToolCtx, ToolError, ToolRuntime,
};
use super::tool_handler::{
    ApprovalPolicy, FileChange, FreeformTool, FreeformToolFormat, JsonSchema, ResponsesApiTool,
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload, ToolSpec,
};
use super::tool_orchestrator::ToolOrchestrator;
use crate::tools::editing::{Patch, PatchOperation};

/// Context for intercepting apply_patch commands
pub struct InterceptApplyPatchContext<'a> {
    pub cwd: &'a Path,
    pub timeout_ms: Option<u64>,
    pub session: Arc<dyn super::tool_handler::ToolSession>,
    pub turn: Arc<super::tool_handler::TurnContext>,
    pub tracker: Option<&'a Arc<tokio::sync::Mutex<super::tool_handler::DiffTracker>>>,
    pub call_id: &'a str,
    pub tool_name: &'a str,
}

/// Apply patch handler
pub struct ApplyPatchHandler;

/// Arguments for apply_patch function call
#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPatchToolArgs {
    pub input: Option<String>,
    pub patch: Option<String>,
}

/// Request for apply_patch runtime
#[derive(Clone, Debug)]
pub struct ApplyPatchRequest {
    pub patch: String,
    pub cwd: PathBuf,
    pub timeout_ms: Option<u64>,
    pub user_explicitly_approved: bool,
}

/// Approval key for caching
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct ApplyPatchApprovalKey {
    patch: String,
    cwd: PathBuf,
}

/// Apply patch runtime for orchestrated execution
#[derive(Default)]
pub struct ApplyPatchRuntime;

impl ApplyPatchRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Sandboxable for ApplyPatchRuntime {
    fn sandbox_preference(&self) -> SandboxablePreference {
        // Patches modify files, so we prefer auto sandbox
        SandboxablePreference::Auto
    }

    fn escalate_on_failure(&self) -> bool {
        // Allow escalation if sandbox fails
        true
    }
}

impl Approvable<ApplyPatchRequest> for ApplyPatchRuntime {
    type ApprovalKey = ApplyPatchApprovalKey;

    fn approval_key(&self, req: &ApplyPatchRequest) -> Self::ApprovalKey {
        ApplyPatchApprovalKey {
            patch: req.patch.clone(),
            cwd: req.cwd.clone(),
        }
    }

    fn exec_approval_requirement(
        &self,
        _req: &ApplyPatchRequest,
    ) -> Option<ExecApprovalRequirement> {
        // Preserve existing behavior from the legacy orchestrator path:
        // apply_patch is executed without additional approval prompts here.
        Some(ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        })
    }

    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::Never => false,
            AskForApproval::Reject(reject_config) => !reject_config.rejects_sandbox_approval(),
            AskForApproval::OnFailure => true,
            AskForApproval::OnRequest => true,
            AskForApproval::UnlessTrusted => true,
        }
    }

    fn start_approval_async<'a>(
        &'a mut self,
        _req: &'a ApplyPatchRequest,
        _ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision> {
        Box::pin(async { ReviewDecision::Approved })
    }
}

#[async_trait]
impl ToolRuntime<ApplyPatchRequest, ExecToolCallOutput> for ApplyPatchRuntime {
    async fn run(
        &mut self,
        req: &ApplyPatchRequest,
        _attempt: &SandboxAttempt<'_>,
        _ctx: &ToolCtx,
    ) -> Result<ExecToolCallOutput, ToolError> {
        // Parse and apply the patch
        let patch = Patch::parse(&req.patch)
            .map_err(|e| ToolError::Rejected(format!("Failed to parse patch: {}", e)))?;

        if patch.is_empty() {
            return Ok(ExecToolCallOutput {
                stdout: "Patch is empty, no changes applied".to_string(),
                stderr: String::new(),
                exit_code: 0,
            });
        }

        // Apply the patch
        match patch.apply(&req.cwd).await {
            Ok(results) => {
                let output = results.join("\n");
                Ok(ExecToolCallOutput {
                    stdout: output,
                    stderr: String::new(),
                    exit_code: 0,
                })
            }
            Err(e) => Ok(ExecToolCallOutput {
                stdout: String::new(),
                stderr: format!("Patch application failed: {}", e),
                exit_code: 1,
            }),
        }
    }
}

#[async_trait]
impl ToolHandler for ApplyPatchHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            payload,
            ToolPayload::Function { .. } | ToolPayload::Custom { .. }
        )
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        true // apply_patch always mutates
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let ToolInvocation {
            session,
            turn,
            tracker,
            call_id,
            tool_name,
            payload,
        } = invocation;

        // Extract patch input from payload
        let patch_input = match payload {
            ToolPayload::Function { arguments } => {
                let args: Value = serde_json::from_str(&arguments).map_err(|e| {
                    ToolCallError::respond(format!("Failed to parse function arguments: {}", e))
                })?;
                crate::tools::apply_patch::decode_apply_patch_input(&args)
                    .map_err(|e| {
                        ToolCallError::respond(format!("Failed to decode patch input: {e}"))
                    })?
                    .map(|input| input.text)
                    .ok_or_else(|| ToolCallError::respond("Missing patch input"))?
            }
            ToolPayload::Custom { input } => input,
            _ => {
                return Err(ToolCallError::respond(
                    "apply_patch handler received unsupported payload",
                ));
            }
        };

        // Parse the patch to get file changes
        let patch = Patch::parse(&patch_input)
            .map_err(|e| ToolCallError::respond(format!("Failed to parse patch: {}", e)))?;

        // Convert patch operations to file changes for tracking
        let changes = convert_patch_to_changes(&patch, &turn.cwd);

        // Create emitter for event tracking
        let emitter = ToolEmitter::apply_patch(changes.clone(), true);
        let event_ctx =
            ToolEventCtx::new(session.as_ref(), turn.as_ref(), &call_id, tracker.as_ref());
        emitter.begin(event_ctx).await;

        // Create request
        let req = ApplyPatchRequest {
            patch: patch_input.clone(),
            cwd: turn.cwd.clone(),
            timeout_ms: None,
            user_explicitly_approved: true,
        };

        // Execute using orchestrator
        let mut orchestrator = ToolOrchestrator::new();
        let mut runtime = ApplyPatchRuntime::new();
        let tool_ctx = ToolCtx {
            session: session.clone(),
            turn: turn.clone(),
            call_id: call_id.clone(),
            tool_name: tool_name.clone(),
        };

        let result = orchestrator
            .run(
                &mut runtime,
                &req,
                &tool_ctx,
                turn.as_ref(),
                map_approval_policy(turn.approval_policy.value()),
            )
            .await;

        // Emit completion event and format output
        let event_ctx =
            ToolEventCtx::new(session.as_ref(), turn.as_ref(), &call_id, tracker.as_ref());
        let content = emitter.finish(event_ctx, result).await?;

        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success: Some(true),
        })
    }
}

/// Convert patch operations to file changes for tracking
fn convert_patch_to_changes(patch: &Patch, cwd: &Path) -> HashMap<PathBuf, FileChange> {
    let mut changes = HashMap::new();

    for op in patch.operations() {
        match op {
            PatchOperation::AddFile { path, content } => {
                let full_path = cwd.join(path);
                changes.insert(
                    full_path,
                    FileChange::Add {
                        content: content.clone(),
                    },
                );
            }
            PatchOperation::DeleteFile { path } => {
                let full_path = cwd.join(path);
                changes.insert(full_path, FileChange::Delete);
            }
            PatchOperation::UpdateFile {
                path,
                new_path,
                chunks: _,
            } => {
                let full_path = cwd.join(path);
                if let Some(new_path) = new_path {
                    changes.insert(
                        full_path,
                        FileChange::Rename {
                            new_path: cwd.join(new_path),
                            content: None,
                        },
                    );
                } else {
                    // For updates, we track as update with empty placeholders
                    // The actual content will be computed during application
                    changes.insert(
                        full_path,
                        FileChange::Update {
                            old_content: String::new(),
                            new_content: String::new(),
                        },
                    );
                }
            }
        }
    }

    changes
}

/// Create freeform apply_patch tool spec (for GPT-5 style models)
pub fn create_apply_patch_freeform_tool() -> ToolSpec {
    ToolSpec::Freeform(FreeformTool {
        name: "apply_patch".to_string(),
        description: APPLY_PATCH_DESCRIPTION.to_string(),
        format: FreeformToolFormat {
            lark_grammar: Some(APPLY_PATCH_LARK_GRAMMAR.to_string()),
            examples: vec![
                APPLY_PATCH_ADD_EXAMPLE.to_string(),
                APPLY_PATCH_UPDATE_EXAMPLE.to_string(),
            ],
        },
    })
}

/// Create JSON function apply_patch tool spec (for standard function calling)
pub fn create_apply_patch_json_tool() -> ToolSpec {
    let mut properties = BTreeMap::new();
    properties.insert(
        "input".to_string(),
        JsonSchema::String {
            description: Some("The entire contents of the apply_patch command".to_string()),
        },
    );
    properties.insert(
        "patch".to_string(),
        JsonSchema::String {
            description: Some(crate::tools::apply_patch::APPLY_PATCH_ALIAS_DESCRIPTION.to_string()),
        },
    );

    ToolSpec::Function(ResponsesApiTool {
        name: "apply_patch".to_string(),
        description: format!(
            "{}\n\n{}",
            APPLY_PATCH_DESCRIPTION, APPLY_PATCH_GRAMMAR_HELP
        ),
        strict: false,
        parameters: JsonSchema::Object {
            properties,
            required: Some(vec!["input".to_string()]),
            additional_properties: Some(false.into()),
            any_of: None,
        },
    })
}

/// Intercept apply_patch from shell command
///
/// This checks if a shell command is actually an apply_patch invocation
/// and handles it through the apply_patch handler instead.
#[allow(clippy::too_many_arguments)]
pub async fn intercept_apply_patch(
    command: &[String],
    ctx: InterceptApplyPatchContext<'_>,
) -> Result<Option<ToolOutput>, ToolCallError> {
    // Check if this is an apply_patch command
    let (is_apply_patch, patch_content) = parse_apply_patch_command(command);

    if !is_apply_patch {
        return Ok(None);
    }

    let Some(patch_input) = patch_content else {
        return Ok(None);
    };

    // Log warning about using shell for apply_patch
    ctx.session
        .record_warning(format!(
            "apply_patch was requested via {}. Use the apply_patch tool instead of shell command execution.",
            ctx.tool_name
        ))
        .await;

    // Parse the patch
    let patch = Patch::parse(&patch_input)
        .map_err(|e| ToolCallError::respond(format!("Failed to parse patch: {}", e)))?;

    let changes = convert_patch_to_changes(&patch, ctx.cwd);

    // Create emitter
    let emitter = ToolEmitter::apply_patch(changes.clone(), true);
    let event_ctx = ToolEventCtx::new(
        ctx.session.as_ref(),
        ctx.turn.as_ref(),
        ctx.call_id,
        ctx.tracker,
    );
    emitter.begin(event_ctx).await;

    // Execute
    let req = ApplyPatchRequest {
        patch: patch_input,
        cwd: ctx.cwd.to_path_buf(),
        timeout_ms: ctx.timeout_ms,
        user_explicitly_approved: true,
    };

    let mut orchestrator = ToolOrchestrator::new();
    let mut runtime = ApplyPatchRuntime::new();
    let tool_ctx = ToolCtx {
        session: ctx.session.clone(),
        turn: ctx.turn.clone(),
        call_id: ctx.call_id.to_string(),
        tool_name: ctx.tool_name.to_string(),
    };

    let result = orchestrator
        .run(
            &mut runtime,
            &req,
            &tool_ctx,
            ctx.turn.as_ref(),
            map_approval_policy(ctx.turn.approval_policy.value()),
        )
        .await;

    let event_ctx = ToolEventCtx::new(
        ctx.session.as_ref(),
        ctx.turn.as_ref(),
        ctx.call_id,
        ctx.tracker,
    );
    let content = emitter.finish(event_ctx, result).await?;

    Ok(Some(ToolOutput::Function {
        content,
        content_items: None,
        success: Some(true),
    }))
}

fn map_approval_policy(policy: ApprovalPolicy) -> AskForApproval {
    match policy {
        ApprovalPolicy::Never => AskForApproval::Never,
        ApprovalPolicy::OnMutation => AskForApproval::OnRequest,
        ApprovalPolicy::Always => AskForApproval::UnlessTrusted,
    }
}

/// Parse a shell command to check if it's an apply_patch invocation
pub(crate) fn parse_apply_patch_command(command: &[String]) -> (bool, Option<String>) {
    const APPLY_PATCH_COMMANDS: &[&str] = &["apply_patch", "applypatch"];

    match command {
        // Direct invocation: apply_patch <patch>
        [cmd, body] if APPLY_PATCH_COMMANDS.contains(&cmd.as_str()) => (true, Some(body.clone())),
        // Shell heredoc form is not directly supported here
        // The Codex implementation uses tree-sitter to parse these
        _ => (false, None),
    }
}

// Constants for tool descriptions
const APPLY_PATCH_DESCRIPTION: &str = r#"Use the `apply_patch` tool to edit files.
Your patch language is a stripped-down, file-oriented diff format designed to be easy to parse and safe to apply.

You can think of it as a high-level envelope:

*** Begin Patch
[ one or more file sections ]
*** End Patch

Within that envelope, you get a sequence of file operations.
You MUST include a header to specify the action you are taking.
Each operation starts with one of three headers:

*** Add File: <path> - create a new file. Every following line is a + line (the initial contents).
*** Delete File: <path> - remove an existing file. Nothing follows.
*** Update File: <path> - patch an existing file in place (optionally with a rename)."#;

const APPLY_PATCH_GRAMMAR_HELP: &str = r#"May be immediately followed by *** Move to: <new path> if you want to rename the file.
Then one or more "hunks", each introduced by @@ (optionally followed by a hunk header).
Within a hunk each line starts with:

- ` ` (space) for context lines
- `-` for lines to remove
- `+` for lines to add

Important rules:
- You must include a header with your intended action (Add/Delete/Update)
- You must prefix new lines with `+` even when creating a new file
- File references can only be relative, NEVER ABSOLUTE
- Prefer small hunks with stable semantic @@ anchors like function, class, method, or impl names"#;

const APPLY_PATCH_LARK_GRAMMAR: &str = r#"
patch := "*** Begin Patch" NEWLINE { operation } "*** End Patch"
operation := AddFile | DeleteFile | UpdateFile
AddFile := "*** Add File: " path NEWLINE { "+" text NEWLINE }
DeleteFile := "*** Delete File: " path NEWLINE
UpdateFile := "*** Update File: " path NEWLINE [ MoveTo ] { Hunk }
MoveTo := "*** Move to: " newPath NEWLINE
Hunk := "@@" [ header ] NEWLINE { HunkLine } [ "*** End of File" NEWLINE ]
HunkLine := (" " | "-" | "+") text NEWLINE
"#;

const APPLY_PATCH_ADD_EXAMPLE: &str = r#"*** Begin Patch
*** Add File: hello.txt
+Hello world
*** End Patch"#;

const APPLY_PATCH_UPDATE_EXAMPLE: &str = r#"*** Begin Patch
*** Update File: src/app.py
*** Move to: src/main.py
@@ def greet():
-print("Hi")
+print("Hello, world!")
*** End Patch"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec_policy::RejectConfig;

    #[test]
    fn test_parse_apply_patch_command_direct() {
        let cmd = vec![
            "apply_patch".to_string(),
            "*** Begin Patch\n*** End Patch".to_string(),
        ];
        let (is_patch, content) = parse_apply_patch_command(&cmd);
        assert!(is_patch);
        assert!(content.is_some());
    }

    #[test]
    fn test_parse_apply_patch_command_not_patch() {
        let cmd = vec!["ls".to_string(), "-la".to_string()];
        let (is_patch, content) = parse_apply_patch_command(&cmd);
        assert!(!is_patch);
        assert!(content.is_none());
    }

    #[test]
    fn test_create_freeform_tool() {
        let tool = create_apply_patch_freeform_tool();
        assert_eq!(tool.name(), "apply_patch");
    }

    #[test]
    fn test_create_json_tool() {
        let tool = create_apply_patch_json_tool();
        assert_eq!(tool.name(), "apply_patch");
    }

    #[test]
    fn test_apply_patch_json_args_support_patch_alias() {
        let parsed: ApplyPatchToolArgs =
            serde_json::from_str(r#"{"patch":"*** Begin Patch\n*** End Patch\n"}"#)
                .expect("json args should parse");

        assert_eq!(parsed.input, None);
        assert_eq!(
            parsed.patch.as_deref(),
            Some("*** Begin Patch\n*** End Patch\n")
        );
    }

    #[test]
    fn wants_no_sandbox_approval_reject_respects_sandbox_flag() {
        let runtime = ApplyPatchRuntime::new();
        assert!(runtime.wants_no_sandbox_approval(AskForApproval::OnRequest));
        assert!(
            !runtime.wants_no_sandbox_approval(AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: false,
                request_permissions: false,
                mcp_elicitations: false,
            }))
        );
        assert!(
            runtime.wants_no_sandbox_approval(AskForApproval::Reject(RejectConfig {
                sandbox_approval: false,
                rules: false,
                request_permissions: false,
                mcp_elicitations: false,
            }))
        );
    }
}
