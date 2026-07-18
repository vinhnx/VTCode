use anyhow::{Context, Result};
use vtcode_config::SubagentSpec;
use vtcode_config::core::{CustomProviderConfig, ProviderOverrideConfig};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::VTCodeConfig;
use crate::config::constants::models;
use crate::config::models::{ModelId, Provider};
use crate::core::agent::types::AgentType;
use crate::llm::auto_lightweight_model;
use crate::llm::factory::{infer_provider, infer_provider_from_model};

// ─── Model Resolution ───────────────────────────────────────────────────────

/// Resolves the model for a subagent given an optional request override.
///
/// When `requested` is `None`, `"inherit"`, or empty, the parent model is inherited.
/// Special aliases like `"small"`, `"haiku"`, `"sonnet"`, and `"opus"` are mapped
/// to concrete model identifiers based on the active provider.
pub fn resolve_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    requested: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    let requested = requested.unwrap_or("inherit").trim();
    if requested.eq_ignore_ascii_case("inherit") || requested.is_empty() {
        return resolve_inherit_model(vt_cfg, parent_model, parent_provider, agent_name);
    }
    resolve_explicit_model(vt_cfg, parent_provider, parent_model, requested, agent_name)
}

fn resolve_inherit_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    agent_name: &str,
) -> Result<ModelId> {
    if let Ok(model) = parent_model.parse::<ModelId>() {
        return Ok(model);
    }
    if parent_provider.eq_ignore_ascii_case("copilot") {
        let fallback = ModelId::default_orchestrator_for_provider(Provider::Copilot);
        tracing::warn!(
            agent_name,
            parent_model = parent_model.trim(),
            parent_provider = parent_provider.trim(),
            fallback_model = &*fallback.as_str(),
            "Falling back to the default Copilot subagent model because the inherited parent model identifier is not supported internally"
        );
        return Ok(fallback);
    }

    finalize_subagent_model(&RuntimeModelSources::from_config(vt_cfg), parent_model, parent_provider, agent_name)
}

/// Narrow, read-only view of the config-defined runtime model sources that are
/// not part of the built-in [`ModelId`] catalog.
///
/// Isolating these two fields (instead of passing the whole [`VTCodeConfig`])
/// keeps the custom-model fallback policy independently testable and prevents
/// the resolution logic from coupling to unrelated config surface.
struct RuntimeModelSources<'a> {
    provider_overrides: &'a BTreeMap<String, ProviderOverrideConfig>,
    custom_providers: &'a [CustomProviderConfig],
}

impl<'a> RuntimeModelSources<'a> {
    fn from_config(vt_cfg: &'a VTCodeConfig) -> Self {
        Self {
            provider_overrides: &vt_cfg.provider_overrides,
            custom_providers: &vt_cfg.custom_providers,
        }
    }
}

/// Normalizes `raw_model` and resolves it into a [`ModelId`], attaching a
/// consistent subagent context message on failure.
///
/// Shared by the inherit and explicit resolution paths so both apply the same
/// alias normalization, custom-model fallback, and error wording.
fn finalize_subagent_model(
    sources: &RuntimeModelSources<'_>,
    raw_model: &str,
    provider_hint: &str,
    agent_name: &str,
) -> Result<ModelId> {
    let normalized = normalize_subagent_model_alias(raw_model);
    parse_model_or_custom(sources, normalized, provider_hint)
        .with_context(|| format!("Failed to resolve model '{normalized}' for subagent {agent_name}"))
}

