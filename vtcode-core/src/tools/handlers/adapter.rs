//! Adapter layer connecting Codex-style ToolHandler to vtcode's Tool trait.
//!
//! This module bridges the new handler architecture with the existing tool system,
//! enabling gradual migration while maintaining backward compatibility.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use super::tool_handler::{
    ApprovalPolicy, ShellEnvironmentPolicy, ToolCallError, ToolEvent, ToolHandler, ToolInvocation,
    ToolKind, ToolOutput, ToolPayload, ToolSession, ToolSpec, TurnContext,
};
use crate::tool_policy::ToolPolicy;
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;

/// Adapter that wraps a ToolHandler to implement the Tool trait.
///
/// This allows Codex-style handlers to be used in the existing vtcode tool registry.
pub struct HandlerToToolAdapter<H: ToolHandler> {
    handler: Arc<H>,
    name: &'static str,
    description: &'static str,
    spec: ToolSpec,
    session_factory: Arc<dyn Fn() -> Arc<dyn ToolSession> + Send + Sync>,
}

impl<H: ToolHandler + 'static> HandlerToToolAdapter<H> {
    pub fn new(
        handler: H,
        name: &'static str,
        description: &'static str,
        spec: ToolSpec,
        session_factory: impl Fn() -> Arc<dyn ToolSession> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
            name,
            description,
            spec,
            session_factory: Arc::new(session_factory),
        }
    }

    fn create_invocation(&self, args: Value) -> ToolInvocation {
        let session = (self.session_factory)();
        let turn = Arc::new(TurnContext {
            cwd: session.cwd().clone(),
            turn_id: uuid::Uuid::new_v4().to_string(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::Inherit,
            approval_policy: ApprovalPolicy::Never, // Approval handled by existing system
            codex_linux_sandbox_exe: None,
            sandbox_policy: Default::default(),
        });

        ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: uuid::Uuid::new_v4().to_string(),
            tool_name: self.name.to_string(),
            payload: ToolPayload::Function {
                arguments: serde_json::to_string(&args).unwrap_or_default(),
            },
        }
    }

    fn output_to_value(&self, output: ToolOutput) -> Value {
        let (text, is_success) = match &output {
            ToolOutput::Function { content, .. } => (content.clone(), output.is_success()),
            ToolOutput::Mcp { result } => {
                let text = result
                    .content
                    .iter()
                    .filter_map(|c| c.as_text())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                (text, output.is_success())
            }
        };

        serde_json::json!({
            "success": is_success,
            "content": text,
        })
    }
}

#[async_trait]
impl<H: ToolHandler + 'static> Tool for HandlerToToolAdapter<H> {
    async fn execute(&self, args: Value) -> Result<Value> {
        let invocation = self.create_invocation(args);

        match self.handler.handle(invocation).await {
            Ok(output) => Ok(self.output_to_value(output)),
            Err(ToolCallError::RespondToModel(msg)) => Ok(serde_json::json!({
                "success": false,
                "error": msg,
            })),
            Err(ToolCallError::Rejected(msg)) => Ok(serde_json::json!({
                "success": false,
                "rejected": true,
                "error": msg,
            })),
            Err(ToolCallError::Timeout(ms)) => Ok(serde_json::json!({
                "success": false,
                "timeout": true,
                "timeout_ms": ms,
            })),
            Err(ToolCallError::Internal(e)) => Err(e),
        }
    }

    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        let invocation = self.create_invocation(args);

        match self.handler.handle(invocation).await {
            Ok(output) => {
                let ui_content = output.content().unwrap_or("").to_string();

                // Create a summary for LLM (first 500 chars or key info)
                let llm_content = if ui_content.len() > 500 {
                    format!(
                        "{}...[truncated, {} chars total]",
                        &ui_content[..500],
                        ui_content.len()
                    )
                } else {
                    ui_content.clone()
                };

                Ok(SplitToolResult::new(self.name, &llm_content, &ui_content))
            }
            Err(e) => {
                let error_msg = e.to_string();
                Ok(SplitToolResult::error(self.name, &error_msg))
            }
        }
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn parameter_schema(&self) -> Option<Value> {
        match &self.spec {
            ToolSpec::Function(tool) => serde_json::to_value(&tool.parameters).ok(),
            ToolSpec::Freeform(tool) => serde_json::to_value(&tool.format).ok(),
            _ => None,
        }
    }

    fn default_permission(&self) -> ToolPolicy {
        // Map handler mutability to policy
        ToolPolicy::Prompt
    }
}

