use std::path::Path;
use vtcode_core::config::constants::prompt_budget as prompt_budget_constants;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::SystemPromptMode;
use vtcode_core::prompts::PromptContext;
use vtcode_core::prompts::system::{
    SystemPromptReport, compose_system_instruction_with_report, default_lightweight_prompt, default_system_prompt,
    minimal_system_prompt, specialized_system_prompt,
};

/// Seed system prompt used on the interactive critical path before full
/// workspace prompt composition finishes after the first UI frame.
/// Missing config follows `SystemPromptMode::default()` (Minimal).
pub(crate) fn fallback_base_system_prompt(vt_cfg: Option<&vtcode_core::config::VTCodeConfig>) -> &'static str {
    match vt_cfg.map(|cfg| cfg.agent.system_prompt_mode).unwrap_or_default() {
        SystemPromptMode::Minimal => minimal_system_prompt(),
        SystemPromptMode::Lightweight => default_lightweight_prompt(),
        SystemPromptMode::Specialized => specialized_system_prompt(),
        SystemPromptMode::Default => default_system_prompt(),
    }
}

pub(crate) async fn read_system_prompt(
    workspace: &Path,
    session_addendum: Option<&str>,
    available_subagents: &[(String, String, bool)],
) -> (String, SystemPromptReport) {
    let mut prompt_context = PromptContext::from_workspace_tools(workspace, std::iter::empty::<String>());
    prompt_context.load_available_skills();

    let vt_cfg = ConfigManager::load_from_workspace(workspace)
        .ok()
        .map(|manager| manager.config().clone());

    let (mut prompt, mut report) =
        compose_system_instruction_with_report(workspace, vt_cfg.as_ref(), Some(&prompt_context)).await;

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
        let max_chars =
            usize::try_from(remaining_budget.saturating_mul(vtcode_commons::tokens::CHARS_PER_TOKEN as u64))
                .unwrap_or(usize::MAX);
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

    let token_estimate = vtcode_commons::estimate_tokens(&prompt) as u64;
    let over_budget = token_estimate > max_tokens;
    let trimmed_sections = report.trimmed_sections.clone();
    (prompt, SystemPromptReport { token_estimate, over_budget, trimmed_sections })
}

fn budget_addendum(addendum: &str, remaining_budget_tokens: u64) -> String {
    let addendum_tokens = vtcode_commons::estimate_tokens(addendum) as u64;
    if addendum_tokens <= remaining_budget_tokens {
        return addendum.to_string();
    }

    const TRUNCATION_MARKER: &str = "...";
    let marker_tokens = vtcode_commons::estimate_tokens(TRUNCATION_MARKER) as u64;
    if remaining_budget_tokens <= marker_tokens {
        return vtcode_commons::truncate_to_tokens(
            TRUNCATION_MARKER,
            usize::try_from(remaining_budget_tokens).unwrap_or(usize::MAX),
        );
    }
    let content_budget = usize::try_from(remaining_budget_tokens - marker_tokens).unwrap_or(usize::MAX);
    let truncated = vtcode_commons::truncate_to_tokens(addendum, content_budget);
    format!("{truncated}{TRUNCATION_MARKER}")
}

/// Heading plus delegation guidance shared by the full and budgeted subagent
/// sections, so the size estimate and both renderings stay in sync.
const SUBAGENT_SECTION_HEADER: &str = "## Subagents\n\
Delegated child agents available in this session. Treat the main thread as the controller: keep the next blocking \
step local, and delegate only bounded independent work that is large enough to be worth a separate context, such as a \
broad search or a self-contained change. Checking your own work is not independent work, so run that check yourself. \
Read-only agents may be used proactively when their description matches; write-capable agents require explicit \
delegation.\n\
Users can explicitly target one with natural language or an `@agent-<name>` mention.\n\
If the user explicitly selects a subagent for the task, delegate with `spawn_agent` to that subagent instead of \
handling the task on the main thread. Join child results back into the parent flow before you depend on them.";

fn subagent_entry(name: &str, description: &str, read_only: bool) -> String {
    let suffix = if read_only {
        " Read-only."
    } else {
        " Explicit delegation only."
    };
    format!("- {name}: {description}{suffix}")
}

fn estimate_subagent_section_chars(subagents: &[(String, String, bool)]) -> usize {
    let entries: usize = subagents
        .iter()
        .map(|(name, desc, read_only)| subagent_entry(name, desc, *read_only).len() + 1)
        .sum();
    SUBAGENT_SECTION_HEADER.len() + 1 + entries
}

fn build_full_subagent_section(subagents: &[(String, String, bool)]) -> String {
    let mut section = SUBAGENT_SECTION_HEADER.to_string();
    for (name, description, read_only) in subagents {
        section.push('\n');
        section.push_str(&subagent_entry(name, description, *read_only));
    }
    section
}

fn build_summarized_subagent_section(subagents: &[(String, String, bool)]) -> String {
    let count = subagents.len();
    let read_only = subagents.iter().filter(|(_, _, ro)| *ro).count();
    let writable = count - read_only;
    format!(
        "## Subagents\n{count} subagents available ({} read-only, {} writable). Use `spawn_agent` to delegate bounded independent work; keep the next blocking step, and checks of your own work, on the main thread. The user can list them with `/agent`.",
        read_only, writable
    )
}

fn budget_subagent_section(subagents: &[(String, String, bool)], max_chars: usize) -> String {
    let mut section = SUBAGENT_SECTION_HEADER.to_string();
    let mut remaining = max_chars.saturating_sub(section.len());
    for (index, (name, description, read_only)) in subagents.iter().enumerate() {
        let entry = subagent_entry(name, description, *read_only);
        if entry.len() > remaining {
            section.push_str(&format!("\n- ... ({} more agents truncated)", subagents.len() - index));
            break;
        }
        remaining = remaining.saturating_sub(entry.len() + 1);
        section.push('\n');
        section.push_str(&entry);
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::VTCodeConfig;
    use vtcode_core::config::types::SystemPromptMode;

    #[test]
    fn test_fallback_base_system_prompt_defaults_to_enum_default() {
        // Missing config must follow SystemPromptMode::default() (Minimal).
        assert_eq!(SystemPromptMode::default(), SystemPromptMode::Minimal);
        assert_eq!(fallback_base_system_prompt(None), minimal_system_prompt());
    }

    #[test]
    fn test_fallback_base_system_prompt_uses_explicit_default_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;

        assert_eq!(fallback_base_system_prompt(Some(&config)), default_system_prompt());
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
        assert!(prompt.contains("builder: Write-capable implementation agent Explicit delegation only."));
        assert!(!prompt.contains("subagents available"));
    }

    #[test]
    fn budget_addendum_uses_token_boundary_for_large_input() {
        let addendum = "Use the repository guidance carefully. ".repeat(2_000);
        let budget = 32;

        let result = budget_addendum(&addendum, budget);

        assert!(result.ends_with("..."));
        assert!(result.len() < addendum.len());
        assert!(vtcode_commons::estimate_tokens(&result) <= usize::try_from(budget + 1).unwrap());
        assert!(budget_addendum(&addendum, 0).is_empty());
    }
}
