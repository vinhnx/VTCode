use super::ToolRegistry;
use crate::config::types::CapabilityLevel;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

pub type ToolExecutorFn =
    for<'a> fn(&'a mut ToolRegistry, Value) -> BoxFuture<'a, anyhow::Result<Value>>;

use std::fmt;

#[derive(Debug, Clone, Default)]
pub struct ToolMetadata {
    parameter_schema: Option<Value>,
    config_schema: Option<Value>,
    state_schema: Option<Value>,
    prompt_path: Option<String>,
    default_permission: Option<ToolPolicy>,
    allowlist: Vec<String>,
    denylist: Vec<String>,
    aliases: Vec<String>,
    server_hint: Option<String>,
}

impl ToolMetadata {
    pub fn with_parameter_schema(mut self, schema: Value) -> Self {
        self.parameter_schema = Some(schema);
        self
    }

    pub fn with_config_schema(mut self, schema: Value) -> Self {
        self.config_schema = Some(schema);
        self
    }

    pub fn with_state_schema(mut self, schema: Value) -> Self {
        self.state_schema = Some(schema);
        self
    }

    pub fn with_prompt_path(mut self, path: impl Into<String>) -> Self {
        self.prompt_path = Some(path.into());
        self
    }

    pub fn with_permission(mut self, permission: ToolPolicy) -> Self {
        self.default_permission = Some(permission);
        self
    }

    pub fn with_allowlist(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allowlist.extend(patterns.into_iter().map(Into::into));
        self
    }

    pub fn with_denylist(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.denylist.extend(patterns.into_iter().map(Into::into));
        self
    }

    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases.extend(aliases.into_iter().map(Into::into));
        self
    }

    pub fn with_server_hint(mut self, hint: impl Into<String>) -> Self {
        self.server_hint = Some(hint.into());
        self
    }

    pub fn parameter_schema(&self) -> Option<&Value> {
        self.parameter_schema.as_ref()
    }

    pub fn config_schema(&self) -> Option<&Value> {
        self.config_schema.as_ref()
    }

    pub fn state_schema(&self) -> Option<&Value> {
        self.state_schema.as_ref()
    }

    pub fn prompt_path(&self) -> Option<&str> {
        self.prompt_path.as_deref()
    }

    pub fn default_permission(&self) -> Option<ToolPolicy> {
        self.default_permission.clone()
    }

    pub fn allowlist(&self) -> &[String] {
        &self.allowlist
    }

    pub fn denylist(&self) -> &[String] {
        &self.denylist
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub fn server_hint(&self) -> Option<&str> {
        self.server_hint.as_deref()
    }
}

#[derive(Clone)]
pub enum ToolHandler {
    RegistryFn(ToolExecutorFn),
    TraitObject(Arc<dyn Tool>),
}

