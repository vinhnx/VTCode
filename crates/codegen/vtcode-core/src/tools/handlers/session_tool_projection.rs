//! Cached, documentation-mode-specific projections of immutable tool entries.

use crate::config::ToolDocumentationMode;
use crate::tools::handlers::compact::{compact_parameters, compact_tool_description};
use crate::tools::handlers::session_tool_catalog::ToolCatalogEntry;
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;
use vtcode_commons::tokens::heuristic_tokens_from_byte_len;

#[derive(Debug)]
pub(super) struct ToolEntryProjection {
    description: String,
    parameters: Value,
    serialized_token_estimate: OnceLock<usize>,
}

impl ToolEntryProjection {
    pub(super) fn description(&self) -> &str {
        &self.description
    }

    pub(super) fn parameters(&self) -> &Value {
        &self.parameters
    }

    pub(super) fn serialized_token_estimate(&self, name: &str) -> usize {
        *self.serialized_token_estimate.get_or_init(|| {
            serde_json::to_string(&SerializedToolSchema {
                name,
                description: self.description.as_str(),
                parameters: &self.parameters,
            })
            .map(|serialized| heuristic_tokens_from_byte_len(serialized.len()))
            .unwrap_or(0)
        })
    }

    #[cfg(test)]
    pub(super) fn has_serialized_token_estimate(&self) -> bool {
        self.serialized_token_estimate.get().is_some()
    }
}

#[derive(Debug)]
pub(super) struct ToolProjectionCache {
    entries: Vec<[OnceLock<ToolEntryProjection>; 3]>,
}

impl ToolProjectionCache {
    pub(super) fn new(entry_count: usize) -> Self {
        Self {
            entries: (0..entry_count).map(|_| std::array::from_fn(|_| OnceLock::new())).collect(),
        }
    }

    pub(super) fn get_or_init(
        &self,
        entry_index: usize,
        entry: &ToolCatalogEntry,
        documentation_mode: ToolDocumentationMode,
    ) -> &ToolEntryProjection {
        self.entries[entry_index][documentation_mode_index(documentation_mode)].get_or_init(|| {
            let description =
                compact_tool_description(entry.description.as_str(), documentation_mode, entry.max_description_length);
            let parameters = compact_parameters(entry.parameters.clone(), documentation_mode);

            ToolEntryProjection {
                description,
                parameters,
                serialized_token_estimate: OnceLock::new(),
            }
        })
    }
}

#[derive(Debug, Serialize)]
struct SerializedToolSchema<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

fn documentation_mode_index(documentation_mode: ToolDocumentationMode) -> usize {
    match documentation_mode {
        ToolDocumentationMode::Minimal => 0,
        ToolDocumentationMode::Progressive => 1,
        ToolDocumentationMode::Full => 2,
    }
}
