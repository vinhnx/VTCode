pub(super) use super::*;
pub(super) use crate::config::types::ShellPromptProfile;

/// Default-density guidance with the platform shell profile and the
/// parallel-call hint, matching what static composition ships.
fn guidelines_for(tools: &[String], capability_level: Option<CapabilityLevel>) -> String {
    generate_tool_guidelines_for_profile(
        tools,
        capability_level,
        ShellPromptProfile::Auto.resolve_for_current_platform(),
        true,
    )
}

/// Reference the previous model-aware assembly: measure Default guidance,
/// then regenerate it with resolved density and capabilities.
fn rebuilt_model_tool_prompt(
    mut prompt: String,
    snapshot: &SessionToolCatalogSnapshot,
    metadata: bool,
    shell: ResolvedShellPromptProfile,
    provider: &dyn crate::llm::provider::LLMProvider,
    model: &str,
    config: &crate::config::VTCodeConfig,
) -> String {
    append_runtime_tool_prompt_sections_for_profile(&mut prompt, snapshot, metadata, shell);
    let names = snapshot_tool_names(snapshot);
    let pricing = crate::config::models::model_catalog_entry(provider.name(), model).map(|entry| entry.pricing);
    let profile = ToolGuidanceProfile::resolve(
        crate::compaction::effective_context_budget(Some(config), provider, model),
        vtcode_commons::estimate_tokens(&prompt),
        config.agent.max_system_prompt_tokens as usize,
        pricing.and_then(|price| price.input),
        config.agent.harness.max_budget_usd,
    );
    if snapshot.planning_active && profile == ToolGuidanceProfile::Default {
        return prompt;
    }
    remove_prompt_section(&mut prompt, "## Active Tools", SectionBoundaryMode::BracketOrMarkdown);
    let presence = ToolPresence::of(names);
    let mut guidance = generate_tool_guidelines_with_capabilities(
        names,
        Some(infer_capability_level_from_presence(&presence)),
        shell,
        profile,
        provider.supports_parallel_tool_config(model),
    );
    if snapshot.planning_active {
        append_minimal_planning_addendum(&mut guidance, &presence);
    }
    append_prompt_block(&mut prompt, guidance.trim_start_matches('\n'));
    prompt
}

mod capability;
mod compact_deferred;
mod model_prompts;
mod parallel_planning;
mod planning_shell;
mod presence_parallel;
mod runtime_sections;
mod schema_ownership;
mod universal_docs;