impl fmt::Debug for ToolHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolHandler::RegistryFn(_) => write!(f, "ToolHandler::RegistryFn"),
            ToolHandler::TraitObject(_) => write!(f, "ToolHandler::TraitObject"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRegistration {
    name: &'static str,
    capability: CapabilityLevel,
    uses_pty: bool,
    expose_in_llm: bool,
    deprecated: bool,
    deprecation_message: Option<String>,
    handler: ToolHandler,
    metadata: ToolMetadata,
}

impl ToolRegistration {
    pub fn new(
        name: &'static str,
        capability: CapabilityLevel,
        uses_pty: bool,
        executor: ToolExecutorFn,
    ) -> Self {
        Self {
            name,
            capability,
            uses_pty,
            expose_in_llm: true,
            deprecated: false,
            deprecation_message: None,
            handler: ToolHandler::RegistryFn(executor),
            metadata: ToolMetadata::default(),
        }
    }

    pub fn from_tool(name: &'static str, capability: CapabilityLevel, tool: Arc<dyn Tool>) -> Self {
        let mut metadata = ToolMetadata::default();
        if let Some(schema) = tool.parameter_schema() {
            metadata = metadata.with_parameter_schema(schema);
        }
        if let Some(schema) = tool.config_schema() {
            metadata = metadata.with_config_schema(schema);
        }
        if let Some(schema) = tool.state_schema() {
            metadata = metadata.with_state_schema(schema);
        }
        if let Some(path) = tool.prompt_path() {
            metadata = metadata.with_prompt_path(path.into_owned());
        }
        metadata = metadata.with_permission(tool.default_permission());
        if let Some(patterns) = tool.allow_patterns() {
            metadata = metadata.with_allowlist(patterns.iter().copied());
        }
        if let Some(patterns) = tool.deny_patterns() {
            metadata = metadata.with_denylist(patterns.iter().copied());
        }

        Self {
            name,
            capability,
            uses_pty: false,
            expose_in_llm: true,
            deprecated: false,
            deprecation_message: None,
            handler: ToolHandler::TraitObject(tool),
            metadata,
        }
    }

    pub fn from_tool_instance<T>(name: &'static str, capability: CapabilityLevel, tool: T) -> Self
    where
        T: Tool + 'static,
    {
        Self::from_tool(name, capability, Arc::new(tool))
    }

    pub fn with_llm_visibility(mut self, expose: bool) -> Self {
        self.expose_in_llm = expose;
        self
    }

    pub fn with_pty(mut self, uses_pty: bool) -> Self {
        self.uses_pty = uses_pty;
        self
    }

    pub fn with_deprecated(mut self, deprecated: bool) -> Self {
        self.deprecated = deprecated;
        self
    }

    pub fn with_deprecation_message(mut self, message: impl Into<String>) -> Self {
        self.deprecation_message = Some(message.into());
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn capability(&self) -> CapabilityLevel {
        self.capability
    }

    pub fn uses_pty(&self) -> bool {
        self.uses_pty
    }

    pub fn expose_in_llm(&self) -> bool {
        self.expose_in_llm
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecated
    }

    pub fn deprecation_message(&self) -> Option<&str> {
        self.deprecation_message.as_deref()
    }

    pub fn handler(&self) -> ToolHandler {
        self.handler.clone()
    }

    pub fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    pub fn parameter_schema(&self) -> Option<&Value> {
        self.metadata.parameter_schema()
    }

    pub fn config_schema(&self) -> Option<&Value> {
        self.metadata.config_schema()
    }

    pub fn state_schema(&self) -> Option<&Value> {
        self.metadata.state_schema()
    }

    pub fn prompt_path(&self) -> Option<&str> {
        self.metadata.prompt_path()
    }

    pub fn default_permission(&self) -> Option<ToolPolicy> {
        self.metadata.default_permission()
    }

    pub fn with_metadata(mut self, metadata: ToolMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_prompt_path(mut self, path: impl Into<String>) -> Self {
        self.metadata = self.metadata.clone().with_prompt_path(path);
        self
    }

    pub fn with_parameter_schema(mut self, schema: Value) -> Self {
        self.metadata = self.metadata.clone().with_parameter_schema(schema);
        self
    }

    pub fn with_config_schema(mut self, schema: Value) -> Self {
        self.metadata = self.metadata.clone().with_config_schema(schema);
        self
    }

    pub fn with_state_schema(mut self, schema: Value) -> Self {
        self.metadata = self.metadata.clone().with_state_schema(schema);
        self
    }

    pub fn with_permission(mut self, permission: ToolPolicy) -> Self {
        self.metadata = self.metadata.clone().with_permission(permission);
        self
    }

    pub fn with_allowlist(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.metadata = self.metadata.clone().with_allowlist(patterns);
        self
    }

    pub fn with_denylist(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.metadata = self.metadata.clone().with_denylist(patterns);
        self
    }

    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.metadata = self.metadata.clone().with_aliases(aliases);
        self
    }

    pub fn with_server_hint(mut self, hint: impl Into<String>) -> Self {
        self.metadata = self.metadata.clone().with_server_hint(hint);
        self
    }
}
