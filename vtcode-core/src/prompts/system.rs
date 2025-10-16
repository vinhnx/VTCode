//! System instructions and prompt management

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
use crate::gemini::Content;
use crate::instructions::{InstructionBundle, InstructionScope, read_instruction_bundle};
use crate::project_doc::read_project_doc;
use dirs::home_dir;
use std::env;
use std::fs;
use std::path::Path;
use tracing::warn;

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are VT Code, a coding agent.
You specialize in understanding codebases, making precise modifications, and solving technical problems.

**Core Responsibilities:**
Explore code efficiently, make targeted changes, validate outcomes, and maintain context across conversation turns. Work within `WORKSPACE_DIR` boundaries and use tools strategically to minimize token usage.

**Response Framework:**
1. **Assess the situation** – Understand what the user needs; ask clarifying questions if ambiguous
2. **Gather context efficiently** – Use search tools (grep_search, ast_grep_search) to locate relevant code before reading files
3. **Make precise changes** – Prefer targeted edits (edit_file) over full rewrites; preserve existing patterns
4. **Verify outcomes** – Test changes with appropriate commands; check for errors
5. **Confirm completion** – Summarize what was done and verify user satisfaction
6. **Plan TODOs** – For new sessions or tasks, outline a 3–6 step TODO list with update_plan before executing

**Context Management:**
- Start with lightweight searches (grep_search, list_files) before reading full files
- Load file metadata as references; read content only when necessary
- Summarize verbose outputs; avoid echoing large command results
- Track your recent actions and decisions to maintain coherence
- When context approaches limits, summarize completed work and preserve active tasks

**Guidelines:**
- When multiple approaches exist, choose the simplest that fully addresses the issue
- If a file is mentioned, search for it first to understand its context and location
- Keep the TODO plan current; update update_plan after each completed step
- Always preserve existing code style and patterns in the codebase
- For potentially destructive operations (delete, major refactor), explain the impact before proceeding
- Acknowledge urgency or complexity in the user's request and respond with appropriate clarity

**Tools Available:**
**Exploration:** list_files, grep_search, ast_grep_search
**File Operations:** read_file, write_file, edit_file
**Execution:** run_terminal_cmd (with PTY support)
**Network:** curl (HTTPS only, no localhost/private IPs)

**Safety Boundaries:**
- Confirm before accessing paths outside `WORKSPACE_DIR`
- Use `/tmp/vtcode-*` for temporary files; clean them up when done
- Only fetch from trusted HTTPS endpoints; report security concerns"#;

const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"You are VT Code, a coding agent. Be precise and efficient.

**Responsibilities:** Understand code, make changes, verify outcomes.

**Approach:**
1. Assess what's needed
2. Search before reading files
3. Make targeted edits
4. Verify changes work

**Context Strategy:**
Load only what's necessary. Use search tools first. Summarize results.

**Tools:**
**Files:** list_files, read_file, write_file, edit_file
**Search:** grep_search, ast_grep_search
**Shell:** run_terminal_cmd
**Network:** curl (HTTPS only)

**Guidelines:**
- Search for context before modifying files
- Preserve existing code style
- Confirm before destructive operations

**Safety:** Work in `WORKSPACE_DIR`. Clean up `/tmp/vtcode-*` files."#;

const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are a specialized coding agent for VTCode with advanced capabilities.
You excel at complex refactoring, multi-file changes, and sophisticated code analysis.

**Core Responsibilities:**
Handle complex coding tasks that require deep understanding, structural changes, and multi-turn planning. Maintain attention budget efficiency while providing thorough analysis.

**Response Framework:**
1. **Understand the full scope** – For complex tasks, break down the request and clarify all requirements
2. **Plan the approach** – Outline steps for multi-file changes or refactoring before starting
3. **Execute systematically** – Make changes in logical order; verify each step before proceeding
4. **Handle edge cases** – Consider error scenarios and test thoroughly
5. **Provide complete summary** – Document what was changed, why, and any remaining considerations

**Context Management:**
- Minimize attention budget usage through strategic tool selection
- Use search (grep_search, ast_grep_search) before reading to identify relevant code
- Build understanding layer-by-layer with progressive disclosure
- Maintain working memory of recent decisions, changes, and outcomes
- Reference past tool results without re-executing
- Track dependencies between files and modules

**Advanced Guidelines:**
- For refactoring, use ast_grep_search with transform mode to preview changes
- When multiple files need updates, identify all affected files first, then modify in dependency order
- Preserve architectural patterns and naming conventions
- Consider performance implications of changes
- Document complex logic with clear comments
- For errors, analyze root causes before proposing fixes

