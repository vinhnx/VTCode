use anyhow::{Context, Result};
use futures::StreamExt;
use serde_json::{Map, Value, json};
use std::io::{self, Write};
use vtcode_core::utils::colors::style;
use vtcode_core::{
    cli::args::AskOutputFormat,
    config::types::AgentConfig as CoreAgentConfig,
    llm::{
        factory::{create_provider_for_model, create_provider_with_config, ProviderConfig},
        provider::{
            FinishReason, LLMRequest, LLMResponse, LLMStreamEvent, Message, ToolChoice, Usage,
        },
    },
    tools::{ToolRegistry, ToolPermissionPolicy},
};
use crate::cli::AskCommandOptions;

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
        // Surface reasoning traces for providers that return them (e.g., OpenAI Responses API, Gemini reasoning).
        if let Some(reasoning) = response
            .reasoning
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if printed_any {
                // Ensure separation from previously streamed tokens.
                eprintln!();
            }
            eprintln!("--- reasoning ---");
            eprintln!("{reasoning}");
            eprintln!("-----------------");
        }

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
    prompt_arg: Option<String>,
    options: AskCommandOptions,
) -> Result<()> {
    let prompt = resolve_prompt(prompt_arg, config.quiet)?;
    tokio::select! {
        res = handle_ask_command_impl(config, &prompt, options) => res,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("{}", style("\nCancelled by user.").red());
            // Standard generic error (1) or specialized exit code logic would go here,
            // but for now we bail which results in non-zero exit from main.
            anyhow::bail!("Operation cancelled");
        }
    }
}

async fn handle_ask_command_impl(
    config: &CoreAgentConfig,
    prompt: &str,
    options: AskCommandOptions,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("No prompt provided. Use: vtcode ask \"Your question here\"");
    }

    let wants_json = options.wants_json();
    if !wants_json && !config.quiet {
        eprintln!("{}", style("[ASK]").cyan().bold());
        eprintln!("  {:16} {}", "provider", &config.provider);
        eprintln!("  {:16} {}\n", "model", &config.model);
    }

    // Check if we should run with tools or without tools
    let has_tool_permissions = !options.allowed_tools.is_empty() || options.skip_confirmations;

    if has_tool_permissions {
        // Run with tools using AgentRunner
        run_ask_with_tools(config, prompt, options).await
    } else {
        // Run without tools using simple LLM request
        run_ask_without_tools(config, prompt, options).await
    }
}

async fn run_ask_without_tools(
    config: &CoreAgentConfig,
    prompt: &str,
    options: AskCommandOptions,
) -> Result<()> {
    let wants_json = options.wants_json();

    let provider = match create_provider_for_model(
        &config.model,
        config.api_key.clone(),
        Some(config.prompt_cache.clone()),
        Some(config.model_behavior.clone()),
    ) {
        Ok(provider) => provider,
        Err(_) => create_provider_with_config(
            &config.provider,
            ProviderConfig {
                api_key: Some(config.api_key.clone()),
                base_url: None,
                model: Some(config.model.clone()),
                prompt_cache: Some(config.prompt_cache.clone()),
                timeouts: None,
                anthropic: None,
                model_behavior: Some(config.model_behavior.clone()),
            },
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
        model: config.model.clone(),
        stream: matches!(request_mode, AskRequestMode::Streaming),
        tool_choice: Some(ToolChoice::none()),
        reasoning_effort,
        ..Default::default()
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
                                eprintln!();
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
                                eprint!("");
                                printed_reasoning = true;
                                reasoning_line_finished = false;
                            }
                            eprint!("{}", delta);
                            io::stderr().flush().ok();
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
                    eprintln!();
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

async fn run_ask_with_tools(
    config: &CoreAgentConfig,
    prompt: &str,
    options: AskCommandOptions,
) -> Result<()> {
    use vtcode_core::core::agent::runner::{AgentRunner, Task, ContextItem};
    use vtcode_core::config::VTCodeConfig;
    use vtcode_core::config::loader::ConfigManager;

    // Load the full configuration for the agent runner
    let full_config = ConfigManager::load_from_workspace(&config.workspace)?
        .config()
        .clone();

    // Create the agent runner
    let mut runner = AgentRunner::new(
        config.clone(),
        full_config,
        config.workspace.clone(),
    ).await?;

    // Apply tool permissions based on CLI options
    if options.skip_confirmations {
        // If skipping confirmations, apply allowed tools but filter out disallowed ones
        let mut effective_allowed_tools = options.allowed_tools.clone();
        if !options.disallowed_tools.is_empty() {
            effective_allowed_tools.retain(|tool| !options.disallowed_tools.contains(tool));
        }
        runner.enable_full_auto(&effective_allowed_tools).await?;
    } else if !options.allowed_tools.is_empty() {
        // If specific allowed tools are provided, filter out disallowed ones
        let mut effective_allowed_tools = options.allowed_tools.clone();
        if !options.disallowed_tools.is_empty() {
            effective_allowed_tools.retain(|tool| !options.disallowed_tools.contains(tool));
        }
        runner.enable_full_auto(&effective_allowed_tools).await?;
    }
    // Note: If only disallowed tools are specified without allowed tools,
    // the default behavior (with confirmations) will apply, which is appropriate

    // Create a single task for the prompt
    let task_id = format!(
        "ask-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("System clock is before UNIX_EPOCH while creating ask task id")?
            .as_secs()
    );
    let task = Task {
        id: task_id,
        title: "CLI Ask Task".to_string(),
        description: prompt.to_string(),
        instructions: None,
    };

    // Execute the task
    let result = runner.execute_task(&task, &[]).await?;

    // Output the result based on format preference
    if options.wants_json() {
        // Emit structured JSON response
        let mut payload = Map::new();
        payload.insert("provider".to_string(), json!(config.provider));
        payload.insert("model".to_string(), json!(config.model));
        payload.insert("content".to_string(), json!(result.summary));
        payload.insert("finish_reason".to_string(), json!("stop"));
        payload.insert("turns_executed".to_string(), json!(result.turns_executed));
        payload.insert("outcome".to_string(), json!(result.outcome.code()));
        payload.insert("modified_files".to_string(), json!(result.modified_files));
        payload.insert("executed_commands".to_string(), json!(result.executed_commands));

        let serialized = serde_json::to_string_pretty(&Value::Object(payload))
            .context("failed to serialize ask response as JSON")?;
        println!("{serialized}");
    } else {
        // Print the summary response
        println!("{}", result.summary);
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

    if let Some(usage) = &response.usage {
        payload.insert("usage".to_string(), usage_to_json(usage));
    }

    if let Some(tool_calls) = &response.tool_calls {
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

fn resolve_prompt(prompt_arg: Option<String>, quiet: bool) -> Result<String> {
    match prompt_arg {
        Some(p) if p != "-" => Ok(p),
        maybe_dash => {
            use vtcode_core::utils::tty::TtyExt;

            let force_stdin = matches!(maybe_dash.as_deref(), Some("-"));
            if io::stdin().is_tty_ext() && !force_stdin {
                anyhow::bail!(
                    "No prompt provided. Pass a prompt argument, pipe input, or use '-' to read from stdin."
                );
            }
            if !force_stdin && !quiet {
                eprintln!("Reading prompt from stdin...");
            }
            let mut buffer = String::with_capacity(1024);
            io::stdin()
                .read_to_string(&mut buffer)
                .context("Failed to read prompt from stdin")?;
            if buffer.trim().is_empty() {
                anyhow::bail!("No prompt provided via stdin.");
            }
            Ok(buffer)
        }
    }
}