/// Adapter that wraps a Tool to implement ToolHandler trait.
///
/// This allows existing vtcode tools to be used in the new Codex-style router.
pub struct ToolToHandlerAdapter {
    tool: Arc<dyn Tool>,
}

impl ToolToHandlerAdapter {
    pub fn new(tool: Arc<dyn Tool>) -> Self {
        Self { tool }
    }
}

#[async_trait]
impl ToolHandler for ToolToHandlerAdapter {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        // Conservative default: assume mutating unless we know otherwise
        !matches!(self.tool.default_permission(), ToolPolicy::Allow)
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        // Extract arguments from payload
        let args: Value = match &invocation.payload {
            ToolPayload::Function { arguments } => serde_json::from_str(arguments)
                .map_err(|e| ToolCallError::respond(format!("Invalid arguments: {e}")))?,
            _ => return Err(ToolCallError::respond("Unsupported payload type")),
        };

        // Execute the underlying tool
        match self.tool.execute(args).await {
            Ok(result) => {
                let text = if result.is_string() {
                    result.as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
                };

                Ok(ToolOutput::simple(text))
            }
            Err(e) => Err(ToolCallError::Internal(e)),
        }
    }
}

/// Default session implementation for adapters.
pub struct DefaultToolSession {
    cwd: PathBuf,
    workspace_root: PathBuf,
    shell: String,
}

impl DefaultToolSession {
    pub fn new(cwd: PathBuf) -> Self {
        let workspace_root = cwd.clone();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        Self {
            cwd,
            workspace_root,
            shell,
        }
    }

    pub fn with_workspace(cwd: PathBuf, workspace_root: PathBuf) -> Self {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        Self {
            cwd,
            workspace_root,
            shell,
        }
    }
}

#[async_trait]
impl ToolSession for DefaultToolSession {
    fn cwd(&self) -> &PathBuf {
        &self.cwd
    }

    fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }

    async fn record_warning(&self, message: String) {
        tracing::warn!("{}", message);
    }

    fn user_shell(&self) -> &str {
        &self.shell
    }

    async fn send_event(&self, event: ToolEvent) {
        match event {
            ToolEvent::Begin(e) => {
                tracing::debug!(tool = %e.tool_name, call_id = %e.call_id, "Tool execution started");
            }
            ToolEvent::Success(e) => {
                tracing::debug!(call_id = %e.call_id, "Tool execution succeeded");
            }
            ToolEvent::Failure(e) => {
                tracing::warn!(call_id = %e.call_id, error = %e.error, "Tool execution failed");
            }
            _ => {}
        }
    }
}

/// Factory function to create a session for the current directory.
pub fn create_cwd_session() -> Arc<dyn ToolSession> {
    Arc::new(DefaultToolSession::new(
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::tool_handler::{JsonSchema, ResponsesApiTool};
    use super::*;
    use std::collections::BTreeMap;

    struct TestHandler;

    #[async_trait]
    impl ToolHandler for TestHandler {
        fn kind(&self) -> ToolKind {
            ToolKind::Function
        }

        async fn handle(&self, _invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
            Ok(ToolOutput::simple("Test output"))
        }
    }

    #[tokio::test]
    async fn test_handler_to_tool_adapter() {
        let spec = ToolSpec::Function(ResponsesApiTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::new(),
                required: None,
                additional_properties: None,
            },
            strict: false,
        });

        let adapter = HandlerToToolAdapter::new(
            TestHandler,
            "test_tool",
            "A test tool",
            spec,
            create_cwd_session,
        );

        assert_eq!(adapter.name(), "test_tool");
        assert_eq!(adapter.description(), "A test tool");

        let result = adapter.execute(serde_json::json!({})).await.unwrap();
        assert!(
            result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_default_tool_session() {
        let session = DefaultToolSession::new(PathBuf::from("/tmp"));
        assert_eq!(session.cwd(), &PathBuf::from("/tmp"));
        assert_eq!(session.workspace_root(), &PathBuf::from("/tmp"));
    }
}
