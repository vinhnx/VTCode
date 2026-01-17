//! Ask command implementation - single prompt without tools

use crate::config::models::ModelId;
use crate::config::types::AgentConfig;
use crate::gemini::models::SystemInstruction;
use crate::gemini::{Content, GenerateContentRequest};
use crate::llm::make_client;
use crate::prompts::{generate_lightweight_instruction, generate_system_instruction};
use anyhow::Result;

/// Handle the ask command - single prompt without tools
pub async fn handle_ask_command(
    config: AgentConfig,
    prompt: Vec<String>,
    options: crate::cli::AskCommandOptions,
) -> Result<()> {
    let model_id = config
        .model
        .parse::<ModelId>()
        .map_err(|_| anyhow::anyhow!("Invalid model: {}", config.model))?;
    let mut client = make_client(config.api_key.clone(), model_id)?;
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
            let content = generate_system_instruction(&Default::default()).await;
            if let Some(text) = content.parts.first().and_then(|p| p.as_text()) {
                SystemInstruction::new(text)
            } else {
                SystemInstruction::new(crate::prompts::system::default_lightweight_prompt())
            }
        }
    } else {
        let content = generate_system_instruction(&Default::default()).await;
        if let Some(text) = content.parts.first().and_then(|p| p.as_text()) {
            SystemInstruction::new(text)
        } else {
            SystemInstruction::new(crate::prompts::system::default_lightweight_prompt())
        }
    };

    let request = GenerateContentRequest {
        contents,
        tools: None,
        tool_config: None,
        generation_config: None,
        system_instruction: Some(system_instruction),
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
                    crate::gemini::Part::Text { text, .. } => text.clone(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let response = client.generate(&prompt).await?;
    let backend_kind = client.backend_kind();

    // Handle output based on format preference
    if let Some(crate::cli::args::AskOutputFormat::Json) = options.output_format {
        // Build a comprehensive JSON structure
        let output = serde_json::json!({
            "response": response,
            "provider": {
                "kind": backend_kind,
                "model": response.model,
            }
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Print the response content directly (default behavior)
        println!("{}", response.content);
    }

    Ok(())
}
