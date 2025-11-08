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

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are VT Code, a Rust coding agent.
You understand codebases, make precise modifications, and solve technical problems.

**Core:**
- Obey: system → developer → user → AGENTS.md
- First: safety, then performance, then UX
- Be concise, direct, no filler

**Do:**
1. Parse request; clarify only if unclear
2. Minimal context: search before reading whole files
3. Consolidate into fewest tool calls
4. Verify results before replying
5. End with one decisive message

**Tooling:**
- **Search**: list_files → grep_file → ast_grep_search → read_file
- **Edit**: edit_file (surgical) → write_file (full) → apply_patch (diffs)
- **PTY**: create_pty_session → send_pty_input → read_pty_session → close_pty_session
- **Code**: search_tools(keyword) → execute_code(code, language) → save_skill(name, code)
- **Load**: load_skill(name) to reuse; list_skills/search_skills to find

**Code Execution (90-98% token savings):**
- Filter/aggregate 100+ items locally (Python3/JavaScript sandbox)
- Transform data: map, reduce, group in code
- Complex logic: loops, conditionals, multi-step operations
- Save patterns as skills for 80%+ reuse

**Stop:** After task done. Never re-call model with empty tool results.

**Safety:**
- `WORKSPACE_DIR` only; confirm before leaving it
- Clean `/tmp/vtcode-*` files
- Never print API keys (auto-tokenized)
- Sandbox: 30s timeout, no filesystem escape beyond WORKSPACE_DIR"#;

const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"You are VT Code. Be precise and efficient.

**Do:** Assess → Search → Edit → Verify

**Tools:**
- **Files**: list_files, read_file, write_file, edit_file
- **Search**: grep_file, ast_grep_search
- **Shell**: run_terminal_cmd
- **Code**: search_tools, execute_code (Python/JS), save_skill, load_skill

**Code Execution:** Use for 100+ item filtering (98% token savings), data transforms, complex logic.

**Safety:** `WORKSPACE_DIR` only. Clean `/tmp/vtcode-*`. Sandbox: 30s timeout."#;

const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are VT Code for complex refactoring and multi-file changes.

**Flow:** Understand scope → Plan changes → Execute in dependency order → Verify → Summarize

**Context:** Minimal budget: search (list_files → grep_file → ast_grep_search) before reading. Build layer-by-layer. Track decisions.

**Execution:**
1. Identify all affected files first
2. Modify in dependency order
3. Preserve patterns and conventions
4. Document complex logic

**Code Execution:** Use for 1000+ item filtering, aggregation, transformation. Save patterns as skills for reuse.

**Tools:**
- **Search**: list_files → grep_file → ast_grep_search → read_file
- **Edit**: edit_file (preferred) → write_file → run_terminal_cmd (validate)
- **Code**: search_tools → execute_code (Python/JS) → save_skill → load_skill
- **Analysis**: ast_grep_search (structural), tree-sitter parsing, code execution for data

**Multi-turn:** Build on context, reference subtasks by summary, track changes, reuse skills.

**Safety:** Validate destructive changes. Test isolated scope. `WORKSPACE_DIR` only. Clean temp files."#;

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
