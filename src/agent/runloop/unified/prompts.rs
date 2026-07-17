use std::path::Path;
use vtcode_core::config::constants::prompt_budget as prompt_budget_constants;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::SystemPromptMode;
use vtcode_core::prompts::PromptContext;
use vtcode_core::prompts::system::{
    SystemPromptReport, compose_system_instruction_with_report, default_lightweight_prompt,
    default_system_prompt, minimal_system_prompt, specialized_system_prompt,
};

fn fallback_base_system_prompt(vt_cfg: Option<&vtcode_core::config::VTCodeConfig>) -> &'static str {
    match vt_cfg.map(|cfg| cfg.agent.system_prompt_mode) {
        Some(SystemPromptMode::Minimal) => minimal_system_prompt(),
        Some(SystemPromptMode::Lightweight) => default_lightweight_prompt(),
        Some(SystemPromptMode::Specialized) => specialized_system_prompt(),
        _ => default_system_prompt(),
    }
}

pub(crate) async fn read_system_prompt(
    workspace: &Path,
    session_addendum: Option<&str>,
    available_subagents: &[(String, String, bool)],
) -> (String, SystemPromptReport) {
    let mut prompt_context =
        PromptContext::from_workspace_tools(workspace, std::iter::empty::<String>());
    prompt_context.load_available_skills();

    let vt_cfg = ConfigManager::load_from_workspace(workspace)
        .ok()
        .map(|manager| manager.config().clone());

    let (mut prompt, mut report) =
        compose_system_instruction_with_report(workspace, vt_cfg.as_ref(), Some(&prompt_context))
            .await;

    if prompt.is_empty() {
        prompt = fallback_base_system_prompt(vt_cfg.as_ref()).to_string();
        let max_tokens = vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.max_system_prompt_tokens)
            .unwrap_or(prompt_budget_constants::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS);
        report = SystemPromptReport::measure(&prompt, max_tokens);
    }

    let max_tokens = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.max_system_prompt_tokens)
        .unwrap_or(prompt_budget_constants::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS);
    let remaining_budget = max_tokens.saturating_sub(report.token_estimate);

    if let Some(addendum) = session_addendum {
        let trimmed = addendum.trim();
        if !trimmed.is_empty() {
            let budgeted = budget_addendum(trimmed, remaining_budget);
            if !budgeted.is_empty() {
                prompt.push_str("\n\n");
                prompt.push_str(&budgeted);
            }
        }
    }

    prompt.push_str(
        "\n\nEffective runtime controller, tool, permission, model, reasoning, and temporal state is supplied in request runtime state.",
    );

    if !available_subagents.is_empty() {
        let subagent_chars = estimate_subagent_section_chars(available_subagents);
        let max_chars = (remaining_budget * 4) as usize;
        let section = if available_subagents.len() > 3 {
            build_summarized_subagent_section(available_subagents)
        } else if subagent_chars > max_chars {
            budget_subagent_section(available_subagents, max_chars)
        } else {
            build_full_subagent_section(available_subagents)
        };
        prompt.push_str("\n\n");
        prompt.push_str(&section);
    }

    let token_estimate = prompt.len().div_ceil(4) as u64;
    let over_budget = report.token_estimate > max_tokens;
    let trimmed_sections = report.trimmed_sections.clone();
    (prompt, SystemPromptReport { token_estimate, over_budget, trimmed_sections })
}

fn budget_addendum(addendum: &str, remaining_budget_tokens: u64) -> String {
    let addendum_tokens = addendum.len() as u64 / 4;
    if addendum_tokens <= remaining_budget_tokens {
        return addendum.to_string();
    }
    let max_chars = (remaining_budget_tokens * 4) as usize;
    let truncated: String = addendum.chars().take(max_chars).collect();
    format!("{truncated}...")
}

fn estimate_subagent_section_chars(subagents: &[(String, String, bool)]) -> usize {
    let header = "## Subagents\nDelegated child agents available in this session. Treat the main thread as the controller: keep the next blocking step local, and delegate only bounded independent work. Read-only agents may be used proactively when their description matches; write-capable agents require explicit delegation.\nUsers can explicitly target one with natural language or an `@agent-<name>` mention.\nIf the user explicitly selects a subagent for the task, delegate with `spawn_agent` to that subagent instead of handling the task on the main thread. Join child results back into the parent flow before you depend on them.\n";
    let entries: usize = subagents
        .iter()
        .map(|(name, desc, read_only)| {
            let suffix = if *read_only {
                " Read-only."
            } else {
                " Explicit delegation only."
            };
            name.len() + desc.len() + suffix.len() + 3 // "- " + "\n"
        })
        .sum();
    header.len() + entries
}