/// Parses `model` into a [`ModelId`], falling back to a runtime [`ModelId::Custom`]
/// variant for user-defined providers and local providers.
///
/// Local providers (Ollama, LM Studio, llama.cpp), `[providers.<name>]`
/// overrides, and `[[custom_providers]]` endpoints expose arbitrary model
/// identifiers that are not part of the built-in catalog, so a strict
/// `parse::<ModelId>()` would incorrectly reject valid runtime models. This
/// helper honors those identifiers instead of failing subagent resolution.
///
/// Config-defined models are only honored when they belong to the active
/// provider (`provider_hint`); this preserves the invalid-override fallback for
/// unrelated providers and avoids mis-routing a model to the wrong endpoint.
fn parse_model_or_custom(sources: &RuntimeModelSources<'_>, model: &str, provider_hint: &str) -> Result<ModelId> {
    let trimmed = model.trim();
    if let Ok(parsed) = trimmed.parse::<ModelId>() {
        return Ok(parsed);
    }

    let provider_hint = provider_hint.trim();
    let hinted_provider = provider_hint.parse::<Provider>().ok();

    // Built-in provider overrides (`[providers.<name>]`), scoped to the active provider.
    for (provider_key, override_cfg) in sources.provider_overrides {
        let matches_hint = match hinted_provider {
            Some(active) => provider_key.parse::<Provider>().ok() == Some(active),
            None => provider_key.eq_ignore_ascii_case(provider_hint),
        };
        if matches_hint && override_cfg.models.iter().any(|m| m.trim() == trimmed) {
            return Ok(ModelId::Custom(provider_key.clone(), trimmed.to_string()));
        }
    }

    // Custom OpenAI-compatible providers (`[[custom_providers]]`), matched by key.
    for custom in sources.custom_providers {
        if custom.name.eq_ignore_ascii_case(provider_hint) && custom.effective_models().iter().any(|m| m == trimmed) {
            return Ok(ModelId::Custom(custom.name.to_lowercase(), trimmed.to_string()));
        }
    }

    // Local providers expose arbitrary runtime model identifiers that cannot be
    // validated against the built-in catalog; honor them as custom models.
    if let Some(provider) = hinted_provider.filter(|provider| provider.is_local()) {
        return Ok(ModelId::Custom(provider.to_string(), trimmed.to_string()));
    }

    Ok(trimmed.parse::<ModelId>()?)
}

fn resolve_explicit_model(
    vt_cfg: &VTCodeConfig,
    parent_provider: &str,
    parent_model: &str,
    requested: &str,
    agent_name: &str,
) -> Result<ModelId> {
    let resolved = if requested.eq_ignore_ascii_case("small") {
        resolve_lightweight_model(vt_cfg, parent_provider, parent_model, agent_name)
    } else if matches!(requested.to_ascii_lowercase().as_str(), "haiku" | "sonnet" | "opus") {
        alias_model_for_provider(parent_provider, requested, parent_model)
    } else {
        requested.to_string()
    };

    finalize_subagent_model(&RuntimeModelSources::from_config(vt_cfg), resolved.as_str(), parent_provider, agent_name)
}

fn resolve_lightweight_model(
    vt_cfg: &VTCodeConfig,
    parent_provider: &str,
    parent_model: &str,
    agent_name: &str,
) -> String {
    if vt_cfg.agent.small_model.model.trim().is_empty() {
        return auto_lightweight_model(parent_provider, parent_model);
    }

    let configured = vt_cfg.agent.small_model.model.trim();
    let active_provider = infer_provider(Some(parent_provider), parent_model);
    let configured_provider = infer_provider_from_model(configured).or_else(|| infer_provider(None, configured));

    if configured_provider.is_some() && configured_provider != active_provider {
        tracing::warn!(
            agent_name,
            configured_model = configured,
            active_provider = active_provider
                .map(|provider| provider.to_string())
                .unwrap_or_else(|| parent_provider.to_string()),
            "Ignoring cross-provider lightweight subagent model; using same-provider automatic route"
        );
        auto_lightweight_model(parent_provider, parent_model)
    } else {
        configured.to_string()
    }
}

