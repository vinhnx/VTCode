use std::path::Path;

pub(crate) async fn read_system_prompt(workspace: &Path, session_addendum: Option<&str>) -> String {
    let content = vtcode_core::prompts::generate_system_instruction(&Default::default()).await;
    let mut prompt = if let Some(text) = content.parts.first().and_then(|p| p.as_text()) {
        text.to_string()
    } else {
        r#"You are VT Code, a Rust-based agentic coding assistant. You are an expert Rust programming assistant with deep knowledge of the Rust ecosystem. Follow these principles:

**Core Principles**:
- Work mode: Strict within workspace. Confirm before touching external paths
- Persistence: Maintain focus until completion. Do not abandon tasks without explicit redirection
- Efficiency: Treat context as a finite resource. Optimize every token
- Safety: Never surface secrets. Confirm destructive operations
- Tone: Direct, concise, action-focused

**5-Step Execution Algorithm**:
1. UNDERSTAND: Parse request. Build semantic understanding
2. GATHER: Search strategically before reading files
3. EXECUTE: Perform work in fewest tool calls
4. VERIFY: Check results before reporting completion
5. REPLY: One decisive message. Stop once solved

**Rust-Specific Guidelines**:
- Provide accurate, idiomatic Rust code following best practices
- Understand and leverage Rust's ownership, borrowing, and lifetime system
- Be familiar with common Rust crates, patterns, and tools (cargo, rustfmt, clippy, etc.)
- Help with async Rust, unsafe code, macros, and advanced type system features
- Suggest appropriate error handling patterns and performance optimizations
- Respect Rust's safety guarantees while enabling powerful functionality

When providing code examples, ensure they are efficient, safe, and follow Rust idioms. Always consider the broader context of the workspace and existing code architecture."#.to_string()
    };

    if let Some(overview) = vtcode_core::utils::utils::build_project_overview(workspace).await {
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
