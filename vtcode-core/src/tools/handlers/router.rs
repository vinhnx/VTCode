//! Tool routing and dispatch system (from Codex).
//!
//! The ToolRouter provides the critical dispatch layer that:
//! - Builds tool calls from LLM response items
//! - Routes tool calls to appropriate handlers
//! - Manages tool registry with specs
//!
//! This module bridges LLM outputs to tool execution.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolOutput, ToolPayload, ToolSession, ToolSpec,
    TurnContext,
};

/// A parsed tool call ready for dispatch.
#[derive(Clone, Debug)]
pub struct ToolCall {
    /// Name of the tool to invoke.
    pub tool_name: String,
    /// Unique identifier for this call.
    pub call_id: String,
    /// Payload containing arguments.
    pub payload: ToolPayload,
}

/// Configured tool specification with parallel execution support.
#[derive(Clone, Debug)]
pub struct ConfiguredToolSpec {
    /// The tool specification.
    pub spec: ToolSpec,
    /// Whether this tool can be executed in parallel with others.
    pub supports_parallel_tool_calls: bool,
}

impl ConfiguredToolSpec {
    pub fn new(spec: ToolSpec, supports_parallel_tool_calls: bool) -> Self {
        Self {
            spec,
            supports_parallel_tool_calls,
        }
    }
}

/// Tool registry holding handler mappings.
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
}

fn normalize_router_tool_name(tool_name: &str) -> Option<String> {
    let lowered = tool_name.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }

    let normalized = lowered
        .replace([' ', '-'], "_")
        .replace(['(', ')', '\'', '"'], "");

    let mapped = match normalized.as_str() {
        "exec_code" | "run_code" | "run_command" | "run_command_pty" | "container.exec"
        | "bash" => "run_pty_cmd",
        "search_text" => "grep_file",
        "read_file" => "read_file",
        "write_file" => "write_file",
        "edit_file" => "edit_file",
        "list_files" => "list_files",
        _ => normalized.as_str(),
    };

    if mapped == lowered {
        None
    } else {
        Some(mapped.to_string())
    }
}

fn suggest_similar_tool_names(
    requested_tool_name: &str,
    handlers: &HashMap<String, Arc<dyn ToolHandler>>,
) -> Vec<String> {
    let requested_lower = requested_tool_name.to_ascii_lowercase();
    let normalized = normalize_router_tool_name(requested_tool_name).unwrap_or_default();

    let mut available: Vec<String> = handlers.keys().cloned().collect();
    available.sort_unstable();

    available
        .into_iter()
        .filter(|candidate| {
            candidate.contains(&requested_lower)
                || requested_lower.contains(candidate)
                || (!normalized.is_empty()
                    && (candidate.contains(&normalized) || normalized.contains(candidate)))
        })
        .take(3)
        .collect()
}

impl ToolRegistry {
    pub fn new(handlers: HashMap<String, Arc<dyn ToolHandler>>) -> Self {
        Self { handlers }
    }