/// Resolves the effective subagent model, preferring the explicit override, then the spec model,
/// then falling back to the parent model.
pub fn resolve_effective_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    model_override: Option<&str>,
    spec_model: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    if let Some(requested) = model_override {
        match resolve_subagent_model(vt_cfg, parent_model, parent_provider, Some(requested), agent_name) {
            Ok(model) => return Ok(model),
            Err(err) => {
                return handle_model_override_failure(
                    vt_cfg,
                    parent_model,
                    parent_provider,
                    requested,
                    spec_model,
                    agent_name,
                    err,
                );
            }
        }
    }

    match resolve_subagent_model(vt_cfg, parent_model, parent_provider, spec_model, agent_name) {
        Ok(model) => Ok(model),
        Err(err) if spec_model.map(str::trim).is_some_and(|v| v.eq_ignore_ascii_case("small")) => {
            tracing::warn!(
                agent_name,
                error = %err,
                "Failed to resolve lightweight subagent model from spec; falling back to parent model"
            );
            resolve_subagent_model(vt_cfg, parent_model, parent_provider, Some("inherit"), agent_name)
        }
        Err(err) => Err(err),
    }
}

fn handle_model_override_failure(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    requested: &str,
    spec_model: Option<&str>,
    agent_name: &str,
    err: anyhow::Error,
) -> Result<ModelId> {
    if requested.trim().eq_ignore_ascii_case("small") {
        tracing::warn!(
            agent_name,
            requested_model = requested.trim(),
            error = %err,
            "Failed to bootstrap lightweight subagent model; falling back to parent model"
        );
        return resolve_subagent_model(vt_cfg, parent_model, parent_provider, Some("inherit"), agent_name);
    }
    let fallback = spec_model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("inherit");
    tracing::warn!(
        agent_name,
        requested_model = requested.trim(),
        fallback_model = fallback,
        error = %err,
        "Failed to resolve subagent model override; falling back"
    );
    let model = resolve_subagent_model(vt_cfg, parent_model, parent_provider, spec_model, agent_name)
        .or_else(|_| resolve_subagent_model(vt_cfg, parent_model, parent_provider, Some("inherit"), agent_name))?;
    Ok(model)
}

fn normalize_subagent_model_alias(model: &str) -> &str {
    match model.trim() {
        "claude-haiku-4.5" => models::anthropic::CLAUDE_HAIKU_4_5,
        "claude-sonnet-4.6" => models::anthropic::CLAUDE_SONNET_4_6,
        "claude-opus-4.8" => models::anthropic::CLAUDE_OPUS_4_8,
        other => other,
    }
}

fn alias_model_for_provider(parent_provider: &str, alias: &str, parent_model: &str) -> String {
    match infer_provider(Some(parent_provider), parent_model) {
        Some(Provider::Anthropic) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::anthropic::CLAUDE_HAIKU_4_5.to_string(),
            "opus" => models::anthropic::CLAUDE_OPUS_4_8.to_string(),
            _ => models::anthropic::CLAUDE_SONNET_4_6.to_string(),
        },
        Some(Provider::OpenAI) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::openai::GPT_5_4_MINI.to_string(),
            "opus" => models::openai::GPT_5_4.to_string(),
            _ => models::openai::GPT_5_4.to_string(),
        },
        Some(Provider::Gemini) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            _ => models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        },
        _ => parent_model.to_string(),
    }
}

/// Maps a subagent spec name to its corresponding [`AgentType`] variant.
pub fn agent_type_for_spec(spec: &SubagentSpec) -> AgentType {
    match spec.name.as_str() {
        "explorer" | "explore" => AgentType::Explore,
        "plan" => AgentType::Plan,
        "worker" | "general" | "general-purpose" | "default" => AgentType::General,
        _ => AgentType::Custom(spec.name.clone()),
    }
}

// ─── Memory Appendix ────────────────────────────────────────────────────────

use super::constants::{SUBAGENT_MEMORY_BYTES_LIMIT, SUBAGENT_MEMORY_HIGHLIGHT_LIMIT, SUBAGENT_MEMORY_LINE_LIMIT};
use crate::persistent_memory::extract_memory_highlights;
use vtcode_config::SubagentMemoryScope;

