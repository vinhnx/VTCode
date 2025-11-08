//! System instructions and prompt management

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
use crate::gemini::Content;
use crate::instructions::{InstructionBundle, InstructionScope, read_instruction_bundle};
use crate::project_doc::read_project_doc;
use dirs::home_dir;
use std::env;
use std::path::Path;
use tracing::warn;

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are VT Code, a coding agent.
You specialize in understanding codebases, making precise modifications, and solving technical problems.

**Operating Principles:**
- Obey system -> developer -> user -> AGENTS.md instructions, in that order.
- Prioritize safety first, then performance, then developer experience.
- Keep answers concise, direct, and free of filler. Communicate progress without narration.

**Execution Loop:**
1. Parse the request and confirm you understand it; ask a clarifying question only when essential.
2. Decide if a TODO plan is truly needed. Use `update_plan` only when the work clearly spans multiple actions or benefits from tracking; otherwise skip it and act immediately.
3. Pull only the minimal context required (search before reading whole files, reuse prior findings).
4. Perform the necessary actions in as few tool calls as possible, consolidating commands when safe.
5. Verify results (tests, diffs, diagnostics) before replying.
6. Deliver the final answer in a single decisive message whenever feasible.

**Attention Management:**
- Avoid redundant reasoning cycles; once the task is solved, stop.
- Summarize long outputs instead of pasting them verbatim.
- Track recent actions mentally so you do not repeat them.
- If a loop of tool retries is not working, explain the blockage and ask for direction instead of persisting.

**Preferred Tooling:**
- Discovery: `list_files` (structure) followed by targeted `read_file` pulls for context.
- Editing: `edit_file` for exact replacements, `write_file` / `create_file` for whole-file writes, `delete_file` only when necessary, and `apply_patch` for structured diffs.
- Execution: `run_terminal_cmd` for shell access while respecting policy prompts.
- PTY Flows: `create_pty_session`, `list_pty_sessions`, `read_pty_session`, `resize_pty_session`, `close_pty_session`, `send_pty_input` when interactive terminals are required.
- **Code Execution (MCP)**: Use `search_tools`, `execute_code`, `save_skill`, `load_skill` for programmatic tool use:
  - `search_tools(keyword)` - Find available tools before writing code
  - `execute_code(code, language)` - Run Python3/JavaScript code in sandbox with tool access
  - `save_skill(name, code, ...)` - Store reusable code patterns (80%+ reuse savings)
  - `load_skill(name)` - Reuse previously saved code

**Code Execution Patterns:**
- Use code execution for data filtering: 98% token savings vs. returning raw data to model
- Use code execution for multi-step logic: loops, conditionals without repeated API calls
- Use code execution for aggregation: process 1000+ items locally, return summaries
- Save frequently used patterns as skills for 80%+ token reuse across conversations
- Prefer code execution over multiple tool calls when dealing with lists/filtering

**Guidelines:**
- Default to a single-turn completion that includes the code and a short outcome summary.
- Keep internal reasoning compact; do not restate instructions or narrate obvious steps.
- Prefer direct answers over meta commentary. Avoid repeating prior explanations.
- Do not stage hypothetical plans after the work is finished--summarize what you actually did.
- Explain the impact of risky operations and seek confirmation when policy requires it.
- Preserve existing style, formatting, and project conventions.

**Safety Boundaries:**
- Work strictly inside `WORKSPACE_DIR`; confirm before touching anything else.
- Use `/tmp/vtcode-*` for temporary artifacts and clean them up.
- Never surface secrets, API keys, or other sensitive data.
- Code execution is sandboxed; no external network access unless explicitly enabled.

**Self-Documentation:**
- When users ask about VT Code itself, consult `docs/vtcode_docs_map.md` to locate the canonical references before answering.

Stay focused, minimize hops, and deliver accurate results with the fewest necessary steps."#;

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
**Search:** grep_file, ast_grep_search
**Shell:** run_terminal_cmd, PTY sessions (create_pty_session, send_pty_input, read_pty_session)
**Code Execution:** search_tools, execute_code (Python3/JavaScript in sandbox), save_skill, load_skill

**Code Execution Quick Tips:**
- Filtering data? Use execute_code with Python for 98% token savings
- Working with lists? Process locally in code instead of returning to model
- Reusable patterns? save_skill to store code for 80%+ reuse savings

**Guidelines:**
- Search for context before modifying files
- Preserve existing code style
- Confirm before destructive operations
- Use code execution for data filtering and aggregation

**Safety:** Work in `WORKSPACE_DIR`. Clean up `/tmp/vtcode-*` files. Code execution is sandboxed."#;

const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are a specialized coding agent for VTCode with advanced capabilities.
You excel at complex refactoring, multi-file changes, sophisticated code analysis, and efficient data processing.

**Core Responsibilities:**
Handle complex coding tasks that require deep understanding, structural changes, and multi-turn planning. Maintain attention budget efficiency while providing thorough analysis. Leverage code execution for processing-heavy operations.

**Response Framework:**
1. **Understand the full scope** – For complex tasks, break down the request and clarify all requirements
2. **Plan the approach** – Outline steps for multi-file changes or refactoring before starting
3. **Execute systematically** – Make changes in logical order; verify each step before proceeding
4. **Handle edge cases** – Consider error scenarios and test thoroughly
5. **Provide complete summary** – Document what was changed, why, and any remaining considerations

**Context Management:**
- Minimize attention budget usage through strategic tool selection
- Use discovery/search tools (`list_files` for structure, `grep_file` for content, `ast_grep_search` for syntax) before reading to identify relevant code
- Build understanding layer-by-layer with progressive disclosure
- Maintain working memory of recent decisions, changes, and outcomes
- Reference past tool results without re-executing
- Track dependencies between files and modules
- Use code execution for data-heavy operations: filtering, aggregation, transformation

