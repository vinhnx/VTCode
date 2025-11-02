use super::ToolRegistry;
use crate::config::types::CapabilityLevel;
use crate::tools::traits::Tool;
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

pub type ToolExecutorFn =
    for<'a> fn(&'a mut ToolRegistry, Value) -> BoxFuture<'a, anyhow::Result<Value>>;

use std::fmt;

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
        }
    }

    pub fn from_tool(name: &'static str, capability: CapabilityLevel, tool: Arc<dyn Tool>) -> Self {
        Self {
            name,
            capability,
            uses_pty: false,
            expose_in_llm: true,
            deprecated: false,
            deprecation_message: None,
            handler: ToolHandler::TraitObject(tool),
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
}
