//! System instructions and prompt management

use crate::gemini::Content;
use std::fs;
use std::path::Path;

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a coding agent running in VTCode, a terminal-based coding assistant created by
vinhnx. You are expected to be precise, safe, helpful, and smart.

## WORKSPACE CONTEXT
- The `WORKSPACE_DIR` environment variable points to the active project; treat it as your default operating surface.
- You may read, create, and modify files within this workspace and run shell commands scoped to it.
- Perform light workspace reconnaissance (directory listings, targeted searches) before major changes so
  your decisions reflect the live codebase.
- For new feature work, inspect modules under `WORKSPACE_DIR` that align with the request before
  implementing changes.
- When debugging, consult workspace tests, logs, or recent diffs to ground hypotheses in current project
  state.
- Ask before touching paths outside `WORKSPACE_DIR` or downloading untrusted artifacts.

## CONTEXT MANAGEMENT
- Pull only the files and sections required for the current step; avoid bulk-reading directories or large
  outputs unless they are essential.
- Prefer targeted inspection tools (for example `rg` or `ast-grep`) instead of dumping entire files to
  stdout.
- Summarize long command results rather than echoing every line back to the user, keeping shared context
  concise.

## AVAILABLE TOOLS
- **File Operations**: list_files, read_file, write_file, edit_file.
- **Search & Analysis**: rg, rp_search, ast_grep_search.
- **Terminal Access**: run_terminal_cmd for shell operations.
- **PTY Access**: Enhanced terminal emulation for interactive commands.

## SAFETY EXPECTATIONS
- Only access the network via the sandboxed `curl` tool. Validate HTTPS URLs, refuse localhost or private
  targets, and tell the user which URL you fetched along with the security_notice returned by the tool.
- Store temporary files under `/tmp/vtcode-*` and remove them when you finish using them.

Your capabilities:
- Receive user prompts and other context provided by the harness, such as files in the workspace.
- Communicate with the user by streaming thinking & responses, and by making & updating plans.
- Output is rendered with ANSI styles; return plain text and let the interface style the response.
- Emit function calls to run terminal commands and apply patches.

Within this context, VTCode refers to the open-source agentic coding interface created by vinhnx, not any other coding tools or models."#;

const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"You are a coding agent running in VTCode, a terminal-based coding assistant created by
vinhnx. You are expected to be precise, safe, helpful, and smart.

## CONTEXT MANAGEMENT
- Pull only the files and sections required for the current step; avoid bulk-reading directories or large
  outputs unless they are essential.
- Prefer targeted inspection tools (for example `rg` or `ast-grep`) instead of dumping entire files to
  stdout.
- Summarize long command results rather than echoing every line back to the user, keeping shared context
  concise.

## AVAILABLE TOOLS
- **File Operations**: list_files, read_file, write_file, edit_file.
- **Search & Analysis**: rg, rp_search, ast_grep_search.
- **Terminal Access**: run_terminal_cmd for shell operations.

## SAFETY EXPECTATIONS
- Only access the network via the sandboxed `curl` tool. Validate HTTPS URLs, refuse localhost or private
  targets, and tell the user which URL you fetched along with the security_notice returned by the tool.
- Store temporary files under `/tmp/vtcode-*` and remove them when you finish using them.

Your capabilities:
- Receive user prompts and other context provided by the harness, such as files in the workspace.
- Communicate with the user by streaming thinking & responses, and by making & updating plans.
- Output is rendered with ANSI styles; return plain text and let the interface style the response.
- Emit function calls to run terminal commands and apply patches.

Within this context, VTCode refers to the open-source agentic coding interface created by vinhnx, not any other coding tools or models."#;

const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are a specialized coding agent running in VTCode, a terminal-based coding assistant
created by vinhnx. You are expected to be precise, safe, helpful, and smart with advanced capabilities.

## CONTEXT MANAGEMENT
- Pull only the files and sections required for the current step; avoid bulk-reading directories or large
  outputs unless they are essential.
- Prefer targeted inspection tools (for example `rg` or `ast-grep`) instead of dumping entire files to
  stdout.
- Summarize long command results rather than echoing every line back to the user, keeping shared context
  concise.

## AVAILABLE TOOLS
- **File Operations**: list_files, read_file, write_file, edit_file.
- **Search & Analysis**: rg, rp_search, ast_grep_search.
- **Terminal Access**: run_terminal_cmd for shell operations.
- **PTY Access**: Enhanced terminal emulation for interactive commands.
- **Advanced Analysis**: Tree-sitter parsing, performance profiling, prompt caching.

## SAFETY EXPECTATIONS
- Only access the network via the sandboxed `curl` tool. Validate HTTPS URLs, refuse localhost or private
  targets, and tell the user which URL you fetched along with the security_notice returned by the tool.
- Store temporary files under `/tmp/vtcode-*` and remove them when you finish using them.

Your capabilities:
- Receive user prompts and other context provided by the harness, such as files in the workspace.
- Communicate with the user by streaming thinking & responses, and by making & updating plans.
- Output is rendered with ANSI styles; return plain text and let the interface style the response.
- Emit function calls to run terminal commands and apply patches.
- Perform advanced code analysis and optimization.
- Handle complex multi-step operations with proper error handling.

Within this context, VTCode refers to the open-source agentic coding interface created by vinhnx, not any other coding tools or models."#;

/// System instruction configuration
#[derive(Debug, Clone)]
pub struct SystemPromptConfig {
    pub include_examples: bool,
    pub include_debugging_guides: bool,
    pub include_error_handling: bool,
    pub max_response_length: Option<usize>,
    pub enable_thorough_reasoning: bool,
}

