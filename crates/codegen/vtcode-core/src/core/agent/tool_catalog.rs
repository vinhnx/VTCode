//! Tool catalog snapshot for session-scoped tool management.
//!
//! Tracks the current set of available tools, their names, and cache state
//! so the harness can efficiently filter and expose tools per turn.

use std::sync::Arc;

use crate::llm::provider::ToolDefinition;

/// Snapshot of the tool catalog for a single session turn.
#[derive(Debug, Clone)]
pub struct SessionToolCatalogSnapshot {
    pub version: u64,
    pub epoch: u64,
    pub planning_active: bool,
    pub request_user_input_enabled: bool,
    pub snapshot: Option<Arc<Vec<ToolDefinition>>>,
    pub active_tool_names: Arc<Vec<String>>,
    pub cache_hit: bool,
    pub tool_catalog_hash: Option<u64>,
}

impl SessionToolCatalogSnapshot {
    pub fn new(
        version: u64,
        epoch: u64,
        planning_active: bool,
        request_user_input_enabled: bool,
        snapshot: Option<Arc<Vec<ToolDefinition>>>,
        cache_hit: bool,
    ) -> Self {
        let active_tool_names = Arc::new(tool_names_from_definitions(snapshot.as_deref().map(Vec::as_slice)));
        Self::with_active_tool_names(
            version,
            epoch,
            planning_active,
            request_user_input_enabled,
            snapshot,
            active_tool_names,
            cache_hit,
        )
    }

    pub fn with_active_tool_names(
        version: u64,
        epoch: u64,
        planning_active: bool,
        request_user_input_enabled: bool,
        snapshot: Option<Arc<Vec<ToolDefinition>>>,
        active_tool_names: Arc<Vec<String>>,
        cache_hit: bool,
    ) -> Self {
        let tool_catalog_hash = super::harness_kernel::hash_tool_definitions(snapshot.as_deref().map(Vec::as_slice));
        Self {
            version,
            epoch,
            planning_active,
            request_user_input_enabled,
            snapshot,
            active_tool_names,
            cache_hit,
            tool_catalog_hash,
        }
    }

    #[must_use]
    pub fn available_tools(&self) -> usize {
        self.active_tool_names.len()
    }

    #[must_use]
    pub fn catalog_tools(&self) -> usize {
        self.snapshot.as_ref().map_or(0, |defs| defs.len())
    }

    #[must_use]
    pub fn has_tools(&self) -> bool {
        self.snapshot.is_some()
    }

    #[must_use]
    pub fn with_cache_hit(mut self, cache_hit: bool) -> Self {
        self.cache_hit = cache_hit;
        self
    }
}

fn tool_names_from_definitions(tools: Option<&[ToolDefinition]>) -> Vec<String> {
    let Some(tools) = tools else {
        return Vec::new();
    };

    tools.iter().map(|tool| tool.function_name().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::tool_names_from_definitions;
    use crate::llm::provider::ToolDefinition;

    #[test]
    fn borrowed_tool_slice_preserves_order_and_optional_empty_inputs() {
        let tools = [
            ToolDefinition::function("second".into(), "Second tool".into(), serde_json::json!({})),
            ToolDefinition::function("first".into(), "First tool".into(), serde_json::json!({})),
        ];
        assert_eq!(tool_names_from_definitions(Some(&tools)), ["second", "first"]);
        assert_eq!(tool_names_from_definitions(Some(&tools[1..])), ["first"]);
        assert!(tool_names_from_definitions(Some(&[])).is_empty());
        assert!(tool_names_from_definitions(None).is_empty());
    }
}
