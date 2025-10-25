use std::path::Path;

pub(crate) async fn read_system_prompt(workspace: &Path, session_addendum: Option<&str>) -> String {
    let mut prompt = vtcode_core::prompts::read_system_prompt_from_md()
        .await
        .unwrap_or_else(|_| "You are a helpful coding assistant for a Rust workspace.".to_string());

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
