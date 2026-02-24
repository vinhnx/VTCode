//! Ask command implementation - single prompt without tools

use crate::config::models::ModelId;
use crate::config::types::AgentConfig;
use crate::gemini::models::SystemInstruction;
use crate::gemini::{Content, GenerateContentRequest};
use crate::llm::make_client;
use crate::prompts::{generate_lightweight_instruction, generate_system_instruction};
use anyhow::Result;
use crossterm::tty::IsTty;

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
        eprintln!("Sending prompt to {}: {}", config.model, prompt_text);
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
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &output)?;
        writeln!(stdout)?;
    } else {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        if is_pipe_output() {
            if let Some(code_only) = extract_code_only(response.content_text()) {
                write!(stdout, "{code_only}")?;
            } else {
                writeln!(stdout, "{}", response.content_text())?;
            }
        } else {
            // Print the response content directly (default behavior)
            writeln!(stdout, "{}", response.content_text())?;
        }
    }

    Ok(())
}

fn is_pipe_output() -> bool {
    !std::io::stdout().is_tty()
}

fn extract_code_only(text: &str) -> Option<String> {
    let blocks = extract_code_fence_blocks(text);
    let block = select_best_code_block(&blocks)?;
    let mut output = block.lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Some(output)
}

fn extract_code_fence_blocks(text: &str) -> Vec<CodeFenceBlock> {
    let mut blocks = Vec::new();
    let mut current_language: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for raw_line in text.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some(rest) = trimmed_start.strip_prefix("```") {
            let rest_clean = rest.trim_matches('\r');
            let rest_trimmed = rest_clean.trim();
            if current_language.is_some() {
                if rest_trimmed.is_empty() {
                    let language = current_language.take().and_then(|lang| {
                        let cleaned = lang.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
                        let cleaned = cleaned.trim();
                        if cleaned.is_empty() {
                            None
                        } else {
                            Some(cleaned.to_string())
                        }
                    });
                    let block_lines = std::mem::take(&mut current_lines);
                    blocks.push(CodeFenceBlock {
                        language,
                        lines: block_lines,
                    });
                    continue;
                }
            } else {
                let token = rest_trimmed.split_whitespace().next().unwrap_or_default();
                let normalized = token
                    .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'))
                    .trim();
                current_language = Some(normalized.to_ascii_lowercase());
                current_lines.clear();
                continue;
            }
        }

        if current_language.is_some() {
            current_lines.push(raw_line.trim_end_matches('\r').to_string());
        }
    }

    blocks
}

fn select_best_code_block(blocks: &[CodeFenceBlock]) -> Option<&CodeFenceBlock> {
    let mut best = None;
    let mut best_score = (0usize, 0u8);
    for block in blocks {
        let score = score_code_block(block);
        if score > best_score {
            best_score = score;
            best = Some(block);
        }
    }
    best
}

fn score_code_block(block: &CodeFenceBlock) -> (usize, u8) {
    let line_count = block
        .lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    let has_language = block
        .language
        .as_ref()
        .is_some_and(|lang| !lang.trim().is_empty());
    (line_count, if has_language { 1 } else { 0 })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeFenceBlock {
    language: Option<String>,
    lines: Vec<String>,
}
