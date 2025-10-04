use anyhow::{Context, Result};
use console::style;
use futures::StreamExt;
use std::io::{self, Write};
use vtcode_core::{
    config::types::AgentConfig as CoreAgentConfig,
    llm::{
        factory::{create_provider_for_model, create_provider_with_config},
        provider::{LLMRequest, LLMResponse, LLMStreamEvent, Message, ToolChoice},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AskRequestMode {
    Streaming,
    Static,
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
pub async fn handle_ask_command(config: &CoreAgentConfig, prompt: &str) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("No prompt provided. Use: vtcode ask \"Your question here\"");
    }

    println!("{}", style("Single Prompt Mode").blue().bold());
    println!("Provider: {}", &config.provider);
    println!("Model: {}", &config.model);
    println!();

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

            while let Some(event) = stream.next().await {
                match event {
                    Ok(LLMStreamEvent::Token { delta }) => {
                        if printed_reasoning && !reasoning_line_finished {
                            println!();
                            reasoning_line_finished = true;
                        }
                        print!("{}", delta);
                        io::stdout().flush().ok();
                        printed_any = true;
                    }
                    Ok(LLMStreamEvent::Reasoning { delta }) => {
                        if !printed_reasoning {
                            print!("Thinking: ");
                            printed_reasoning = true;
                            reasoning_line_finished = false;
                        }
                        print!("{}", delta);
                        io::stdout().flush().ok();
                    }
                    Ok(LLMStreamEvent::Completed { response }) => {
                        final_response = Some(response);
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                }
            }

            if printed_reasoning && !reasoning_line_finished {
                println!();
            }

            print_final_response(printed_any, final_response);
        }
        AskRequestMode::Static => {
            let response = provider
                .generate(request)
                .await
                .context("Completion failed")?;

            print_final_response(false, Some(response));
        }
    }

    Ok(())
}