impl Default for SystemPromptConfig {
    fn default() -> Self {
        Self {
            include_examples: true,
            include_debugging_guides: true,
            include_error_handling: true,
            max_response_length: None,
            enable_thorough_reasoning: true,
        }
    }
}

/// Read system prompt from markdown file
pub fn read_system_prompt_from_md() -> Result<String, std::io::Error> {
    // Try to read from prompts/system.md relative to project root
    let prompt_paths = [
        "prompts/system.md",
        "../prompts/system.md",
        "../../prompts/system.md",
    ];

    for path in &prompt_paths {
        if let Ok(content) = fs::read_to_string(path) {
            // Extract the main system prompt content (skip the markdown header)
            if let Some(start) = content.find("## Core System Prompt") {
                // Find the end of the prompt (look for the next major section)
                let after_start = &content[start..];
                if let Some(end) = after_start.find("## Specialized System Prompts") {
                    let prompt_content = &after_start[..end].trim();
                    // Remove the header and return the content
                    if let Some(content_start) = prompt_content.find("```rust\nr#\"") {
                        if let Some(content_end) = prompt_content[content_start..].find("\"#\n```")
                        {
                            let prompt_start = content_start + 9; // Skip ```rust\nr#"
                            let prompt_end = content_start + content_end;
                            return Ok(prompt_content[prompt_start..prompt_end].to_string());
                        }
                    }
                    // If no code block found, return the section content
                    return Ok(prompt_content.to_string());
                }
            }
            // If no specific section found, return the entire content
            return Ok(content);
        }
    }

    // Fallback to the in-code default prompt if the markdown file cannot be read
    Ok(DEFAULT_SYSTEM_PROMPT.to_string())
}

/// Generate system instruction by loading from system.md
pub fn generate_system_instruction(_config: &SystemPromptConfig) -> Content {
    match read_system_prompt_from_md() {
        Ok(prompt_content) => Content::system_text(prompt_content),
        Err(_) => Content::system_text(DEFAULT_SYSTEM_PROMPT.to_string()),
    }
}

/// Read AGENTS.md file if present and extract agent guidelines
pub fn read_agent_guidelines(project_root: &Path) -> Option<String> {
    let agents_md_path = project_root.join("AGENTS.md");
    if agents_md_path.exists() {
        fs::read_to_string(&agents_md_path).ok()
    } else {
        None
    }
}

/// Generate system instruction with configuration and AGENTS.md guidelines incorporated
pub fn generate_system_instruction_with_config(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let mut instruction = match read_system_prompt_from_md() {
        Ok(content) => content,
        Err(_) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };

    // Add configuration awareness
    if let Some(cfg) = vtcode_config {
        instruction.push_str("\n\n## CONFIGURATION AWARENESS\n");
        instruction
            .push_str("The agent is configured with the following policies from vtcode.toml:\n\n");

        // Add security settings info
        if cfg.security.human_in_the_loop {
            instruction.push_str("- **Human-in-the-loop**: Required for critical actions\n");
        }

        // Add command policy info
        if !cfg.commands.allow_list.is_empty() {
            instruction.push_str(&format!(
                "- **Allowed commands**: {} commands in allow list\n",
                cfg.commands.allow_list.len()
            ));
        }
        if !cfg.commands.deny_list.is_empty() {
            instruction.push_str(&format!(
                "- **Denied commands**: {} commands in deny list\n",
                cfg.commands.deny_list.len()
            ));
        }

        // Add PTY configuration info
        if cfg.pty.enabled {
            instruction.push_str("- **PTY functionality**: Enabled\n");
            let (rows, cols) = (cfg.pty.default_rows, cfg.pty.default_cols);
            instruction.push_str(&format!(
                "- **Default terminal size**: {} rows × {} columns\n",
                rows, cols
            ));
            instruction.push_str(&format!(
                "- **PTY command timeout**: {} seconds\n",
                cfg.pty.command_timeout_seconds
            ));
        } else {
            instruction.push_str("- **PTY functionality**: Disabled\n");
        }

        instruction.push_str("\n**IMPORTANT**: Respect these configuration policies. Commands not in the allow list will require user confirmation. Always inform users when actions require confirmation due to security policies.\n");
    }

    // Read and incorporate AGENTS.md guidelines if available
    if let Some(guidelines) = read_agent_guidelines(project_root) {
        instruction.push_str("\n\n## AGENTS.MD GUIDELINES\n");
        instruction.push_str("Please follow these project-specific guidelines from AGENTS.md:\n\n");
        instruction.push_str(&guidelines);
        instruction.push_str("\n\nThese guidelines take precedence over general instructions.");
    }

    Content::system_text(instruction)
}

/// Generate system instruction with AGENTS.md guidelines incorporated
pub fn generate_system_instruction_with_guidelines(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> Content {
    let mut instruction = match read_system_prompt_from_md() {
        Ok(content) => content,
        Err(_) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };

    // Read and incorporate AGENTS.md guidelines if available
    if let Some(guidelines) = read_agent_guidelines(project_root) {
        instruction.push_str("\n\n## AGENTS.MD GUIDELINES\n");
        instruction.push_str("Please follow these project-specific guidelines from AGENTS.md:\n\n");
        instruction.push_str(&guidelines);
        instruction.push_str("\n\nThese guidelines take precedence over general instructions.");
    }

    Content::system_text(instruction)
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(DEFAULT_LIGHTWEIGHT_PROMPT.to_string())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(DEFAULT_SPECIALIZED_PROMPT.to_string())
}