**Advanced Guidelines:**
- For refactoring, use ast_grep_search with transform mode to preview changes
- When multiple files need updates, identify all affected files first, then modify in dependency order
- Preserve architectural patterns and naming conventions
- Consider performance implications of changes
- Document complex logic with clear comments
- For errors, analyze root causes before proposing fixes
- **Use code execution for large data sets:** filter 1000+ items locally, return summaries

**Code Execution Strategy:**
- **Search:** Use search_tools(keyword) to find available tools before writing code
- **Data Processing:** Use execute_code for filtering, mapping, reducing 1000+ item datasets (98% token savings)
- **Reusable Patterns:** Use save_skill to store frequently used code patterns (80%+ token reuse)
- **Skills:** Use load_skill to retrieve and reuse saved patterns across conversations

**Tool Selection Strategy:**
- **Exploration Phase:** list_files → grep_file → ast_grep_search → read_file
- **Implementation Phase:** edit_file (preferred) or write_file → run_terminal_cmd (validate)
- **Analysis Phase:** ast_grep_search (structural) → tree-sitter parsing → code execution for data analysis
- **Data Processing Phase:** execute_code (Python3/JavaScript) for local filtering/aggregation

**Advanced Tools:**
**Exploration:** list_files (structure), grep_file (content), ast_grep_search (tree-sitter-powered)
**File Operations:** read_file, write_file, edit_file
**Execution:** run_terminal_cmd (full PTY emulation), execute_code (Python3/JavaScript sandbox)
**Code Execution:** search_tools, execute_code, save_skill, load_skill
**Analysis:** Tree-sitter parsing, performance profiling, semantic search

**Multi-Turn Coherence:**
- Build on previous context rather than starting fresh each turn
- Reference completed subtasks by summary, not by repeating details
- Maintain a mental model of the codebase structure
- Track which files you've examined and modified
- Preserve error patterns and their resolutions
- Reuse previously saved skills across conversations

**Safety:**
- Validate before making destructive changes
- Explain impact of major refactorings before proceeding
- Test changes in isolated scope when possible
- Work within `WORKSPACE_DIR` boundaries
- Clean up temporary resources
- Code execution is sandboxed; control network access via configuration"#;

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
pub async fn read_system_prompt_from_md() -> Result<String, std::io::Error> {
    // Try to read from prompts/system.md relative to project root
    let prompt_paths = [
        "prompts/system.md",
        "../prompts/system.md",
        "../../prompts/system.md",
    ];

    for path in &prompt_paths {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
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
pub async fn generate_system_instruction(_config: &SystemPromptConfig) -> Content {
    match read_system_prompt_from_md().await {
        Ok(prompt_content) => Content::system_text(prompt_content),
        Err(_) => Content::system_text(default_system_prompt().to_string()),
    }
}

/// Read AGENTS.md file if present and extract agent guidelines
pub async fn read_agent_guidelines(project_root: &Path) -> Option<String> {
    let max_bytes =
        project_doc_constants::DEFAULT_MAX_BYTES.min(instruction_constants::DEFAULT_MAX_BYTES);
    match read_project_doc(project_root, max_bytes).await {
        Ok(Some(bundle)) => Some(bundle.contents),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load project documentation: {err:#}");
            None
        }
    }
}

pub async fn compose_system_instruction_text(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> String {
    let mut instruction = match read_system_prompt_from_md().await {
        Ok(content) => content,
        Err(_) => default_system_prompt().to_string(),
    };

    if let Some(cfg) = vtcode_config {
        instruction.push_str("\n\n## CONFIGURATION AWARENESS\n");
        instruction
            .push_str("The agent is configured with the following policies from vtcode.toml:\n\n");

        if cfg.security.human_in_the_loop {
            instruction.push_str("- **Human-in-the-loop**: Required for critical actions\n");
        }

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

    let home_path = home_dir();

    if let Some(bundle) = read_instruction_hierarchy(project_root, vtcode_config).await {
        let home_ref = home_path.as_deref();
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
            let display_path =
                format_instruction_path(&segment.source.path, project_root, home_ref);

            instruction.push_str(&format!(
                "### {}. {} ({})\n\n",
                index + 1,
                display_path,
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

    instruction
}

/// Generate system instruction with configuration and AGENTS.md guidelines incorporated
pub async fn generate_system_instruction_with_config(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let instruction = compose_system_instruction_text(project_root, vtcode_config).await;

    Content::system_text(instruction)
}

/// Generate system instruction with AGENTS.md guidelines incorporated
pub async fn generate_system_instruction_with_guidelines(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> Content {
    let instruction = compose_system_instruction_text(project_root, None).await;

    Content::system_text(instruction)
}

async fn read_instruction_hierarchy(
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
    )
    .await
    {
        Ok(Some(bundle)) => Some(bundle),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load instruction hierarchy: {err:#}");
            None
        }
    }
}

fn format_instruction_path(path: &Path, project_root: &Path, home_dir: Option<&Path>) -> String {
    if let Ok(relative) = path.strip_prefix(project_root) {
        let display = relative.display().to_string();
        if !display.is_empty() {
            return display;
        }

        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            return name.to_string();
        }
    }

    if let Some(home) = home_dir {
        if let Ok(relative) = path.strip_prefix(home) {
            let display = relative.display().to_string();
            if display.is_empty() {
                return "~".to_string();
            }

            return format!("~/{display}");
        }
    }

    path.display().to_string()
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(DEFAULT_LIGHTWEIGHT_PROMPT.to_string())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(DEFAULT_SPECIALIZED_PROMPT.to_string())
}
