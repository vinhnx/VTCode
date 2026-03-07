//! Canonical public tool catalog accessors for ToolRegistry.

use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use crate::llm::provider::ToolDefinition;
use crate::tools::handlers::{
    SessionSurface, SessionToolsConfig, ToolCallError, ToolModelCapabilities, ToolSchemaEntry,
};
use crate::tools::names::canonical_tool_name;

use super::ToolRegistry;

fn strip_wrapping_quotes(value: &str) -> &str {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
}

fn strip_tool_namespace_prefix(value: &str) -> &str {
    for prefix in [
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "assistant.",
        "recipient_name.",
    ] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            return stripped;
        }
    }
    value
}

fn push_candidate(candidates: &mut Vec<String>, value: &str) {
    let trimmed = strip_wrapping_quotes(value);
    if trimmed.is_empty() {
        return;
    }

    if !candidates.iter().any(|existing| existing == trimmed) {
        candidates.push(trimmed.to_string());
    }

    let stripped = strip_tool_namespace_prefix(trimmed);
    if stripped != trimmed && !candidates.iter().any(|existing| existing == stripped) {
        candidates.push(stripped.to_string());
    }

    let lowered = stripped.trim().to_ascii_lowercase();
    if !lowered.is_empty() && !candidates.iter().any(|existing| existing == &lowered) {
        candidates.push(lowered.clone());
    }

    let underscored = lowered.replace([' ', '-'], "_");
    if !underscored.is_empty() && !candidates.iter().any(|existing| existing == &underscored) {
        candidates.push(underscored);
    }
}

pub(super) fn public_tool_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let raw = strip_wrapping_quotes(name);
    if raw.is_empty() {
        return candidates;
    }

    push_candidate(&mut candidates, raw);

    if let Some((lhs, rhs)) = raw.split_once("<|channel|>") {
        push_candidate(&mut candidates, rhs);
        push_candidate(&mut candidates, lhs);
    }

    if let Some((_, suffix)) = raw.rsplit_once(':') {
        push_candidate(&mut candidates, suffix);
    }

    candidates
}

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
        public_tool_name_candidates(name)
            .into_iter()
            .find_map(|candidate| assembly.resolve_registration_name(&candidate).ok())
            .map(str::to_owned)
            .ok_or_else(|| {
                ToolCallError::respond(format!("Unknown tool: {}", canonical_tool_name(name)))
            })
    }

    pub(crate) fn resolve_public_tool_entry(
        &self,
        name: &str,
    ) -> Option<(String, crate::tool_policy::ToolPolicy)> {
        let assembly = self
            .tool_assembly
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        public_tool_name_candidates(name)
            .into_iter()
            .find_map(|candidate| {
                assembly.find_catalog_entry(&candidate).map(|entry| {
                    (
                        entry.registration_name.clone(),
                        entry.default_permission.clone(),
                    )
                })
            })
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
    use super::public_tool_name_candidates;

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
