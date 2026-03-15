//! Ask command implementation - single prompt without tools

use crate::cli::input_hardening::validate_agent_safe_text;
use crate::config::types::AgentConfig;
use crate::llm::factory::{ProviderConfig, create_provider_with_config, infer_provider_from_model};
use crate::llm::provider::{LLMRequest, Message};
use crate::prompts::system::lightweight_instruction_text;
use anyhow::Result;
use crossterm::tty::IsTty;
use std::sync::Arc;

/// Handle the ask command - single prompt without tools
pub async fn handle_ask_command(
    config: AgentConfig,
    prompt: Vec<String>,
    options: crate::cli::AskCommandOptions,
) -> Result<()> {
    let prompt_text = prompt.join(" ");
    validate_agent_safe_text("prompt", &prompt_text)?;

    if config.verbose {
        eprintln!("Sending prompt to {}: {}", config.model, prompt_text);
    }

    let request = LLMRequest {
        messages: vec![Message::user(prompt_text)],
        system_prompt: Some(Arc::new(lightweight_instruction_text())),
        model: config.model.clone(),
        ..Default::default()
    };
    let provider_name = if config.provider.trim().is_empty() {
        infer_provider_from_model(&request.model)
            .map(|provider| provider.to_string())
            .ok_or_else(|| anyhow::anyhow!("Cannot determine provider for model: {}", request.model))?
    } else {
        config.provider.to_lowercase()
    };
    let provider = create_provider_with_config(
        &provider_name,
        ProviderConfig {
            api_key: Some(config.api_key.clone()),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            base_url: None,
            model: Some(request.model.clone()),
            prompt_cache: None,
            timeouts: None,
            openai: None,
            anthropic: None,
            model_behavior: config.model_behavior.clone(),
        },
    )?;
    let backend_kind = provider.name().to_string();
    let response = provider.generate(request).await?;
    let response_model = if response.model.is_empty() {
        config.model.clone()
    } else {
        response.model.clone()
    };

    // Handle output based on format preference
    if let Some(crate::cli::args::AskOutputFormat::Json) = options.output_format {
        // Build a comprehensive JSON structure
        let output = serde_json::json!({
            "response": response,
            "provider": {
                "kind": backend_kind,
                "model": response_model,
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
