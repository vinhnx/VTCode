//! Bounded, cache-key-scoped comparison evidence independent of replay lineage.

use super::OpenAIProvider;
use crate::provider::{LLMRequest, LLMResponse};
use compact_str::CompactString;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(super) struct CacheComparison {
    cache_key: Option<CompactString>,
    response_id: CompactString,
}

pub(super) struct CacheComparisonTracker {
    state: Arc<Mutex<Option<CacheComparison>>>,
    cache_key: Option<CompactString>,
}

impl OpenAIProvider {
    pub(super) fn cache_comparison_tracker(&self, request: &LLMRequest) -> Option<CacheComparisonTracker> {
        let model = if request.model.trim().is_empty() {
            self.model.as_ref()
        } else {
            &request.model
        };
        (self.is_native_openai_api()
            && self.prompt_cache_enabled
            && self.prompt_cache_settings.surface_metrics
            && super::super::request_builder::supports_prompt_cache_options(model))
        .then(|| CacheComparisonTracker {
            state: Arc::clone(&self.cache_comparison),
            cache_key: request
                .prompt_cache_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(CompactString::new),
        })
    }
}

impl CacheComparisonTracker {
    pub(super) fn add_comparison(&self, payload: &mut Value) {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(comparison) = state.as_ref().filter(|comparison| comparison.cache_key == self.cache_key) {
            if !payload.get("prompt_cache_options").is_some_and(Value::is_object) {
                payload["prompt_cache_options"] = json!({});
            }
            payload["prompt_cache_options"]["comparison_response_id"] = json!(comparison.response_id.as_str());
        }
    }

    /// Called only for successful foreground completions, never auxiliary
    /// summaries, compaction, transport warmups, failures, or cancelled streams.
    pub(super) fn record_completion(&self, response: &LLMResponse) {
        if let Some(response_id) = response.request_id.as_deref().filter(|id| {
            id.starts_with("resp_")
                && id.len() <= 256
                && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }) {
            *self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CacheComparison {
                cache_key: self.cache_key.clone(),
                response_id: CompactString::new(response_id),
            });
        }
    }
}

#[cfg(test)]
mod tests;
