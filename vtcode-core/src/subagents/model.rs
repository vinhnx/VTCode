use std::borrow::Cow;

use anyhow::{Context, Result};
use vtcode_config::SubagentSpec;

use crate::config::VTCodeConfig;
use crate::config::constants::models;
use crate::config::models::{ModelId, Provider};
use crate::core::agent::types::AgentType;
use crate::llm::auto_lightweight_model;
use crate::llm::factory::{infer_provider, infer_provider_from_model};

// ─── Model Resolution ───────────────────────────────────────────────────────

pub fn resolve_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    requested: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    let requested = requested.unwrap_or("inherit").trim();
    if requested.eq_ignore_ascii_case("inherit") || requested.is_empty() {
        return resolve_inherit_model(parent_model, parent_provider, agent_name);
    }
    resolve_explicit_model(vt_cfg, parent_provider, parent_model, requested, agent_name)
}

fn resolve_inherit_model(
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
            fallback_model = fallback.as_str(),
            "Falling back to the default Copilot subagent model because the inherited parent model identifier is not supported internally"
        );
        return Ok(fallback);
    }

    let normalized = normalize_subagent_model_alias(parent_model);
    normalized.parse::<ModelId>().with_context(|| {
        format!(
            "Failed to resolve model '{}' for subagent {}",
            normalized, agent_name
        )
    })
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
    } else if matches!(
        requested.to_ascii_lowercase().as_str(),
        "haiku" | "sonnet" | "opus"
    ) {
        alias_model_for_provider(parent_provider, requested, parent_model)
    } else {
        requested.to_string()
    };

    let normalized = normalize_subagent_model_alias(resolved.as_str());
    normalized.parse::<ModelId>().with_context(|| {
        format!(
            "Failed to resolve model '{}' for subagent {}",
            normalized, agent_name
        )
    })
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
    let configured_provider =
        infer_provider_from_model(configured).or_else(|| infer_provider(None, configured));

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

pub fn resolve_effective_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    model_override: Option<&str>,
    spec_model: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    if let Some(requested) = model_override {
        match resolve_subagent_model(
            vt_cfg,
            parent_model,
            parent_provider,
            Some(requested),
            agent_name,
        ) {
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

    match resolve_subagent_model(
        vt_cfg,
        parent_model,
        parent_provider,
        spec_model,
        agent_name,
    ) {
        Ok(model) => Ok(model),
        Err(err)
            if spec_model
                .map(str::trim)
                .is_some_and(|v| v.eq_ignore_ascii_case("small")) =>
        {
            tracing::warn!(
                agent_name,
                error = %err,
                "Failed to resolve lightweight subagent model from spec; falling back to parent model"
            );
            resolve_subagent_model(
                vt_cfg,
                parent_model,
                parent_provider,
                Some("inherit"),
                agent_name,
            )
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
        return resolve_subagent_model(
            vt_cfg,
            parent_model,
            parent_provider,
            Some("inherit"),
            agent_name,
        );
    }
    let fallback = spec_model
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("inherit");
    tracing::warn!(
        agent_name,
        requested_model = requested.trim(),
        fallback_model = fallback,
        error = %err,
        "Failed to resolve subagent model override; falling back"
    );
    Ok(resolve_subagent_model(
        vt_cfg,
        parent_model,
        parent_provider,
        spec_model,
        agent_name,
    )
    .unwrap_or_else(|_| {
        resolve_subagent_model(
            vt_cfg,
            parent_model,
            parent_provider,
            Some("inherit"),
            agent_name,
        )
        .expect("inherit fallback should succeed")
    }))
}

fn normalize_subagent_model_alias(model: &str) -> Cow<'_, str> {
    match model.trim() {
        "claude-haiku-4.5" => Cow::Borrowed(models::anthropic::CLAUDE_HAIKU_4_5),
        "claude-sonnet-4.6" => Cow::Borrowed(models::anthropic::CLAUDE_SONNET_4_6),
        "claude-opus-4.7" => Cow::Borrowed(models::anthropic::CLAUDE_OPUS_4_7),
        other => Cow::Borrowed(other),
    }
}

fn alias_model_for_provider(parent_provider: &str, alias: &str, parent_model: &str) -> String {
    match infer_provider(Some(parent_provider), parent_model) {
        Some(Provider::Anthropic) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::anthropic::CLAUDE_HAIKU_4_5.to_string(),
            "opus" => models::anthropic::CLAUDE_OPUS_4_7.to_string(),
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

pub fn agent_type_for_spec(spec: &SubagentSpec) -> AgentType {
    match spec.name.as_str() {
        "explorer" | "explore" => AgentType::Explore,
        "plan" => AgentType::Plan,
        "worker" | "general" | "general-purpose" | "default" => AgentType::General,
        _ => AgentType::Custom(spec.name.clone()),
    }
}

// ─── Memory Appendix ────────────────────────────────────────────────────────

use super::constants::{
    SUBAGENT_MEMORY_BYTES_LIMIT, SUBAGENT_MEMORY_HIGHLIGHT_LIMIT, SUBAGENT_MEMORY_LINE_LIMIT,
};
use crate::persistent_memory::extract_memory_highlights;
use vtcode_config::SubagentMemoryScope;

pub fn load_memory_appendix(
    workspace_root: &std::path::Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_dir = match scope {
        SubagentMemoryScope::Project => {
            workspace_root.join(".vtcode/agent-memory").join(agent_name)
        }
        SubagentMemoryScope::Local => workspace_root
            .join(".vtcode/agent-memory-local")
            .join(agent_name),
        SubagentMemoryScope::User => dirs::home_dir()
            .unwrap_or_default()
            .join(".vtcode/agent-memory")
            .join(agent_name),
    };
    std::fs::create_dir_all(&memory_dir).with_context(|| {
        format!(
            "Failed to create subagent memory directory {}",
            memory_dir.display()
        )
    })?;
    let memory_file = memory_dir.join("MEMORY.md");
    if !memory_file.exists() {
        return Ok(Some(format!(
            "Persistent memory file: {}. Create or update `MEMORY.md` with concise reusable notes when you discover stable repository conventions.",
            memory_file.display()
        )));
    }

    let content = std::fs::read_to_string(&memory_file)
        .with_context(|| format!("Failed to read {}", memory_file.display()))?;
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

    let excerpt = excerpt_lines.join("\n");
    let truncated = excerpt_lines.len() < total_lines;
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