    pub fn handler(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Dispatch a tool invocation to the appropriate handler.
    pub async fn dispatch(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        let tool_name = &invocation.tool_name;

        let normalized_name = normalize_router_tool_name(tool_name);
        let handler = self.handler(tool_name).or_else(|| {
            normalized_name
                .as_deref()
                .and_then(|candidate| self.handler(candidate))
        });
        let handler = handler.ok_or_else(|| {
            let suggested = suggest_similar_tool_names(tool_name, &self.handlers);
            let normalized_hint = normalized_name
                .as_deref()
                .filter(|candidate| *candidate != tool_name)
                .map(|candidate| format!(" Normalized as '{candidate}'."))
                .unwrap_or_default();
            let suggestion_hint = if suggested.is_empty() {
                String::new()
            } else {
                format!(" Did you mean: {}?", suggested.join(", "))
            };
            ToolCallError::respond(format!(
                "Unknown tool: {tool_name}.{normalized_hint}{suggestion_hint}"
            ))
        })?;

        if !handler.matches_kind(&invocation.payload) {
            return Err(ToolCallError::respond(format!(
                "Tool {tool_name} invoked with incompatible payload type"
            )));
        }

        handler.handle(invocation).await
    }
}

/// Builder for constructing ToolRegistry with specs.
pub struct ToolRegistryBuilder {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    specs: Vec<ConfiguredToolSpec>,
}

impl Default for ToolRegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistryBuilder {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            specs: Vec::new(),
        }
    }

    /// Add a tool spec without parallel support.
    pub fn push_spec(&mut self, spec: ToolSpec) -> &mut Self {
        self.push_spec_with_parallel_support(spec, false)
    }

    /// Add a tool spec with parallel support flag.
    pub fn push_spec_with_parallel_support(
        &mut self,
        spec: ToolSpec,
        supports_parallel_tool_calls: bool,
    ) -> &mut Self {
        self.specs
            .push(ConfiguredToolSpec::new(spec, supports_parallel_tool_calls));
        self
    }

    /// Register a handler for a tool name.
    pub fn register_handler(
        &mut self,
        name: impl Into<String>,
        handler: Arc<dyn ToolHandler>,
    ) -> &mut Self {
        let name = name.into();
        if self.handlers.contains_key(&name) {
            tracing::warn!("Overwriting handler for tool {name}");
        }
        self.handlers.insert(name, handler);
        self
    }

    /// Register multiple tool name aliases for the same handler.
    pub fn register_aliases(&mut self, names: &[&str], handler: Arc<dyn ToolHandler>) -> &mut Self {
        for name in names {
            self.handlers.insert((*name).to_string(), handler.clone());
        }
        self
    }

    /// Build the registry and return specs.
    pub fn build(self) -> (Vec<ConfiguredToolSpec>, ToolRegistry) {
        let registry = ToolRegistry::new(self.handlers);
        (self.specs, registry)
    }
}

/// The main router that builds and dispatches tool calls.
///
/// This is the central component that:
/// 1. Builds tool calls from LLM response items
/// 2. Dispatches calls to registered handlers
/// 3. Manages tool specifications for the LLM
pub struct ToolRouter {
    registry: ToolRegistry,
    specs: Vec<ConfiguredToolSpec>,
}

impl ToolRouter {
    /// Create a router from a builder.
    pub fn from_builder(builder: ToolRegistryBuilder) -> Self {
        let (specs, registry) = builder.build();
        Self { registry, specs }
    }

    /// Get tool specs for sending to the LLM.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.specs.iter().map(|c| c.spec.clone()).collect()
    }

    /// Get configured specs with parallel support info.
    pub fn configured_specs(&self) -> &[ConfiguredToolSpec] {
        &self.specs
    }

    /// Check if a tool supports parallel execution.
    pub fn tool_supports_parallel(&self, tool_name: &str) -> bool {
        self.specs
            .iter()
            .filter(|c| c.supports_parallel_tool_calls)
            .any(|c| c.spec.name() == tool_name)
    }

    /// Build a ToolCall from a function call response.
    ///
    /// This parses LLM output into a structured ToolCall that can be dispatched.
    pub fn build_tool_call(
        name: String,
        call_id: String,
        arguments: String,
        mcp_prefix: Option<&str>,
    ) -> Result<ToolCall, ToolCallError> {
        // Check if this is an MCP tool call (has server prefix)
        if let Some(prefix) = mcp_prefix
            && name.starts_with(prefix)
        {
            let parts: Vec<&str> = name.splitn(2, '/').collect();
            if parts.len() == 2 {
                return Ok(ToolCall {
                    tool_name: name.clone(),
                    call_id,
                    payload: ToolPayload::Mcp {
                        arguments: Some(serde_json::from_str(&arguments).unwrap_or_default()),
                    },
                });
            }
        }

        // Standard function call
        Ok(ToolCall {
            tool_name: name,
            call_id,
            payload: ToolPayload::Function { arguments },
        })
    }

    /// Dispatch a tool call to its handler.
    pub async fn dispatch_tool_call(
        &self,
        session: Arc<dyn ToolSession>,
        turn: Arc<TurnContext>,
        call: ToolCall,
    ) -> Result<ToolOutput, ToolCallError> {
        let invocation = ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: call.call_id,
            tool_name: call.tool_name,
            payload: call.payload,
        };

        self.registry.dispatch(invocation).await
    }

    /// Create a failure response for a tool call.
    pub fn failure_response(_call_id: String, error: ToolCallError) -> ToolOutput {
        ToolOutput::error(error.to_string())
    }
}

