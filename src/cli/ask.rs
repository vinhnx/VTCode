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

fn classify_request_mode(provider_name: &str, prompt: &str) -> AskRequestMode {
    let is_streaming_provider = provider_name.eq_ignore_ascii_case("gemini");
    let is_static_prompt = prompt
        .split_whitespace()
        .next()
        .map(|word| word.trim_matches(|ch: char| !ch.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(|word| word.eq_ignore_ascii_case("comment") || word.eq_ignore_ascii_case("comments"))
        .unwrap_or(false);

    if is_streaming_provider && !is_static_prompt {
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

    let provider = match create_provider_for_model(&config.model, config.api_key.clone()) {
        Ok(provider) => provider,
        Err(_) => create_provider_with_config(
            &config.provider,
            Some(config.api_key.clone()),
            None,
            Some(config.model.clone()),
        )
        .context("Failed to initialize provider for ask command")?,
    };

    let request_mode = classify_request_mode(provider.name(), prompt);
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
        reasoning_effort: None,
    };

    match request_mode {
        AskRequestMode::Streaming => {
            let mut stream = provider
                .stream(request)
                .await
                .context("Streaming completion failed")?;

            let mut printed_any = false;
            let mut final_response = None;

            while let Some(event) = stream.next().await {
                match event? {
                    LLMStreamEvent::Token { delta } => {
                        print!("{}", delta);
                        io::stdout().flush().ok();
                        printed_any = true;
                    }
                    LLMStreamEvent::Completed { response } => {
                        final_response = Some(response);
                    }
                }
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
