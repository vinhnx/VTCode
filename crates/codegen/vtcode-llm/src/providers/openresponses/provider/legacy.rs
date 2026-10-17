//! Chat Completions serialization and fallback transport.

use super::OpenResponsesProvider;
use crate::error_display;
use crate::provider::{FinishReason, LLMError, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, ToolCall};
use crate::providers::common::{chat_completions_url, serialize_message_content_openai};
use crate::providers::error_handling::{format_network_error, format_parse_error};
use crate::providers::shared::Utf8StreamDecoder;
use anyhow::Result;
use async_stream::try_stream;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamEventWire<'a> {
    #[serde(borrow)]
    choices: Option<Vec<ChatCompletionChoiceWire<'a>>>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoiceWire<'a> {
    #[serde(borrow)]
    delta: Option<ChatCompletionDeltaWire<'a>>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionDeltaWire<'a> {
    #[serde(borrow)]
    content: Option<&'a str>,
    tool_calls: Option<Vec<Value>>,
}

impl OpenResponsesProvider {
    fn build_payload(&self, request: &LLMRequest, stream: bool) -> Result<Value, LLMError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system_prompt {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        for message in request.messages.iter() {
            let role = message.role.as_generic_str();
            let mut message_obj = json!({
                "role": role,
                "content": serialize_message_content_openai(&message.content)
            });

            if let Some(tool_calls) = &message.tool_calls {
                let tool_calls_json: Vec<Value> = tool_calls
                    .iter()
                    .filter_map(|tc| {
                        tc.function.as_ref().map(|f| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": f.name,
                                    "arguments": f.arguments
                                }
                            })
                        })
                    })
                    .collect();
                message_obj["tool_calls"] = json!(tool_calls_json);
            }

            if let Some(tool_call_id) = &message.tool_call_id {
                message_obj["tool_call_id"] = json!(tool_call_id);
            }

            messages.push(message_obj);
        }

        let mut payload = json!({
            "model": request.model,
            "messages": messages,
            "stream": stream
        });

        if let Some(max_tokens) = request.max_tokens {
            payload["max_tokens"] = json!(max_tokens);
        }

        if let Some(temp) = request.temperature {
            payload["temperature"] = json!(crate::providers::common::sampling_param_f64(temp));
        }

        if let Some(tools) = &request.tools {
            let tools_json: Vec<Value> = tools
                .iter()
                .filter_map(|t| {
                    t.function.as_ref().map(|f| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": f.name,
                                "description": f.description,
                                "parameters": f.parameters
                            }
                        })
                    })
                })
                .collect();
            payload["tools"] = json!(tools_json);
        }

        Ok(payload)
    }

    pub(super) async fn generate_fallback(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(&request, false)?;
        let url = chat_completions_url(&self.base_url);

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error("OpenResponses", &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let json: Value = response.json().await.map_err(|e| format_parse_error("OpenResponses", &e))?;

        let choice = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .ok_or_else(|| LLMError::Provider {
                message: "Invalid response from OpenResponses: missing choices".to_string(),
                metadata: None,
            })?;

        let message = choice.get("message").ok_or_else(|| LLMError::Provider {
            message: "Invalid response from OpenResponses: missing message".to_string(),
            metadata: None,
        })?;

        let content = message.get("content").and_then(|c| c.as_str()).map(|s| s.to_string());

        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id").and_then(|v| v.as_str())?;
                        let function = call.get("function")?;
                        let namespace = call
                            .get("namespace")
                            .and_then(|v| v.as_str())
                            .or_else(|| function.get("namespace").and_then(|v| v.as_str()))
                            .map(ToOwned::to_owned);
                        let name = function.get("name").and_then(|v| v.as_str())?;
                        let arguments = function.get("arguments").and_then(|v| v.as_str())?;
                        Some(ToolCall::function_with_namespace(
                            id.to_string(),
                            namespace,
                            name.to_string(),
                            arguments.to_string(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|calls| !calls.is_empty());

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|fr| fr.as_str())
            .map(|fr| match fr {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolCalls,
                other => FinishReason::Error(other.to_string()),
            })
            .unwrap_or(FinishReason::Stop);

        Ok(LLMResponse {
            content,
            tool_calls,
            model,
            usage: None,
            finish_reason,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
            organization_id: None,
            compaction: None,
        })
    }

    pub(super) async fn stream_fallback(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let model = request.model.clone();
        let payload = self.build_payload(&request, true)?;
        let url = chat_completions_url(&self.base_url);

        let response = self
            .http_client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_network_error("OpenResponses", &e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = crate::providers::common::read_provider_error_body(response).await;
            let formatted_error = error_display::format_llm_error("OpenResponses", &format!("HTTP {status}: {body}"));
            return Err(LLMError::Provider { message: formatted_error, metadata: None });
        }

        let stream = try_stream! {
            let mut body_stream = response.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            let mut offset = 0usize;
            let mut decoder = Utf8StreamDecoder::new();
            let mut aggregator = crate::providers::shared::StreamAggregator::new(model);

            while let Some(chunk_result) = body_stream.next().await {
                let chunk = chunk_result.map_err(|e| format_network_error("OpenResponses", &e))?;
                decoder.push_bytes(&chunk, &mut buf);

                while let Some(event) =
                    crate::providers::shared::next_sse_event(&buf, &mut offset).expect("valid utf-8 stream data")
                {

                    if let Some(data_payload) = crate::providers::shared::extract_data_payload(event) {
                        let trimmed = data_payload.trim();
                        if trimmed.is_empty() || trimmed == "[DONE]" {
                            continue;
                        }

                        if let Ok(payload) = serde_json::from_str::<ChatCompletionStreamEventWire<'_>>(trimmed)
                            && let Some(delta) = payload.choices.and_then(|choices| choices.into_iter().next()).and_then(|choice| choice.delta)
                        {
                            if let Some(content) = delta.content {
                                for ev in aggregator.handle_content(content) {
                                    yield ev;
                                }
                            }

                            if let Some(tool_calls) = delta.tool_calls.as_deref() {
                                aggregator.handle_tool_calls(tool_calls);
                            }
                        }
                    }
                }

                // Keep `buf` bounded to the unprocessed tail rather than
                // growing for the entire stream.
                crate::providers::shared::drain_consumed_sse(&mut buf, &mut offset);
            }

            yield LLMStreamEvent::Completed { response: Box::new(aggregator.finalize()) };
        };

        Ok(Box::pin(stream))
    }
}
