use anyhow::{Context, Result};
use console::style;
use vtcode_core::{
    config::types::AgentConfig as CoreAgentConfig,
    llm::{
        factory::create_provider_with_config,
        make_client,
        provider::{LLMRequest, Message, ToolChoice},
    },
    models::ModelId,
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

    if config.provider.trim().eq_ignore_ascii_case("openrouter") {
        let provider = create_provider_with_config(
            "openrouter",
            Some(config.api_key.clone()),
            None,
            Some(config.model.clone()),
        )
        .context("Failed to initialize OpenRouter provider")?;

        let request = LLMRequest {
            messages: vec![Message::user(prompt.to_string())],
            system_prompt: None,
            tools: None,
            model: config.model.clone(),
            max_tokens: None,
            temperature: None,
            stream: false,
            tool_choice: Some(ToolChoice::none()),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort: None,
        };

        let response = provider
            .generate(request)
            .await
            .context("OpenRouter request failed")?;
        println!("{}", response.content.unwrap_or_default());
        return Ok(());
    }

    let model_id: ModelId = config.model.parse()?;

    let mut client = make_client(config.api_key.clone(), model_id);
    let resp = client.generate(prompt).await?;
    println!("{}", resp.content);

    Ok(())
}
