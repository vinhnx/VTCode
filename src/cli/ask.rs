use anyhow::{Context, Result};
use console::style;
use futures::StreamExt;
use serde_json::{Map, Value, json};
use std::io::{self, Write};
use vtcode_core::{
    cli::args::AskOutputFormat,
    config::types::AgentConfig as CoreAgentConfig,
    llm::{
        factory::{create_provider_for_model, create_provider_with_config},
        provider::{
            FinishReason, LLMRequest, LLMResponse, LLMStreamEvent, Message, ToolChoice, Usage,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AskRequestMode {
    Streaming,
    Static,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AskCommandOptions {
    pub output_format: Option<AskOutputFormat>,
}

impl AskCommandOptions {
    fn wants_json(&self) -> bool {
        matches!(self.output_format, Some(AskOutputFormat::Json))
    }
}

fn classify_request_mode(provider_supports_streaming: bool) -> AskRequestMode {
    if provider_supports_streaming {
        AskRequestMode::Streaming
    } else {
        AskRequestMode::Static
    }
}

fn print_final_response(printed_any: bool, response: Option<LLMResponse>) {
    if let Some(response) = response {
        match (printed_any, response.content) {
            (false, Some(content)) => println!("{}", content),
            (true, Some(content)) => {
                if !content.ends_with('\n') {
                    println!();
                }
            }
            (true, None) => println!(),
            (false, None) => {}
        }
    }
}

/// Handle the ask command - single prompt, no tools
pub async fn handle_ask_command(
    config: &CoreAgentConfig,
    prompt: &str,
    options: AskCommandOptions,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("No prompt provided. Use: vtcode ask \"Your question here\"");
    }

    let wants_json = options.wants_json();
    if !wants_json {
        println!("{}", style("Single Prompt Mode").blue().bold());
        println!("Provider: {}", &config.provider);
        println!("Model: {}", &config.model);
        println!();
    }

    let provider = match create_provider_for_model(
        &config.model,
        config.api_key.clone(),
        Some(config.prompt_cache.clone()),
    ) {
        Ok(provider) => provider,
        Err(_) => create_provider_with_config(
            &config.provider,
            Some(config.api_key.clone()),
            None,
            Some(config.model.clone()),
            Some(config.prompt_cache.clone()),
        )
        .context("Failed to initialize provider for ask command")?,
    };

    let request_mode = classify_request_mode(provider.supports_streaming());
    let reasoning_effort = if provider.supports_reasoning_effort(&config.model) {
        Some(config.reasoning_effort)
    } else {
        None
    };
    let request = LLMRequest {
        messages: vec![Message::user(prompt.to_string())],
        system_prompt: None,
        tools: None,
        model: config.model.clone(),
        max_tokens: None,
        temperature: None,
        stream: matches!(request_mode, AskRequestMode::Streaming),
        tool_choice: Some(ToolChoice::none()),
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort,
    };

    match request_mode {
        AskRequestMode::Streaming => {
            let mut stream = provider
                .stream(request)
                .await
                .context("Streaming completion failed")?;

            let mut printed_any = false;
            let mut final_response = None;
            let mut printed_reasoning = false;
            let mut reasoning_line_finished = true;
            let mut streamed_reasoning = String::new();

            while let Some(event) = stream.next().await {
                match event {
                    Ok(LLMStreamEvent::Token { delta }) => {
                        if wants_json {
                            // The final response carries the content; suppress streaming output.
                        } else {
                            if printed_reasoning && !reasoning_line_finished {
                                println!();
                                reasoning_line_finished = true;
                            }
                            print!("{}", delta);
                            io::stdout().flush().ok();
                            printed_any = true;
                        }
                    }
                    Ok(LLMStreamEvent::Reasoning { delta }) => {
                        if wants_json {
                            streamed_reasoning.push_str(&delta);
                        } else {
                            if !printed_reasoning {
                                print!("Thinking: ");
                                printed_reasoning = true;
                                reasoning_line_finished = false;
                            }
                            print!("{}", delta);
                            io::stdout().flush().ok();
                        }
                    }
                    Ok(LLMStreamEvent::Completed { response }) => {
                        final_response = Some(response);
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                }
            }

            let final_response =
                final_response.context("LLM stream ended without a completion response")?;

            if wants_json {
                let reasoning = if streamed_reasoning.trim().is_empty() {
                    None
                } else {
                    Some(streamed_reasoning)
                };
                emit_json_response(config, final_response, reasoning)?;
            } else {
                if printed_reasoning && !reasoning_line_finished {
                    println!();
                }

                print_final_response(printed_any, Some(final_response));
            }
        }
        AskRequestMode::Static => {
            let response = provider
                .generate(request)
                .await
                .context("Completion failed")?;

            if wants_json {
                emit_json_response(config, response, None)?;
            } else {
                print_final_response(false, Some(response));
            }
        }
    }

    Ok(())
}

fn emit_json_response(
    config: &CoreAgentConfig,
    response: LLMResponse,
    streamed_reasoning: Option<String>,
) -> Result<()> {
    let mut payload = Map::new();
    payload.insert("provider".to_string(), json!(config.provider));
    payload.insert("model".to_string(), json!(config.model));
    payload.insert(
        "content".to_string(),
        json!(response.content.unwrap_or_default()),
    );
    payload.insert(
        "finish_reason".to_string(),
        finish_reason_json(&response.finish_reason),
    );

    let reasoning = streamed_reasoning
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .or_else(|| {
            response.reasoning.clone().and_then(|value| {
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            })
        });

    if let Some(reasoning) = reasoning {
        payload.insert("reasoning".to_string(), json!(reasoning));
    }

    if let Some(usage) = response.usage.clone() {
        payload.insert("usage".to_string(), usage_to_json(&usage));
    }

    if let Some(tool_calls) = response.tool_calls.clone() {
        let tool_value =
            serde_json::to_value(tool_calls).context("failed to serialize tool calls")?;
        payload.insert("tool_calls".to_string(), tool_value);
    }

    let serialized = serde_json::to_string_pretty(&Value::Object(payload))
        .context("failed to serialize ask response as JSON")?;
    println!("{serialized}");
    Ok(())
}

fn usage_to_json(usage: &Usage) -> Value {
    json!({
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens,
        "cached_prompt_tokens": usage.cached_prompt_tokens,
        "cache_creation_tokens": usage.cache_creation_tokens,
        "cache_read_tokens": usage.cache_read_tokens,
    })
}

fn finish_reason_json(reason: &FinishReason) -> Value {
    match reason {
        FinishReason::Stop => json!({ "type": "stop" }),
        FinishReason::Length => json!({ "type": "length" }),
        FinishReason::ToolCalls => json!({ "type": "tool_calls" }),
        FinishReason::ContentFilter => json!({ "type": "content_filter" }),
        FinishReason::Error(message) => json!({ "type": "error", "message": message }),
    }
}
