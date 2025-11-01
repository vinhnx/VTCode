//! Compress context command implementation

use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::AgentConfig;
use crate::gemini::models::SystemInstruction;
use crate::gemini::{Content, FunctionResponse, GenerateContentRequest, Part};
use crate::llm::make_client;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;

/// Handle the compress-context command - demonstrate context compression
pub async fn handle_compress_context_command(
    config: AgentConfig,
    _input: Option<std::path::PathBuf>,
    _output: Option<std::path::PathBuf>,
) -> Result<()> {
    println!("{}", style("[CONTEXT] Compression Demo").cyan().bold());
    println!(
        "{}",
        style("Following Cognition's context engineering principles...").dim()
    );

    // Create a sample long conversation history to compress
    let sample_conversation = vec![
        Content::user_text("I want to create a Rust web application with user authentication"),
        Content::system_text(
            "I'll help you create a Rust web application with authentication. Let me start by exploring the current directory structure.",
        ),
        Content::user_parts(vec![Part::FunctionResponse {
            function_response: FunctionResponse {
                name: tools::LIST_FILES.to_string(),
                response: json!({"path": ".", "files": ["Cargo.toml", "src/main.rs"], "directories": ["src", "tests"]}),
            },
        }]),
        Content::system_text(
            "I can see you already have a basic Rust project. Let me check what's in the main.rs file.",
        ),
        Content::user_parts(vec![Part::FunctionResponse {
            function_response: FunctionResponse {
                name: tools::READ_FILE.to_string(),
                response: json!({"path": "src/main.rs", "content": "fn main() {\n    println!(\"Hello World!\");\n}", "metadata": {"size": 45}}),
            },
        }]),
        Content::system_text(
            "Now I need to add web framework dependencies. I'll update Cargo.toml to include Axum and other necessary crates.",
        ),
        Content::user_parts(vec![Part::FunctionResponse {
            function_response: FunctionResponse {
                name: tools::EDIT_FILE.to_string(),
                response: json!({"status": "modified", "path": "Cargo.toml", "action": {"replacements_made": 1}}),
            },
        }]),
        Content::system_text("Good! Now let me create the authentication module structure."),
        Content::user_parts(vec![Part::FunctionResponse {
            function_response: FunctionResponse {
                name: tools::WRITE_FILE.to_string(),
                response: json!({"status": "created", "path": "src/auth.rs", "bytes_written": 234}),
            },
        }]),
        Content::system_text("Now I'll create the main web server with authentication endpoints."),
        Content::user_parts(vec![Part::FunctionResponse {
            function_response: FunctionResponse {
                name: tools::EDIT_FILE.to_string(),
                response: json!({"status": "modified", "path": "src/main.rs", "action": {"replacements_made": 3}}),
            },
        }]),
    ];

    println!(
        "{} {}",
        style("Original conversation length:").yellow(),
        sample_conversation.len()
    );
    println!(
        "{} {:.1}KB",
        style("Estimated token usage:").yellow(),
        sample_conversation.len() as f64 * 0.5
    ); // Rough estimate

    // Create compression prompt following Cognition's principles
    let compression_prompt = r#"You are a context compression specialist. Your task is to compress the following agent conversation history while preserving:

1. KEY DECISIONS made by the agent
2. IMPORTANT ACTIONS taken (tool calls and their results)
3. CRITICAL CONTEXT about the current state
4. USER INTENT and requirements
5. TECHNICAL DECISIONS (frameworks, libraries, architecture choices)

IMPORTANT: Do NOT lose information about:
- What files were created/modified and why
- What dependencies were added
- What the current state of the project is
- What the user's original request was

Compress this conversation into a concise summary that captures all essential information:

ORIGINAL CONVERSATION:"#;

    // Build the conversation content for compression
    let mut compression_content = vec![Content::user_text(compression_prompt)];

    // Add each conversation turn
    for (i, content) in sample_conversation.iter().enumerate() {
        let role_indicator = match content.role.as_str() {
            "user" => "USER",
            "system" => "AGENT",
            _ => "UNKNOWN",
        };

        let mut content_summary = format!("\n--- Turn {} ({}) ---\n", i + 1, role_indicator);

        for part in &content.parts {
            if let Some(text) = part.as_text() {
                content_summary.push_str(text);
            } else if let Part::FunctionCall { function_call } = part {
                content_summary.push_str(&format!(
                    "\n[TOOL CALL: {}({})]",
                    function_call.name, function_call.args
                ));
            } else if let Part::FunctionResponse { function_response } = part {
                content_summary.push_str(&format!(
                    "\n[TOOL RESULT: {}]",
                    serde_json::to_string_pretty(&function_response.response).unwrap_or_default()
                ));
            }
        }

        if i == 0 {
            compression_content[0] =
                Content::user_text(format!("{}{}", compression_prompt, content_summary));
        } else {
            compression_content.push(Content::user_text(content_summary));
        }
    }

    // Add final instruction
    compression_content.push(Content::user_text(
        r#"
COMPRESSION REQUIREMENTS:
- Preserve all key decisions and their rationale
- Keep track of what files were created/modified
- Maintain information about current project state
- Include user's original intent
- Note any important technical choices made

COMPRESSED SUMMARY:"#,
    ));

    // Create request for compression
    let compression_request = GenerateContentRequest {
        contents: compression_content,
        tools: None,
        tool_config: None,
        generation_config: Some(json!({
            "maxOutputTokens": 1000,
            "temperature": 0.1
        })),
        system_instruction: Some(SystemInstruction::new(
            r#"You are an expert at compressing agent conversation history.
Your goal is to create a compressed summary that maintains all critical information while being concise.
Focus on: key decisions, actions taken, current state, and user requirements."#,
        )),
        reasoning_config: None,
    };

    let model_id = config
        .model
        .parse::<ModelId>()
        .map_err(|_| anyhow::anyhow!("Invalid model: {}", config.model))?;
    let mut client = make_client(config.api_key.clone(), model_id);
    println!("{}", style("Compressing conversation...").cyan());

    // Convert the request to a string prompt
    let _prompt = compression_request
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

    // Convert the compression request to a string prompt
    let prompt = compression_request
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

    let compressed_response = client.generate(&prompt).await?;

    // Print the compressed response content directly
    println!("{}", style("Compressed Summary:").green().bold());
    println!("{}", compressed_response.content);

    println!("\n{}", style(" Key Principles Applied:").yellow().bold());
    println!("  • {}", style("Share full context and traces").dim());
    println!("  • {}", style("Actions carry implicit decisions").dim());
    println!(
        "  • {}",
        style("Single-threaded agents are more reliable").dim()
    );
    println!(
        "  • {}",
        style("Context compression enables longer conversations").dim()
    );

    Ok(())
}
