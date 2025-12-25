use std::path::Path;

pub(crate) async fn read_system_prompt(workspace: &Path, session_addendum: Option<&str>) -> String {
    let content = vtcode_core::prompts::generate_system_instruction(&Default::default()).await;
    let mut prompt = if let Some(text) = content.parts.first().and_then(|p| p.as_text()) {
        text.to_string()
    } else {
        r#"# VT Code - Rust Coding Assistant

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
            .to_string()
    };

    if let Some(overview) = vtcode_core::utils::common::build_project_overview(workspace).await {
        prompt.push_str("\n\n## PROJECT OVERVIEW\n");
        prompt.push_str(&overview.as_prompt_block());
    }

    if let Some(guidelines) = vtcode_core::prompts::system::read_agent_guidelines(workspace).await {
        prompt.push_str("\n\n## AGENTS.MD GUIDELINES\n");
        prompt.push_str(&guidelines);
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
