//! Prompt-cache key construction and context/config digests.

use std::path::Path;

use crate::prompts::context::PromptContext;
#[cfg(test)]
use crate::prompts::static_prompts::static_profile_prompt;
use crate::skills::model::SkillScope;

/// Build a cache key for the system prompt.
///
/// `catalog_epoch` is the tool-catalog version at the time of the request. When
/// the tool set changes (e.g. planning workflow is toggled, MCP tools are refreshed), the
/// epoch advances and the old cached prompt is superseded rather than served stale.
#[cfg(test)]
pub(super) fn cache_key(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    catalog_epoch: u64,
) -> String {
    let mode = vtcode_config.map(|cfg| cfg.agent.system_prompt_mode).unwrap_or_default();
    let instruction_digest =
        crate::core::agent::hash_utils::hash_value(&("context-free", format!("{mode:?}"), static_profile_prompt(mode)));
    cache_key_for_identity(project_root, vtcode_config, instruction_digest, 0, catalog_epoch)
}

/// Construct one cache key from independent prompt, context, configuration,
/// provider-capability, and catalog-epoch digests.
pub(super) fn cache_key_for_identity(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    instruction_digest: u64,
    context_digest: u64,
    catalog_epoch: u64,
) -> String {
    let config_digest = prompt_config_digest(vtcode_config);
    let capability_digest = vtcode_config
        .map(|cfg| {
            let catalog = crate::config::models::model_catalog_entry(&cfg.agent.provider, &cfg.agent.default_model);
            crate::core::agent::hash_utils::PromptCapabilityIdentity::from_catalog(
                &cfg.agent.provider,
                &cfg.agent.default_model,
                Some(cfg.agent.reasoning_effort),
                0,
                catalog,
            )
            .digest()
        })
        .unwrap_or_else(|| {
            crate::core::agent::hash_utils::PromptCapabilityIdentity::from_catalog("default", "default", None, 0, None)
                .digest()
        });
    let catalog_epoch_digest = crate::core::agent::hash_utils::hash_value(&catalog_epoch);
    let project_digest = crate::core::agent::hash_utils::hash_value(&project_root.to_string_lossy().as_ref());

    format!(
        "sys_prompt:{project_digest:016x}:{instruction_digest:016x}:{config_digest:016x}:{context_digest:016x}:cap{capability_digest:016x}:catalog{catalog_epoch_digest:016x}"
    )
}

/// Digest configuration fields that affect prompt text or budget behavior.
pub(super) fn prompt_config_digest(vtcode_config: Option<&crate::config::VTCodeConfig>) -> u64 {
    let Some(cfg) = vtcode_config else {
        return crate::core::agent::hash_utils::hash_value(&"default-config");
    };

    use std::hash::{Hash, Hasher};
    let mut hasher = crate::core::agent::hash_utils::StableHasher::new();
    cfg.agent.provider.hash(&mut hasher);
    cfg.agent.default_model.hash(&mut hasher);
    format!("{:?}", cfg.agent.reasoning_effort).hash(&mut hasher);
    cfg.agent.include_working_directory.hash(&mut hasher);
    cfg.agent.include_temporal_context.hash(&mut hasher);
    cfg.agent.temporal_context_use_utc.hash(&mut hasher);
    cfg.agent.include_structured_reasoning_tags.hash(&mut hasher);
    format!("{:?}", cfg.agent.system_prompt_mode).hash(&mut hasher);
    format!("{:?}", cfg.agent.tool_documentation_mode).hash(&mut hasher);
    format!("{:?}", cfg.agent.shell_prompt_profile).hash(&mut hasher);
    cfg.agent.max_system_prompt_tokens.hash(&mut hasher);
    cfg.agent.system_prompt_budget_warning.hash(&mut hasher);
    cfg.agent.trim_system_prompt.hash(&mut hasher);
    cfg.chat.ask_questions.enabled.hash(&mut hasher);
    cfg.mcp.enabled.hash(&mut hasher);
    cfg.prompt_cache.cache_friendly_prompt_shaping.hash(&mut hasher);
    cfg.default_primary_agent.hash(&mut hasher);
    hasher.finish()
}

/// Digest the prompt-bearing parts of a context without relying on pointer or
/// insertion order identity. Vectors are sorted because discovery order is not
/// a semantic part of the rendered prompt contract.
pub(super) fn prompt_context_digest(prompt_context: Option<&PromptContext>) -> u64 {
    let Some(context) = prompt_context else {
        return crate::core::agent::hash_utils::hash_value(&"context-free");
    };

    let languages = sorted_refs(&context.languages);
    let tools = sorted_refs(&context.available_tools);
    let skills = sorted_refs(&context.available_skills);
    let mut metadata = context
        .available_skill_metadata
        .iter()
        .map(|skill| SkillMetadataIdentity {
            name: &skill.name,
            description: &skill.description,
            short_description: skill.short_description.as_deref(),
            path: &skill.path,
            scope: skill.scope,
            manifest: skill.manifest.as_ref().map(|manifest| format!("{manifest:?}")),
        })
        .collect::<Vec<_>>();
    metadata.sort_by(|left, right| left.name.cmp(right.name).then_with(|| left.path.cmp(right.path)));

    let preferences = context.user_preferences.as_ref().map(|preferences| PreferencesIdentity {
        preferred_languages: sorted_refs(&preferences.preferred_languages),
        coding_style: preferences.coding_style.as_deref(),
        preferred_frameworks: sorted_refs(&preferences.preferred_frameworks),
    });

    crate::core::agent::hash_utils::hash_value(&(
        context.workspace.as_ref(),
        languages,
        context.project_type.as_deref(),
        tools,
        skills,
        metadata,
        preferences,
        context.capability_level.map(|level| format!("{level:?}")),
        context.current_directory.as_ref(),
    ))
}

// Field order preserves the former tuple-based hash representation. Borrowed
// strings and paths hash like their owned counterparts; manifest Debug text
// stays owned to retain the existing identity format.
#[derive(Hash)]
struct SkillMetadataIdentity<'a> {
    name: &'a str,
    description: &'a str,
    short_description: Option<&'a str>,
    path: &'a Path,
    scope: SkillScope,
    manifest: Option<String>,
}

#[derive(Hash)]
struct PreferencesIdentity<'a> {
    preferred_languages: Vec<&'a String>,
    coding_style: Option<&'a str>,
    preferred_frameworks: Vec<&'a String>,
}

fn sorted_refs<T: Ord>(values: &[T]) -> Vec<&T> {
    let mut sorted = values.iter().collect::<Vec<_>>();
    sorted.sort();
    sorted
}

#[cfg(test)]
mod tests;