/// Loads the persistent memory appendix for a subagent, including key-point highlights.
///
/// Returns `None` when no memory scope is configured. Creates the memory directory
/// if it does not exist, and returns a guidance prompt when the memory file is absent.
pub fn load_memory_appendix(
    workspace_root: &Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_dir = agent_memory_dir(workspace_root, agent_name, scope);
    std::fs::create_dir_all(&memory_dir)
        .with_context(|| format!("Failed to create subagent memory directory {}", memory_dir.display()))?;
    let memory_file = memory_dir.join("MEMORY.md");
    if !memory_file.exists() {
        return Ok(Some(format!(
            "Persistent memory file: {}. Create or update `MEMORY.md` with concise reusable notes when you discover stable repository conventions.",
            memory_file.display()
        )));
    }

    let content =
        std::fs::read_to_string(&memory_file).with_context(|| format!("Failed to read {}", memory_file.display()))?;
    let (excerpt, truncated) = memory_excerpt(&content);
    let highlights = extract_memory_highlights(&excerpt, SUBAGENT_MEMORY_HIGHLIGHT_LIMIT);
    let mut appendix = String::new();
    appendix.push_str(&format!(
        "Persistent memory file: {}.\nRead and maintain `MEMORY.md` for durable learnings.",
        memory_file.display()
    ));

    if !highlights.is_empty() {
        appendix.push_str("\n\nKey points:\n");
        for highlight in highlights {
            appendix.push_str("- ");
            appendix.push_str(&highlight);
            appendix.push('\n');
        }
    }

    appendix.push_str("\nOpen `MEMORY.md` when exact wording or more detail matters.");
    if truncated {
        appendix.push_str("\nMemory indexing stopped after the configured startup budget.");
    }

    Ok(Some(appendix))
}

/// Loads a read-only memory appendix for the primary agent from a subagent's memory scope.
///
/// Unlike [`load_memory_appendix`], this does not create directories or prompt for writes.
pub fn load_primary_memory_appendix(
    workspace_root: &Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_file = agent_memory_dir(workspace_root, agent_name, scope).join("MEMORY.md");
    if !memory_file.exists() {
        return Ok(None);
    }

    let content =
        std::fs::read_to_string(&memory_file).with_context(|| format!("Failed to read {}", memory_file.display()))?;
    let (excerpt, truncated) = memory_excerpt(&content);
    let highlights = extract_memory_highlights(&excerpt, SUBAGENT_MEMORY_HIGHLIGHT_LIMIT);
    let mut appendix = String::new();
    appendix.push_str(&format!(
        "Primary-agent memory file: {}.\nLoaded read-only for this request.",
        memory_file.display()
    ));

    if !highlights.is_empty() {
        appendix.push_str("\n\nKey points:\n");
        for highlight in highlights {
            appendix.push_str("- ");
            appendix.push_str(&highlight);
            appendix.push('\n');
        }
    }

    if truncated {
        appendix.push_str("\nMemory indexing stopped after the configured startup budget.");
    }

    Ok(Some(appendix))
}

fn agent_memory_dir(workspace_root: &Path, agent_name: &str, scope: SubagentMemoryScope) -> PathBuf {
    match scope {
        SubagentMemoryScope::Project => workspace_root.join(".vtcode/agent-memory").join(agent_name),
        SubagentMemoryScope::Local => workspace_root.join(".vtcode/agent-memory-local").join(agent_name),
        SubagentMemoryScope::User => dirs::home_dir()
            .unwrap_or_default()
            .join(".vtcode/agent-memory")
            .join(agent_name),
    }
}

fn memory_excerpt(content: &str) -> (String, bool) {
    let total_lines = content.lines().count();
    let mut bytes = 0usize;
    let mut excerpt_lines = Vec::new();
    for line in content.lines().take(SUBAGENT_MEMORY_LINE_LIMIT) {
        let next_bytes = bytes.saturating_add(line.len() + 1);
        if next_bytes > SUBAGENT_MEMORY_BYTES_LIMIT {
            break;
        }
        bytes = next_bytes;
        excerpt_lines.push(line);
    }

    let truncated = excerpt_lines.len() < total_lines;
    (excerpt_lines.join("\n"), truncated)
}