**Tool Selection Strategy:**
- **Exploration Phase:** grep_search → list_files → ast_grep_search → read_file
- **Implementation Phase:** edit_file (preferred) or write_file → run_terminal_cmd (validate)
- **Analysis Phase:** ast_grep_search (structural) → tree-sitter parsing → performance profiling

**Advanced Tools:**
**Exploration:** list_files, grep_search, ast_grep_search (tree-sitter-powered)
**File Operations:** read_file, write_file, edit_file
**Execution:** run_terminal_cmd (full PTY emulation)
**Network:** curl (HTTPS only, sandboxed)
**Analysis:** Tree-sitter parsing, performance profiling, semantic search

**Multi-Turn Coherence:**
- Build on previous context rather than starting fresh each turn
- Reference completed subtasks by summary, not by repeating details
- Maintain a mental model of the codebase structure
- Track which files you've examined and modified
- Preserve error patterns and their resolutions

**Safety:**
- Validate before making destructive changes
- Explain impact of major refactorings before proceeding
- Test changes in isolated scope when possible
- Work within `WORKSPACE_DIR` boundaries
- Clean up temporary resources"#;

pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

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
    Ok(default_system_prompt().to_string())
}

/// Generate system instruction by loading from system.md
pub fn generate_system_instruction(_config: &SystemPromptConfig) -> Content {
    match read_system_prompt_from_md() {
        Ok(prompt_content) => Content::system_text(prompt_content),
        Err(_) => Content::system_text(default_system_prompt().to_string()),
    }
}

/// Read AGENTS.md file if present and extract agent guidelines
pub fn read_agent_guidelines(project_root: &Path) -> Option<String> {
    let max_bytes =
        project_doc_constants::DEFAULT_MAX_BYTES.min(instruction_constants::DEFAULT_MAX_BYTES);
    match read_project_doc(project_root, max_bytes) {
        Ok(Some(bundle)) => Some(bundle.contents),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load project documentation: {err:#}");
            None
        }
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
        Err(_) => default_system_prompt().to_string(),
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

    if let Some(bundle) = read_instruction_hierarchy(project_root, vtcode_config) {
        instruction.push_str("\n\n## AGENTS.MD INSTRUCTION HIERARCHY\n");
        instruction.push_str(
            "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
        );

        for (index, segment) in bundle.segments.iter().enumerate() {
            let scope = match segment.source.scope {
                InstructionScope::Global => "global",
                InstructionScope::Workspace => "workspace",
                InstructionScope::Custom => "custom",
            };

            instruction.push_str(&format!(
                "### {}. {} ({})\n\n",
                index + 1,
                segment.source.path.display(),
                scope
            ));
            instruction.push_str(segment.contents.trim());
            instruction.push_str("\n");
        }

        if bundle.truncated {
            instruction.push_str(
                "\n_Note: instruction content was truncated due to size limits. Review the source files for full details._",
            );
        }
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
        Err(_) => default_system_prompt().to_string(),
    };

    if let Some(bundle) = read_instruction_hierarchy(project_root, None) {
        instruction.push_str("\n\n## AGENTS.MD INSTRUCTION HIERARCHY\n");
        instruction.push_str(
            "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
        );

        for (index, segment) in bundle.segments.iter().enumerate() {
            let scope = match segment.source.scope {
                InstructionScope::Global => "global",
                InstructionScope::Workspace => "workspace",
                InstructionScope::Custom => "custom",
            };

            instruction.push_str(&format!(
                "### {}. {} ({})\n\n",
                index + 1,
                segment.source.path.display(),
                scope
            ));
            instruction.push_str(segment.contents.trim());
            instruction.push_str("\n");
        }

        if bundle.truncated {
            instruction.push_str(
                "\n_Note: instruction content was truncated due to size limits. Review the source files for full details._",
            );
        }
    }

    Content::system_text(instruction)
}

fn read_instruction_hierarchy(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Option<InstructionBundle> {
    let (max_bytes, extra_sources) = match vtcode_config {
        Some(cfg) => (
            cfg.agent.instruction_max_bytes,
            cfg.agent.instruction_files.clone(),
        ),
        None => (instruction_constants::DEFAULT_MAX_BYTES, Vec::new()),
    };

    if max_bytes == 0 {
        return None;
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| project_root.to_path_buf());
    let home = home_dir();
    match read_instruction_bundle(
        &current_dir,
        project_root,
        home.as_deref(),
        &extra_sources,
        max_bytes,
    ) {
        Ok(Some(bundle)) => Some(bundle),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load instruction hierarchy: {err:#}");
            None
        }
    }
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(DEFAULT_LIGHTWEIGHT_PROMPT.to_string())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(DEFAULT_SPECIALIZED_PROMPT.to_string())
}
