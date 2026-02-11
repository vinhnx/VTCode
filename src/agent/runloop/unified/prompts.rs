use std::path::Path;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::prompts::PromptContext;
use vtcode_core::prompts::system::compose_system_instruction_text;

pub(crate) async fn read_system_prompt(
    workspace: &Path,
    session_addendum: Option<&str>,
    available_tools: &[String],
) -> String {
    // Build PromptContext with available information (workspace, current directory, tools)
    let mut prompt_context = PromptContext {
        workspace: Some(workspace.to_path_buf()),
        skip_standard_instructions: false,
        ..Default::default()
    };

    // Set current working directory
    if let Ok(cwd) = std::env::current_dir() {
        prompt_context.set_current_directory(cwd);
    }

    // Populate available tools so dynamic tool-aware guidelines match runtime capabilities.
    for tool in available_tools {
        prompt_context.add_tool(tool.clone());
    }
    if !prompt_context.available_tools.is_empty() {
        prompt_context.infer_capability_level();
    }

    // Load configuration
    let vt_cfg = ConfigManager::load_from_workspace(workspace)
        .ok()
        .map(|manager| manager.config().clone());

    // Use the new compose_system_instruction_text with enhancements
    let mut prompt =
        compose_system_instruction_text(workspace, vt_cfg.as_ref(), Some(&prompt_context)).await;

    // Fallback prompt if composition fails (should rarely happen)
    if prompt.is_empty() {
        prompt = r#"# VT Code - Rust Coding Assistant

Use tools immediately. Stop when done or blocked.

## Rules
- Stay in workspace. Confirm destructive ops (rm, force-push). No secrets.
- Read files before editing. Verify changes with tests/cargo check.
- Direct tone, 1-2 sentence summaries. No code dumps or emojis.

## Execution
1. UNDERSTAND: Parse request
2. GATHER: Search before reading files
3. EXECUTE: Fewest tool calls, quote paths
4. VERIFY: Check results
5. REPLY: Stop once solved

## Tool Safety
- Prefer read-only first. Retry transient errors once.
- After 3+ low-signal calls, reassess approach.

## Rust
- Idiomatic code with proper ownership/borrowing
- Use cargo, rustfmt, clippy. Handle errors with Result/anyhow."#
            .to_string();
    }

    if let Some(addendum) = session_addendum {
        let trimmed = addendum.trim();
        if !trimmed.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(trimmed);
        }
    }

    prompt
}
