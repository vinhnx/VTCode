use anyhow::{Context, Result};
use console::style;
use futures::StreamExt;
use std::io::{self, Write};
use vtcode_core::{
    config::types::AgentConfig as CoreAgentConfig,
    llm::{
        factory::{create_provider_for_model, create_provider_with_config},
        provider::{LLMRequest, LLMStreamEvent, Message, ToolChoice},
    },
};

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

    let request = LLMRequest {
        messages: vec![Message::user(prompt.to_string())],
        system_prompt: None,
        tools: None,
        model: config.model.clone(),
        max_tokens: None,
        temperature: None,
        stream: true,
        tool_choice: Some(ToolChoice::none()),
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
    };

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

    if let Some(response) = final_response {
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

    Ok(())
}
