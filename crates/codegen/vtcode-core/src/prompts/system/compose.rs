//! System-instruction composition, budget trimming, and identity substitution.

use std::path::Path;

use crate::config::constants::prompt_budget as prompt_budget_constants;
use crate::config::types::ShellPromptProfile;
use crate::llm::providers::gemini::wire::Content;
use crate::prompts::context::PromptContext;
use crate::prompts::guidelines::{generate_tool_guidelines_for_profile, render_shell_profile_guidance};
use crate::prompts::output_styles::OutputStyleApplier;
use crate::prompts::render::render_environment_addenda;
use crate::prompts::resources::{apply_system_prompt_layers, resolve_system_prompt_layers};
use crate::prompts::static_prompts::static_profile_prompt;
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::skills::render::render_prompt_skills_section;
use tracing::warn;

use super::cache::{cache_key_for_identity, prompt_context_digest};
use super::constants::{PROMPT_IDENTITY_NAME, PROMPT_INTRO, PROMPT_TITLE, STRUCTURED_REASONING_INSTRUCTIONS};
use super::tokens::estimate_token_count;
use super::types::{PromptSection, SectionKind, SystemPromptConfig, SystemPromptReport};

use crate::prompts::static_prompts::agent_identity_label;

/// Compose the base system instruction plus compact tool/skill/environment addenda.
pub async fn compose_system_instruction_text(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> String {
    compose_system_instruction_with_report(project_root, vtcode_config, prompt_context)
        .await
        .0
}

/// Compose the system instruction and return the token-budget report
/// alongside it. See [`SystemPromptReport`] and `SectionKind::trim_priority`
/// for the budget/trim behavior driven by `agent.max_system_prompt_tokens`,
/// `agent.system_prompt_budget_warning`, and `agent.trim_system_prompt`.
pub async fn compose_system_instruction_with_report(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> (String, SystemPromptReport) {
    let (prompt, report, _) =
        compose_system_instruction_with_identity(project_root, vtcode_config, prompt_context).await;
    (prompt, report)
}

/// Compose a prompt and retain the stable instruction digest used by local and
/// provider-facing prompt caches.
async fn compose_system_instruction_with_identity(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> (String, SystemPromptReport, u64) {
    let sections = build_prompt_sections(project_root, vtcode_config, prompt_context).await;
    let instruction_digest = stable_prompt_sections_digest(&sections);
    let (max_tokens, warn_enabled, trim_enabled) = system_prompt_budget_settings(vtcode_config);
    let (prompt, report) = apply_token_budget(sections, max_tokens, warn_enabled, trim_enabled);
    (prompt, report, instruction_digest)
}

/// Measure the system prompt size without applying budget trimming or warnings.
///
/// This is used at startup to warn about potential token budget overruns
/// before the first request is made. Unlike [`compose_system_instruction_with_report`],
/// this function does not apply `agent.trim_system_prompt` and does not emit
/// budget-exceeded warnings.
pub async fn measure_system_prompt_size(
    project_root: &Path,
    vtcode_config: &crate::config::VTCodeConfig,
) -> SystemPromptReport {
    let sections = build_prompt_sections(project_root, Some(vtcode_config), None).await;
    let text = join_prompt_sections(&sections);
    let token_estimate = estimate_token_count(&text);
    SystemPromptReport {
        token_estimate,
        over_budget: token_estimate > vtcode_config.agent.max_system_prompt_tokens,
        trimmed_sections: Vec::new(),
    }
}

/// Resolve the effective `(max_system_prompt_tokens, budget_warning_enabled,
/// trim_enabled)` settings, falling back to the `AgentConfig` defaults when
/// no config is available.
fn system_prompt_budget_settings(vtcode_config: Option<&crate::config::VTCodeConfig>) -> (u64, bool, bool) {
    vtcode_config.map_or((prompt_budget_constants::DEFAULT_MAX_SYSTEM_PROMPT_TOKENS, true, true), |cfg| {
        (
            cfg.agent.max_system_prompt_tokens,
            cfg.agent.system_prompt_budget_warning,
            cfg.agent.trim_system_prompt,
        )
    })
}

/// Build the ordered prompt sections. Each section's text is stored exactly
/// as the legacy single-string builder would have appended it, so
/// [`join_prompt_sections`] reproduces byte-identical output when nothing is
/// trimmed.
pub(super) async fn build_prompt_sections(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> Vec<PromptSection> {
    let prompt_mode = vtcode_config.map(|c| c.agent.system_prompt_mode).unwrap_or_default();
    let static_base_prompt = static_profile_prompt(prompt_mode);
    let resolved_layers = resolve_system_prompt_layers(project_root).await;
    let mut base_prompt = apply_system_prompt_layers(static_base_prompt, &resolved_layers);
    crate::prompts::runtime_guidance::ensure_runtime_guidance(&mut base_prompt);

    tracing::trace!(
        mode = ?prompt_mode,
        base_tokens = estimate_token_count(&base_prompt),
        "Selected system prompt mode"
    );

    // Apply agent identity based on the default primary agent configuration.
    // This combines "VT Code" with the active agent mode so the LLM knows its role.
    if let Some(cfg) = vtcode_config {
        let agent_label = agent_identity_label(&cfg.default_primary_agent);
        base_prompt = apply_agent_identity(&base_prompt, &agent_label);
        apply_coordinator_role_guidance(&mut base_prompt, cfg.default_primary_agent == "coordinator");
    }

    let mut sections = vec![PromptSection { kind: SectionKind::BaseContract, text: base_prompt }];

    if should_include_structured_reasoning(vtcode_config) {
        sections.push(PromptSection {
            kind: SectionKind::StructuredReasoning,
            text: STRUCTURED_REASONING_INSTRUCTIONS.to_string(),
        });
    }

    let shell_profile = vtcode_config
        .map(|cfg| cfg.agent.shell_prompt_profile)
        .unwrap_or(ShellPromptProfile::Auto)
        .resolve_for_current_platform();
    sections.push(PromptSection {
        kind: SectionKind::ShellProfile,
        text: render_shell_profile_guidance(shell_profile),
    });

    if let Some(ctx) = prompt_context {
        // Prompt-caching discipline: static content first, dynamic last.
        // `Skills` is session-stable routing metadata while `ToolGuidelines`
        // derives from the live tool catalog (planning toggles, MCP refreshes),
        // so skills must precede tool guidelines. Otherwise every catalog
        // change would invalidate the cached skills section that follows it.
        if let Some(skills_section) = render_prompt_skills_section(&ctx.available_skill_metadata) {
            sections.push(PromptSection { kind: SectionKind::Skills, text: skills_section });
        }
        // Static prompts ship the parallel-call hint unconditionally; only the
        // runtime per-turn path re-resolves it against the provider.
        let guidelines =
            generate_tool_guidelines_for_profile(&ctx.available_tools, ctx.capability_level, shell_profile, true);
        if !guidelines.is_empty() {
            sections.push(PromptSection {
                kind: SectionKind::ToolGuidelines,
                text: guidelines.trim_start_matches('\n').to_string(),
            });
        }
    }

    if let Some(environment_section) = render_environment_addenda(vtcode_config, prompt_context) {
        sections.push(PromptSection {
            kind: SectionKind::EnvironmentAddenda,
            text: environment_section,
        });
    }

    sections
}

/// Join ordered prompt sections exactly as the legacy single-string builder
/// did: the first section verbatim, then each subsequent section separated
/// by a blank line.
pub(super) fn join_prompt_sections(sections: &[PromptSection]) -> String {
    let capacity = sections.iter().map(|section| section.text.len() + 2).sum();
    let mut joined = String::with_capacity(capacity);
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            joined.push_str("\n\n");
        }
        joined.push_str(&section.text);
    }
    joined
}

/// Enforce the configured system-prompt token budget against the composed
/// sections.
///
/// When under budget, sections are joined and returned unchanged. When over
/// budget and `trim_enabled` is false, the full untrimmed text is still used
/// but a warning is logged (gated on `warn_enabled`). When over budget and
/// `trim_enabled` is true, whole sections are dropped in
/// [`SectionKind::trim_priority`] order (lowest first), re-measuring after
/// each drop, until the prompt fits or only untrimmable sections remain.
pub(super) fn apply_token_budget(
    mut sections: Vec<PromptSection>,
    max_tokens: u64,
    warn_enabled: bool,
    trim_enabled: bool,
) -> (String, SystemPromptReport) {
    let mut text = join_prompt_sections(&sections);
    let mut token_estimate = estimate_token_count(&text);
    let mut trimmed_sections: Vec<&'static str> = Vec::new();

    if token_estimate > max_tokens {
        if trim_enabled {
            while token_estimate > max_tokens {
                let drop_index = sections
                    .iter()
                    .enumerate()
                    .filter_map(|(index, section)| section.kind.trim_priority().map(|priority| (priority, index)))
                    .min_by_key(|(priority, _)| *priority)
                    .map(|(_, index)| index);
                let Some(drop_index) = drop_index else {
                    break;
                };
                let dropped = sections.remove(drop_index);
                trimmed_sections.push(dropped.kind.name());
                text = join_prompt_sections(&sections);
                token_estimate = estimate_token_count(&text);
            }

            if !trimmed_sections.is_empty() {
                tracing::warn!(
                    token_estimate,
                    max_system_prompt_tokens = max_tokens,
                    dropped_sections = ?trimmed_sections,
                    "Trimmed system prompt sections to satisfy token budget"
                );
            }
        } else if warn_enabled {
            tracing::warn!(
                token_estimate,
                max_system_prompt_tokens = max_tokens,
                "System prompt exceeds configured token budget"
            );
        }
    }

    let report = SystemPromptReport {
        token_estimate,
        over_budget: token_estimate > max_tokens,
        trimmed_sections,
    };
    (text, report)
}

/// Hash only the explicitly stable prompt sections.
pub(super) fn stable_prompt_sections_digest(sections: &[PromptSection]) -> u64 {
    let stable_sections = sections
        .iter()
        .filter(|section| section.kind.is_cache_stable())
        .map(|section| (section.kind.name(), section.text.as_str()))
        .collect::<Vec<_>>();
    crate::core::agent::hash_utils::hash_value(&stable_sections)
}

/// Align cached base guidance with the selected role, including switches away from the coordinator.
pub fn apply_coordinator_role_guidance(prompt: &mut String, coordinator_active: bool) {
    use crate::prompts::sections::{SectionBoundaryMode, find_prompt_section_bounds};

    const ORDINARY_DELEGATION: &str = "- Delegate only sizeable, independent work to subagents; keep small tasks and verification in the main thread.";
    const COORDINATOR_DELEGATION: &str = "- As coordinator, delegate execution and verification to scheduler-owned matrix workers; keep decisions in the main thread.";

    if !coordinator_active && !prompt.contains("## Coordinator Role") && !prompt.contains(COORDINATOR_DELEGATION) {
        return;
    }

    let coordinator_role =
        format!("## Coordinator Role\n{}\n", vtcode_config::subagents::builtin_primary_coordinator_agent().prompt);
    if coordinator_active
        && prompt.matches("## Coordinator Role").count() == 1
        && prompt.contains(COORDINATOR_DELEGATION)
        && let Some((start, end)) =
            find_prompt_section_bounds(prompt, "## Coordinator Role", SectionBoundaryMode::BracketOrMarkdown)
        && prompt
            .get(start..end)
            .is_some_and(|section| section.trim() == coordinator_role.trim())
    {
        return;
    }

    while let Some((start, end)) =
        find_prompt_section_bounds(prompt, "## Coordinator Role", SectionBoundaryMode::BracketOrMarkdown)
    {
        prompt.replace_range(start..end, "");
    }
    let (previous, current) = if coordinator_active {
        (ORDINARY_DELEGATION, COORDINATOR_DELEGATION)
    } else {
        (COORDINATOR_DELEGATION, ORDINARY_DELEGATION)
    };
    *prompt = prompt.replace(previous, current);
    if coordinator_active {
        prompt.push_str("\n\n");
        prompt.push_str(&coordinator_role);
    }
}

/// Apply agent identity to the system prompt by replacing the title and intro lines.
/// This combines the "VT Code" identity with the active agent mode so the LLM
/// knows its role (e.g., "VT Code (Build mode)" or "VT Code (Auto mode)").
pub(super) fn apply_agent_identity(prompt: &str, agent_label: &str) -> String {
    let mut result = prompt.to_string();
    let old_title = PROMPT_TITLE;
    let labeled_title = old_title.replacen(PROMPT_IDENTITY_NAME, agent_label, 1);
    let old_intro = PROMPT_INTRO;

    let title_found = if let Some(pos) = result.find(old_title) {
        result.replace_range(pos..pos + old_title.len(), &labeled_title);
        true
    } else {
        warn!("Could not find prompt title '{}' to apply agent identity", old_title);
        false
    };

    let intro_found = if let Some(pos) = result.find(old_intro) {
        let labeled_intro = old_intro.replacen(PROMPT_IDENTITY_NAME, agent_label, 1);
        result.replace_range(pos..pos + old_intro.len(), &labeled_intro);
        true
    } else {
        warn!("Could not find prompt intro '{}' to apply agent identity", old_intro);
        false
    };

    if !title_found || !intro_found {
        warn!(
            agent_label = %agent_label,
            title_replaced = title_found,
            intro_replaced = intro_found,
            "agent identity partially applied"
        );
    }

    result
}

/// Structured reasoning tags are opt-in (`agent.include_structured_reasoning_tags`)
/// in every prompt mode; without a config there is nothing to opt in.
pub(super) fn should_include_structured_reasoning(vtcode_config: Option<&crate::config::VTCodeConfig>) -> bool {
    vtcode_config.is_some_and(|cfg| cfg.agent.should_include_structured_reasoning_tags())
}

/// Generate the stable base system instruction with configuration-aware sections.
///
/// Note: This function maintains backward compatibility by not accepting prompt_context.
/// For enhanced prompts with dynamic guidelines, call `compose_system_instruction_text` directly.
pub async fn generate_system_instruction_with_config(
    config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let (content, _report) =
        generate_system_instruction_with_config_and_report(config, project_root, vtcode_config).await;
    content
}

/// Same as [`generate_system_instruction_with_config`] but also returns the
/// [`SystemPromptReport`] for the composed prompt, whether served from cache
/// or freshly built.
pub async fn generate_system_instruction_with_config_and_report(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> (Content, SystemPromptReport) {
    generate_system_instruction_with_context_and_report(_config, project_root, vtcode_config, None).await
}

/// Generate a system instruction using a context-aware cache identity.
///
/// Context-free and context-aware prompts intentionally use different cache
/// keys. This prevents a prompt assembled with workspace tools or skill
/// metadata from being returned to a caller that requested the static prompt.
pub async fn generate_system_instruction_with_context_and_report(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> (Content, SystemPromptReport) {
    let (built_instruction, built_report, instruction_digest) =
        compose_system_instruction_with_identity(project_root, vtcode_config, prompt_context).await;
    let cache_key = cache_key_for_identity(
        project_root,
        vtcode_config,
        instruction_digest,
        prompt_context_digest(prompt_context),
        0,
    );
    let (instruction, report) = match PROMPT_CACHE.get(&cache_key) {
        Some(cached) => cached,
        None => {
            let built = (built_instruction, built_report);
            PROMPT_CACHE.insert(cache_key, built.clone());
            built
        }
    };

    // Apply output style if configured
    let styled_instruction = apply_output_style(instruction, vtcode_config, project_root).await;
    (Content::system_text(styled_instruction), report)
}

/// Apply output style to a generated system instruction
pub async fn apply_output_style(
    instruction: String,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    project_root: &Path,
) -> String {
    if let Some(config) = vtcode_config {
        let output_style_applier = OutputStyleApplier::new();
        if let Err(e) = output_style_applier.load_styles_from_config(config, project_root).await {
            tracing::warn!("Failed to load output styles: {}", e);
            instruction // Return original if loading fails
        } else {
            output_style_applier
                .apply_style(&config.output_style.active_style, &instruction, config)
                .await
        }
    } else {
        instruction // Return original if no config
    }
}
