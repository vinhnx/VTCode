use serde_json::Value;
use vtcode_core::llm::provider::ToolDefinition;

use crate::acp::tooling::{AcpToolRegistry, SupportedTool, ToolDescriptor};

pub trait ToolRegistryProvider {
    fn registered_tools(&self) -> Vec<SupportedTool>;

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition>;

    fn render_title(&self, descriptor: ToolDescriptor, function_name: &str, args: &Value)
    -> String;

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor>;

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition>;

    fn has_local_tools(&self) -> bool;
}

impl ToolRegistryProvider for AcpToolRegistry {
    fn registered_tools(&self) -> Vec<SupportedTool> {
        AcpToolRegistry::registered_tools(self)
    }

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        AcpToolRegistry::definitions_for(self, enabled_tools, include_local)
    }

    fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        AcpToolRegistry::render_title(self, descriptor, function_name, args)
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        AcpToolRegistry::lookup(self, function_name)
    }

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        AcpToolRegistry::local_definition(self, tool_name)
    }

    fn has_local_tools(&self) -> bool {
        AcpToolRegistry::has_local_tools(self)
    }
}

impl<T> ToolRegistryProvider for std::rc::Rc<T>
where
    T: ToolRegistryProvider,
{
    fn registered_tools(&self) -> Vec<SupportedTool> {
        <T as ToolRegistryProvider>::registered_tools(&**self)
    }

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        <T as ToolRegistryProvider>::definitions_for(&**self, enabled_tools, include_local)
    }

    fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        <T as ToolRegistryProvider>::render_title(&**self, descriptor, function_name, args)
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        <T as ToolRegistryProvider>::lookup(&**self, function_name)
    }

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        <T as ToolRegistryProvider>::local_definition(&**self, tool_name)
    }

    fn has_local_tools(&self) -> bool {
        <T as ToolRegistryProvider>::has_local_tools(&**self)
    }
}
