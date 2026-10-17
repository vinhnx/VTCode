//! Standalone compaction transport and canonical replay windows.

use super::OpenResponsesProvider;
use crate::error_display;
use crate::provider::{LLMError, LLMRequest, Message, ResponsesCompactionOptions};
use crate::providers::error_handling::{format_network_error, format_parse_error};
use crate::providers::shared::parse_compacted_output_messages;
use anyhow::Result;
use serde_json::{Value, json};

impl OpenResponsesProvider {
    pub(crate) async fn compact_history_request(
        &self,
        model: &str,
        history: &[Message],
    ) -> Result<Vec<Message>, LLMError> {
        self.compact_history_request_with_options(model, history, &ResponsesCompactionOptions::default())
            .await
    }

    pub(crate) async fn compact_history_request_with_options(
        &self,
        model: &str,
        history: &[Message],
        options: &ResponsesCompactionOptions,
    ) -> Result<Vec<Message>, LLMError> {
        let resolved_model = if model.trim().is_empty() {
            self.model.clone()
        } else {
            model.trim().to_string()
        };
        let request = LLMRequest {
            model: resolved_model.clone(),
            messages: std::sync::Arc::new(history.to_vec()),
            ..Default::default()
        };
        let native_payload = self.build_native_payload(&request, false)?;
        let input = native_payload.get("input").cloned().unwrap_or_else(|| json!([]));
        let mut compact_payload = json!({
            "model": resolved_model,
            "input": input,
        });
        if let Some(map) = compact_payload.as_object_mut() {
            if let Some(instructions) = options.instructions.as_deref().map(str::trim).filter(|value| !value.is_empty())
            {
                map.insert("instructions".to_string(), json!(instructions));
            }
            if let Some(service_tier) = options.service_tier.as_deref().map(str::trim).filter(|value| !value.is_empty())
            {
                map.insert("service_tier".to_string(), json!(service_tier));
            }
            if let Some(prompt_cache_key) = options
                .prompt_cache_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                map.insert("prompt_cache_key".to_string(), json!(prompt_cache_key));
            }
        }

        let response = self
            .http_client
            .post(self.responses_compact_url())
            .bearer_auth(&self.api_key)
            .json(&compact_payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error(
                "OpenResponses",
                &format!("Compaction endpoint error (HTTP {status}): {body}"),
            );
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let json: Value = response.json().await.map_err(|e| format_parse_error("OpenResponses", &e))?;
        let output = json
            .get("output")
            .and_then(|value| value.as_array())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from OpenResponses compact endpoint: missing output array".to_string(),
                metadata: None,
            })?;

        let compacted = parse_compacted_output_messages(output);
        if compacted.is_empty() {
            return Err(LLMError::Provider {
                message: "Compaction response contained no reusable messages".to_string(),
                metadata: None,
            });
        }

        Ok(compacted)
    }
}
