//! Canonical public tool catalog accessors for ToolRegistry.

use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::tools::handlers::{
    SessionSurface, SessionToolsConfig, ToolCallError, ToolModelCapabilities, ToolSchemaEntry,
};

use super::ToolRegistry;

impl ToolRegistry {
    pub(super) async fn rebuild_tool_assembly(&self) {
        let registrations = self.inventory.registrations_snapshot();
        let next = super::assembly::ToolAssembly::from_registrations(registrations);
        *self
            .tool_assembly
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }

    pub async fn public_tool_names(
        &self,
        surface: SessionSurface,
        capability_level: CapabilityLevel,
    ) -> Vec<String> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly
            .catalog()
            .public_tool_names(SessionToolsConfig::full_public(
                surface,
                capability_level,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            ))
    }

    pub async fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.catalog().schema_entries(config)
    }

    pub async fn function_declarations(
        &self,
        config: SessionToolsConfig,
    ) -> Vec<FunctionDeclaration> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.catalog().function_declarations(config)
    }

    pub async fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.catalog().model_tools(config)
    }

    pub async fn schema_for_public_name(
        &self,
        name: &str,
        config: SessionToolsConfig,
    ) -> Option<ToolSchemaEntry> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.catalog().schema_for_name(name, config)
    }

    pub(crate) fn resolve_public_tool_name_sync(
        &self,
        name: &str,
    ) -> Result<String, ToolCallError> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly
            .resolve_public_tool(name)
            .map(|resolution| resolution.registration_name().to_string())
    }

    pub(super) fn resolve_public_tool(
        &self,
        name: &str,
    ) -> Result<super::assembly::PublicToolResolution, ToolCallError> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.resolve_public_tool(name)
    }

    pub(crate) async fn resolve_public_tool_name(
        &self,
        name: &str,
    ) -> Result<String, ToolCallError> {
        self.resolve_public_tool_name_sync(name)
    }
}

#[cfg(test)]
mod tests {
    use super::super::assembly::public_tool_name_candidates;

    #[test]
    fn public_tool_name_candidates_keep_lowercase_human_label() {
        let candidates = public_tool_name_candidates("Exec code");
        assert!(candidates.iter().any(|candidate| candidate == "exec code"));
        assert!(candidates.iter().any(|candidate| candidate == "exec_code"));
    }

    #[test]
    fn public_tool_name_candidates_strip_tool_prefixes() {
        let candidates = public_tool_name_candidates("functions.read_file");
        assert!(candidates.iter().any(|candidate| candidate == "read_file"));
    }
}
