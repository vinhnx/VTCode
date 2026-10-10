//! Canonical public tool catalog accessors for ToolRegistry.

use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::subagents::is_subagent_tool;
use crate::tools::handlers::{
    SessionSurface, SessionToolsConfig, ToolCallError, ToolModelCapabilities, ToolSchemaEntry,
};

use super::ToolRegistry;

impl ToolRegistry {
    fn matrix_catalog_config(&self, mut config: SessionToolsConfig) -> SessionToolsConfig {
        if self.matrix_coordinator.load(std::sync::atomic::Ordering::Acquire) && self.matrix_worker.read().is_none() {
            // This small control catalogue must remain available without a search tool.
            config.tool_profile = crate::config::ToolProfile::AdvancedVtCode;
            config.deferred_tool_policy = Default::default();
        }
        config
    }
    pub(super) async fn rebuild_tool_assembly(&self) {
        let registrations = self.inventory.registrations_snapshot();
        let next = super::assembly::ToolAssembly::from_registrations(registrations);
        *self.tool_assembly.write().unwrap_or_else(std::sync::PoisonError::into_inner) = next;
    }

    pub async fn public_tool_names(&self, surface: SessionSurface, capability_level: CapabilityLevel) -> Vec<String> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut names = assembly.catalog().public_tool_names(
            self.matrix_catalog_config(
                SessionToolsConfig::full_public(
                    surface,
                    capability_level,
                    ToolDocumentationMode::Full,
                    ToolModelCapabilities::default(),
                )
                .with_tool_profile(
                    *self
                        .active_tool_profile
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                )
                .with_planning_active(self.is_planning_active()),
            ),
        );
        if !self.has_subagent_controller() {
            names.retain(|name| !is_subagent_tool(name));
        }
        if !self.has_matrix_access() {
            names.retain(|name| name != "matrix");
        }
        names.retain(|name| self.enforce_matrix_role(name).is_ok());
        names
    }

    pub async fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut entries = assembly.catalog().schema_entries(self.matrix_catalog_config(config));
        if !self.has_subagent_controller() {
            entries.retain(|entry| !is_subagent_tool(entry.name.as_str()));
        }
        if !self.has_matrix_access() {
            entries.retain(|entry| entry.name != "matrix");
        }
        entries.retain(|entry| self.enforce_matrix_role(&entry.name).is_ok());
        entries
    }

    pub async fn function_declarations(&self, config: SessionToolsConfig) -> Vec<FunctionDeclaration> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut declarations = assembly.catalog().function_declarations(self.matrix_catalog_config(config));
        if !self.has_subagent_controller() {
            declarations.retain(|entry| !is_subagent_tool(entry.name.as_str()));
        }
        if !self.has_matrix_access() {
            declarations.retain(|entry| entry.name != "matrix");
        }
        declarations.retain(|entry| self.enforce_matrix_role(&entry.name).is_ok());
        declarations
    }

    pub async fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut tools = assembly.catalog().model_tools(self.matrix_catalog_config(config));
        if !self.has_subagent_controller() {
            tools.retain(|entry| !is_subagent_tool(entry.function_name()));
        }
        if !self.has_matrix_access() {
            tools.retain(|entry| entry.function_name() != "matrix");
        }
        tools.retain(|entry| self.enforce_matrix_role(entry.function_name()).is_ok());
        tools
    }

    pub async fn schema_for_public_name(&self, name: &str, config: SessionToolsConfig) -> Option<ToolSchemaEntry> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        if name == "matrix" && !self.has_matrix_access() {
            return None;
        }
        if !self.has_subagent_controller() && is_subagent_tool(name) {
            return None;
        }
        if self.enforce_matrix_role(name).is_err() {
            return None;
        }
        assembly.catalog().schema_for_name(name, self.matrix_catalog_config(config))
    }

    pub(crate) fn resolve_public_tool_name_sync(&self, name: &str) -> Result<String, ToolCallError> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly
            .resolve_public_tool(name)
            .map(|resolution| resolution.registration_name().to_string())
    }

    pub(super) fn resolve_public_tool(
        &self,
        name: &str,
    ) -> Result<super::assembly::PublicToolResolution, ToolCallError> {
        let assembly = self.tool_assembly.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        assembly.resolve_public_tool(name)
    }

    pub(crate) fn resolve_public_tool_name(&self, name: &str) -> Result<String, ToolCallError> {
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