/// Trait for types that can provide a ToolRouter.
#[async_trait]
pub trait ToolRouterProvider: Send + Sync {
    /// Get or build a tool router.
    async fn get_tool_router(&self) -> Arc<ToolRouter>;
}

#[cfg(test)]
mod tests {
    use super::super::tool_handler::{ResponsesApiTool, ToolKind};
    use super::*;

    struct MockHandler;

    #[async_trait]
    impl ToolHandler for MockHandler {
        fn kind(&self) -> ToolKind {
            ToolKind::Function
        }

        async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
            Ok(ToolOutput::simple(format!(
                "Handled: {}",
                invocation.tool_name
            )))
        }
    }

    #[test]
    fn test_build_tool_call_function() {
        let call = ToolRouter::build_tool_call(
            "test_tool".to_string(),
            "call-1".to_string(),
            r#"{"arg": "value"}"#.to_string(),
            None,
        )
        .unwrap();

        assert_eq!(call.tool_name, "test_tool");
        assert_eq!(call.call_id, "call-1");
        assert!(matches!(call.payload, ToolPayload::Function { .. }));
    }

    #[test]
    fn test_build_tool_call_mcp() {
        let call = ToolRouter::build_tool_call(
            "mcp_server/do_thing".to_string(),
            "call-2".to_string(),
            r#"{"arg": "value"}"#.to_string(),
            Some("mcp_server"),
        )
        .unwrap();

        assert_eq!(call.tool_name, "mcp_server/do_thing");
        assert!(matches!(
            call.payload,
            ToolPayload::Mcp { arguments: Some(_) }
        ));
    }

    #[test]
    fn test_registry_builder() {
        let handler = Arc::new(MockHandler);
        let spec = ToolSpec::Function(ResponsesApiTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: super::super::tool_handler::JsonSchema::Object {
                properties: std::collections::BTreeMap::new(),
                required: None,
                additional_properties: None,
            },
            strict: false,
        });

        let mut builder = ToolRegistryBuilder::new();
        builder
            .push_spec_with_parallel_support(spec, true)
            .register_handler("test_tool", handler);

        let (specs, registry) = builder.build();

        assert_eq!(specs.len(), 1);
        assert!(specs[0].supports_parallel_tool_calls);
        assert!(registry.handler("test_tool").is_some());
    }

    #[test]
    fn test_router_parallel_support() {
        let handler = Arc::new(MockHandler);
        let spec = ToolSpec::Function(ResponsesApiTool {
            name: "parallel_tool".to_string(),
            description: "Supports parallel".to_string(),
            parameters: super::super::tool_handler::JsonSchema::Object {
                properties: std::collections::BTreeMap::new(),
                required: None,
                additional_properties: None,
            },
            strict: false,
        });

        let mut builder = ToolRegistryBuilder::new();
        builder
            .push_spec_with_parallel_support(spec, true)
            .register_handler("parallel_tool", handler);

        let router = ToolRouter::from_builder(builder);

        assert!(router.tool_supports_parallel("parallel_tool"));
        assert!(!router.tool_supports_parallel("nonexistent"));
    }

    #[test]
    fn test_normalize_router_tool_name_exec_code_label() {
        assert_eq!(
            normalize_router_tool_name("Exec code").as_deref(),
            Some("run_pty_cmd")
        );
        assert_eq!(
            normalize_router_tool_name("run command (PTY)").as_deref(),
            Some("run_pty_cmd")
        );
    }

    #[test]
    fn test_suggest_similar_tool_names_uses_normalized_form() {
        let mut handlers = HashMap::new();
        handlers.insert(
            "run_pty_cmd".to_string(),
            Arc::new(MockHandler) as Arc<dyn ToolHandler>,
        );

        let suggestions = suggest_similar_tool_names("Exec code", &handlers);
        assert_eq!(suggestions, vec!["run_pty_cmd".to_string()]);
    }
}
