//! Ask command implementation - single prompt without tools

use crate::config::models::ModelId;
use crate::config::types::AgentConfig;
use crate::gemini::models::SystemInstruction;
use crate::gemini::{Content, GenerateContentRequest};
use crate::llm::make_client;
use crate::prompts::{generate_lightweight_instruction, read_system_prompt_from_md};
use anyhow::Result;

/// Handle the ask command - single prompt without tools
pub async fn handle_ask_command(config: AgentConfig, prompt: Vec<String>) -> Result<()> {
    let model_id = config
        .model
        .parse::<ModelId>()
        .map_err(|_| anyhow::anyhow!("Invalid model: {}", config.model))?;
    let mut client = make_client(config.api_key.clone(), model_id);
    let prompt_text = prompt.join(" ");

    if config.verbose {
        println!("Sending prompt to {}: {}", config.model, prompt_text);
    }

    let contents = vec![Content::user_text(prompt_text)];
    let lightweight_instruction = generate_lightweight_instruction();

    // Convert Content to SystemInstruction
    let system_instruction = if let Some(part) = lightweight_instruction.parts.first() {
        if let Some(text) = part.as_text() {
            SystemInstruction::new(text)
        } else {
            SystemInstruction::new(
                read_system_prompt_from_md()
                    .unwrap_or_else(|_| "You are a helpful coding assistant.".to_string()),
            )
        }
    } else {
        SystemInstruction::new(
            read_system_prompt_from_md()
                .unwrap_or_else(|_| "You are a helpful coding assistant.".to_string()),
        )
    };

    let request = GenerateContentRequest {
        contents,
        tools: None,
        tool_config: None,
        generation_config: None,
        system_instruction: Some(system_instruction),
        reasoning_config: None,
    };

    // Convert the request to a string prompt
    let prompt = request
        .contents
        .iter()
        .map(|content| {
            content
                .parts
                .iter()
                .map(|part| match part {
                    crate::gemini::Part::Text { text } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let response = client.generate(&prompt).await?;

    // Print the response content directly
    if let Some(content) = response.content {
        println!("{}", content);
    }

    Ok(())
}
