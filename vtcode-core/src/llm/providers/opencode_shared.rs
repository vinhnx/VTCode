#![allow(clippy::result_large_err)]

use crate::config::models::model_catalog_entry;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use super::common::{
    map_finish_reason_common, parse_response_openai_format, serialize_messages_openai_format,
};
use super::error_handling::handle_openai_http_error;

pub(crate) struct OpenCodeCompatibleProvider {
    provider_name: &'static str,
    provider_key: &'static str,
    api_key_env: &'static str,
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    supported_models: &'static [&'static str],
}

impl OpenCodeCompatibleProvider {
    pub(crate) fn new(
        provider_name: &'static str,
        provider_key: &'static str,
        api_key_env: &'static str,
        api_key: String,
        http_client: HttpClient,
        base_url: String,
        model: String,
        supported_models: &'static [&'static str],
    ) -> Self {
        Self {
            provider_name,
            provider_key,
            api_key_env,
            api_key,
            http_client,
            base_url,
            model,
            supported_models,
        }
    }

    fn requested_model<'a>(&'a self, model: &'a str) -> &'a str {
        if model.trim().is_empty() {
            &self.model
        } else {
            model
        }
    }

    fn convert_to_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::new();

        payload.insert("model".to_owned(), Value::String(request.model.clone()));
        payload.insert(
            "messages".to_owned(),
            Value::Array(serialize_messages_openai_format(
                request,
                self.provider_key,
            )?),
        );

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        if let Some(temperature) = request.temperature {
            payload.insert(
                "temperature".to_owned(),
                Value::Number(serde_json::Number::from_f64(temperature as f64).ok_or_else(
                    || LLMError::InvalidRequest {
                        message: "Invalid temperature value".to_string(),
                        metadata: None,
                    },
                )?),
            );
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = super::common::serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_string(),
                choice.to_provider_format(self.provider_key),
            );
        }

        Ok(Value::Object(payload))
    }
}

#[async_trait]
impl LLMProvider for OpenCodeCompatibleProvider {
    fn name(&self) -> &str {
        self.provider_key
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.reasoning)
            .unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.tool_call)
            .unwrap_or(true)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.structured_output)
            .unwrap_or(false)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.caching)
            .unwrap_or(false)
    }

    fn supports_vision(&self, model: &str) -> bool {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.vision)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        model_catalog_entry(self.provider_key, self.requested_model(model))
            .map(|entry| entry.context_window)
            .filter(|value| *value > 0)
            .unwrap_or(128_000)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.convert_to_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| {
                let formatted_error = error_display::format_llm_error(
                    self.provider_name,
                    &format!("Network error: {error}"),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let response =
            handle_openai_http_error(response, self.provider_name, self.api_key_env).await?;

        let response_json: Value = response.json().await.map_err(|error| {
            let formatted_error = error_display::format_llm_error(
                self.provider_name,
                &format!("Failed to parse response: {error}"),
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

        parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            self.provider_name,
            model,
            false,
            None,
        )
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        self.validate_request(&request)?;
        request.stream = true;

        let payload = self.convert_to_format(&request)?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| {
                let formatted_error = error_display::format_llm_error(
                    self.provider_name,
                    &format!("Network error: {error}"),
                );
                LLMError::Network {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        let response =
            handle_openai_http_error(response, self.provider_name, self.api_key_env).await?;

        let bytes_stream = response.bytes_stream();
        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx = event_tx.clone();

        let model_clone = model.clone();
        let provider_name = self.provider_name;
        tokio::spawn(async move {
            let mut aggregator =
                crate::llm::providers::shared::StreamAggregator::new(model_clone.clone());

            let result = crate::llm::providers::shared::process_openai_stream(
                bytes_stream,
                provider_name,
                model_clone,
                |value| {
                    if let Some(choices) = value
                        .get("choices")
                        .and_then(|candidate| candidate.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta")
                            && let Some(content) = delta
                                .get("content")
                                .and_then(|candidate| candidate.as_str())
                        {
                            for event in aggregator.handle_content(content) {
                                let _ = tx.send(Ok(event));
                            }
                        }

                        if let Some(reason) = choice
                            .get("finish_reason")
                            .and_then(|candidate| candidate.as_str())
                        {
                            aggregator.set_finish_reason(map_finish_reason_common(reason));
                        }
                    }

                    if value.get("usage").is_some()
                        && let Some(usage) =
                            crate::llm::providers::common::parse_usage_openai_format(&value, false)
                    {
                        aggregator.set_usage(usage);
                    }
                    Ok(())
                },
            )
            .await;

            match result {
                Ok(_) => {
                    let response = aggregator.finalize();
                    let _ = tx.send(Ok(LLMStreamEvent::Completed {
                        response: Box::new(response),
                    }));
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                }
            }
        });

        let stream = try_stream! {
            let mut receiver = event_rx;
            while let Some(event) = receiver.recv().await {
                yield event?;
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        self.supported_models
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let supported_models = self
            .supported_models
            .iter()
            .map(|model| model.to_string())
            .collect::<Vec<_>>();

        super::common::validate_request_common(
            request,
            self.provider_name,
            self.provider_key,
            Some(&supported_models),
        )
    }
}