fn build_full_subagent_section(subagents: &[(String, String, bool)]) -> String {
    let mut lines = Vec::with_capacity(4 + subagents.len());
    lines.push("## Subagents".to_string());
    lines.push("Delegated child agents available in this session. Treat the main thread as the controller: keep the next blocking step local, and delegate only bounded independent work. Read-only agents may be used proactively when their description matches; write-capable agents require explicit delegation.".to_string());
    lines.push(
        "Users can explicitly target one with natural language or an `@agent-<name>` mention."
            .to_string(),
    );
    lines.push("If the user explicitly selects a subagent for the task, delegate with `spawn_agent` to that subagent instead of handling the task on the main thread. Join child results back into the parent flow before you depend on them.".to_string());
    for (name, description, read_only) in subagents {
        let suffix = if *read_only {
            " Read-only."
        } else {
            " Explicit delegation only."
        };
        lines.push(format!("- {name}: {description}{suffix}"));
    }
    lines.join("\n")
}

fn build_summarized_subagent_section(subagents: &[(String, String, bool)]) -> String {
    let count = subagents.len();
    let read_only = subagents.iter().filter(|(_, _, ro)| *ro).count();
    let writable = count - read_only;
    format!(
        "## Subagents\n{count} subagents available ({} read-only, {} writable). Use `/agent` to inspect and `spawn_agent` to delegate.",
        read_only, writable
    )
}

fn budget_subagent_section(subagents: &[(String, String, bool)], max_chars: usize) -> String {
    let mut lines = Vec::with_capacity(4 + subagents.len());
    lines.push("## Subagents".to_string());
    lines.push("Delegated child agents available in this session. Treat the main thread as the controller: keep the next blocking step local, and delegate only bounded independent work. Read-only agents may be used proactively when their description matches; write-capable agents require explicit delegation.".to_string());
    lines.push(
        "Users can explicitly target one with natural language or an `@agent-<name>` mention."
            .to_string(),
    );
    lines.push("If the user explicitly selects a subagent for the task, delegate with `spawn_agent` to that subagent instead of handling the task on the main thread. Join child results back into the parent flow before you depend on them.".to_string());
    let mut remaining = max_chars.saturating_sub(lines.join("\n").len());
    for (name, description, read_only) in subagents {
        let suffix = if *read_only {
            " Read-only."
        } else {
            " Explicit delegation only."
        };
        let entry = format!("- {name}: {description}{suffix}");
        let entry_len = entry.len();
        if entry_len > remaining {
            lines.push(format!(
                "- ... ({} more agents truncated)",
                subagents.len() - lines.len() + 4
            ));
            break;
        }
        lines.push(entry);
        remaining = remaining.saturating_sub(entry_len + 1);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::VTCodeConfig;
    use vtcode_core::config::types::SystemPromptMode;

    #[test]
    fn test_fallback_base_system_prompt_defaults_to_default() {
        assert_eq!(fallback_base_system_prompt(None), default_system_prompt());
    }

    #[test]
    fn test_fallback_base_system_prompt_uses_minimal_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;

        assert_eq!(fallback_base_system_prompt(Some(&config)), minimal_system_prompt());
    }

    #[test]
    fn test_fallback_base_system_prompt_uses_lightweight_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;

        assert_eq!(fallback_base_system_prompt(Some(&config)), default_lightweight_prompt());
    }

    #[test]
    fn test_fallback_base_system_prompt_uses_specialized_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Specialized;

        assert_eq!(fallback_base_system_prompt(Some(&config)), specialized_system_prompt());
    }

    #[tokio::test]
    async fn test_read_system_prompt_includes_explicit_subagent_execution_model() {
        let workspace = tempfile::TempDir::new().expect("workspace");

        let (prompt, _report) = read_system_prompt(
            workspace.path(),
            None,
            &[("explorer".to_string(), "Read-only repo explorer".to_string(), true)],
        )
        .await;

        assert!(prompt.contains("main thread as the controller"));
        assert!(prompt.contains("bounded independent work"));
        assert!(prompt.contains("Join child results back into the parent flow"));
    }

    #[tokio::test]
    async fn test_read_system_prompt_summarizes_large_subagent_list() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let subagents: Vec<(String, String, bool)> = (0..5)
            .map(|i| (format!("agent-{i}"), format!("Description {i}"), i % 2 == 0))
            .collect();

        let (prompt, _report) = read_system_prompt(workspace.path(), None, &subagents).await;

        assert!(prompt.contains("5 subagents available"));
        assert!(prompt.contains("read-only"));
        assert!(prompt.contains("writable"));
        assert!(!prompt.contains("Description 0"));
    }

    #[tokio::test]
    async fn test_read_system_prompt_lists_small_subagent_list_in_full() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let subagents = vec![
            ("explorer".to_string(), "Read-only repo explorer".to_string(), true),
            ("builder".to_string(), "Write-capable implementation agent".to_string(), false),
        ];

        let (prompt, _report) = read_system_prompt(workspace.path(), None, &subagents).await;

        assert!(prompt.contains("explorer: Read-only repo explorer Read-only."));
        assert!(
            prompt
                .contains("builder: Write-capable implementation agent Explicit delegation only.")
        );
        assert!(!prompt.contains("subagents available"));
    }
}
