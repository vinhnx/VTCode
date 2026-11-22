use anyhow::{Context, Result};
use vtcode_config::SubagentSpec;
use vtcode_config::core::{CustomProviderConfig, ProviderOverrideConfig};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::VTCodeConfig;
use crate::config::constants::models;
use crate::config::models::{ModelId, Provider};
use crate::core::agent::types::AgentType;
use crate::llm::auto_lightweight_model;
use crate::llm::factory::{infer_provider, infer_provider_from_model};
use crate::prompts::resource_cache::{ResourceCache, canonical_cache_path, fingerprint_files};
use vtcode_commons::VtCodePaths;

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
    let sources = RuntimeModelSources::from_config(vt_cfg);
    // Use the same identity rule as the CLI's ModelId::from_config so a parent
    // and its inheriting subagent cannot disagree about the active route.
    if let Ok(model) =
        ModelId::from_config(parent_model, parent_provider, sources.provider_overrides, sources.custom_providers)
    {
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

    finalize_subagent_model(&sources, parent_model, parent_provider, agent_name)
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
    let provider_hint = provider_hint.trim();
    let hinted_provider = provider_hint.parse::<Provider>().ok();
    let declared = declared_custom_model(sources, trimmed, provider_hint, hinted_provider);
    let catalog = trimmed.parse::<ModelId>().ok();

    // An explicit declaration by the active provider wins when the catalog
    // would attribute the slug to a different provider, matching both
    // ModelId::from_config and the lightweight-routing rule.
    if let Some(declared) = &declared
        && catalog.as_ref().is_none_or(|parsed| Some(parsed.provider()) != hinted_provider)
    {
        return Ok(declared.clone());
    }
    if let Some(parsed) = catalog {
        return Ok(parsed);
    }
    if let Some(declared) = declared {
        return Ok(declared);
    }

    // Local providers expose arbitrary runtime model identifiers that cannot be
    // validated against the built-in catalog; honor them as custom models.
    if let Some(provider) = hinted_provider.filter(|provider| provider.is_local()) {
        return Ok(ModelId::Custom(provider.to_string(), trimmed.to_string()));
    }

    Ok(trimmed.parse::<ModelId>()?)
}

/// Find a model explicitly declared by the active provider, using the same
/// scoped matching rules as `ModelId::from_config`.
fn declared_custom_model(
    sources: &RuntimeModelSources<'_>,
    trimmed: &str,
    provider_hint: &str,
    hinted_provider: Option<Provider>,
) -> Option<ModelId> {
    // Built-in provider overrides (`[providers.<name>]`), scoped to the active provider.
    for (provider_key, override_cfg) in sources.provider_overrides {
        let matches_hint = match hinted_provider {
            Some(active) => provider_key.parse::<Provider>().ok() == Some(active),
            None => provider_key.eq_ignore_ascii_case(provider_hint),
        };
        if matches_hint && override_cfg.models.iter().any(|m| m.trim() == trimmed) {
            return Some(ModelId::Custom(provider_key.clone(), trimmed.to_string()));
        }
    }

    // Custom OpenAI-compatible providers (`[[custom_providers]]`), matched by key.
    for custom in sources.custom_providers {
        if custom.name.eq_ignore_ascii_case(provider_hint) && custom.effective_models().iter().any(|m| m == trimmed) {
            return Some(ModelId::Custom(custom.name.to_lowercase(), trimmed.to_string()));
        }
    }
    None
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

    // A custom endpoint may deliberately serve a model whose slug looks like a
    // built-in provider's (e.g. a gateway exposing `gpt-5.6-luna`). Its explicit
    // declaration is authoritative — the same rule `route_for_candidate` uses
    // for every other lightweight route — so provider inference must not reject it.
    let is_declared_custom_model = vt_cfg
        .custom_provider(parent_provider)
        .is_some_and(|provider| provider.effective_models().iter().any(|model| model == configured));

    if configured_provider.is_some() && configured_provider != active_provider && !is_declared_custom_model {
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
        "claude-haiku-4.5" => models::anthropic::CLAUDE_SONNET_5,
        "claude-sonnet-4.6" => models::anthropic::CLAUDE_SONNET_5,
        "claude-opus-4.8" => models::anthropic::CLAUDE_OPUS_5,
        other => other,
    }
}

fn alias_model_for_provider(parent_provider: &str, alias: &str, parent_model: &str) -> String {
    match infer_provider(Some(parent_provider), parent_model) {
        Some(Provider::Anthropic) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::anthropic::CLAUDE_SONNET_5.to_string(),
            "opus" => models::anthropic::CLAUDE_OPUS_5.to_string(),
            _ => models::anthropic::CLAUDE_SONNET_5.to_string(),
        },
        Some(Provider::OpenAI) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::openai::GPT_5_6_LUNA.to_string(),
            "opus" => models::openai::GPT_5_6_SOL.to_string(),
            _ => models::openai::GPT_5_6_SOL.to_string(),
        },
        Some(Provider::Gemini) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            _ => models::google::GEMINI_3_7_FLASH.to_string(),
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

    let memory_dir = agent_memory_dir(workspace_root, agent_name, scope)?;
    VtCodePaths::ensure_user_dir(&memory_dir)
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
    Ok(Some(render_memory_appendix(&memory_file, &content)))
}

/// Async variant of [`load_memory_appendix`] for child-agent prompt assembly.
///
/// Directory creation, existence checks, and file reads use Tokio's filesystem
/// boundary so launching a child agent does not block an executor worker.
pub async fn load_memory_appendix_async(
    workspace_root: &Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_dir = agent_memory_dir(workspace_root, agent_name, scope)?;
    VtCodePaths::ensure_user_dir(&memory_dir)
        .with_context(|| format!("Failed to create subagent memory directory {}", memory_dir.display()))?;
    let memory_file = memory_dir.join("MEMORY.md");
    if !tokio::fs::try_exists(&memory_file).await.unwrap_or(false) {
        return Ok(Some(format!(
            "Persistent memory file: {}. Create or update `MEMORY.md` with concise reusable notes when you discover stable repository conventions.",
            memory_file.display()
        )));
    }

    let content = tokio::fs::read_to_string(&memory_file)
        .await
        .with_context(|| format!("Failed to read {}", memory_file.display()))?;
    Ok(Some(render_memory_appendix(&memory_file, &content)))
}

fn render_memory_appendix(memory_file: &Path, content: &str) -> String {
    let (excerpt, truncated) = memory_excerpt(content);
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

    appendix
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

    load_primary_memory_appendix_cached(agent_memory_dir(workspace_root, agent_name, scope)?.join("MEMORY.md"))
}

/// Async variant of [`load_primary_memory_appendix`] for prompt assembly.
/// Cache misses, metadata scans, and memory parsing run on Tokio's blocking
/// pool so an interactive request never monopolizes a runtime worker with
/// synchronous filesystem work.
pub async fn load_primary_memory_appendix_async(
    workspace_root: &Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_file = agent_memory_dir(workspace_root, agent_name, scope)?.join("MEMORY.md");
    tokio::task::spawn_blocking(move || load_primary_memory_appendix_cached(memory_file))
        .await
        .context("primary-agent memory cache task failed")?
}

fn load_primary_memory_appendix_cached(memory_file: PathBuf) -> Result<Option<String>> {
    let cache_key = canonical_cache_path(&memory_file);
    if let Some(cached) = primary_memory_cache().fast_get(&cache_key) {
        return Ok((*cached).clone());
    }

    primary_memory_cache().with_load_gate(|| {
        if let Some(cached) = primary_memory_cache().fast_get(&cache_key) {
            return Ok((*cached).clone());
        }

        let fingerprint = fingerprint_files(&[memory_file.as_path()]);
        if let Some(cached) = primary_memory_cache().get_if_unchanged(&cache_key, &fingerprint) {
            return Ok((*cached).clone());
        }

        let appendix = load_primary_memory_appendix_uncached(&memory_file)?;
        primary_memory_cache().insert(cache_key, appendix.clone(), fingerprint);
        Ok(appendix)
    })
}

fn load_primary_memory_appendix_uncached(memory_file: &Path) -> Result<Option<String>> {
    let content = match std::fs::read_to_string(memory_file) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", memory_file.display()));
        }
    };
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

static PRIMARY_MEMORY_APPENDIX_CACHE: OnceLock<ResourceCache<PathBuf, Option<String>>> = OnceLock::new();

fn primary_memory_cache() -> &'static ResourceCache<PathBuf, Option<String>> {
    PRIMARY_MEMORY_APPENDIX_CACHE.get_or_init(ResourceCache::default)
}

fn agent_memory_dir(workspace_root: &Path, agent_name: &str, scope: SubagentMemoryScope) -> Result<PathBuf> {
    match scope {
        SubagentMemoryScope::Project => Ok(workspace_root.join(".vtcode/agent-memory").join(agent_name)),
        SubagentMemoryScope::Local => Ok(workspace_root.join(".vtcode/agent-memory-local").join(agent_name)),
        SubagentMemoryScope::User => VtCodePaths::resolve()
            .and_then(|paths| paths.state_path("agent-memory"))
            .map(|path| path.join(agent_name))
            .context("could not resolve VT Code state directory for user subagent memory"),
    }
}

fn memory_excerpt(content: &str) -> (String, bool) {
    let total_lines = content.lines().count();
    let mut bytes = 0usize;
    let mut excerpt_lines = Vec::new();
    for line in content.lines().take(SUBAGENT_MEMORY_LINE_LIMIT) {
        // Only lines already accepted contribute a separator in `join("\n")`;
        // counting a trailing newline for every line overcharges by one byte
        // and can drop a line that actually fits the budget.
        let separator = usize::from(!excerpt_lines.is_empty());
        let next_bytes = bytes.saturating_add(separator).saturating_add(line.len());
        if next_bytes > SUBAGENT_MEMORY_BYTES_LIMIT {
            break;
        }
        bytes = next_bytes;
        excerpt_lines.push(line);
    }

    let truncated = excerpt_lines.len() < total_lines;
    (excerpt_lines.join("\n"), truncated)
}

#[cfg(test)]
mod memory_cache_tests {
    use super::{load_primary_memory_appendix, load_primary_memory_appendix_async, primary_memory_cache};
    use tempfile::TempDir;
    use vtcode_config::SubagentMemoryScope;

    fn memory_file(workspace: &TempDir) -> std::path::PathBuf {
        workspace.path().join(".vtcode/agent-memory/reviewer/MEMORY.md")
    }

    #[test]
    #[serial_test::serial]
    fn primary_memory_cache_refreshes_edits_and_missing_files() {
        primary_memory_cache().clear();
        let workspace = TempDir::new().expect("workspace");

        assert!(
            load_primary_memory_appendix(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project))
                .expect("missing memory")
                .is_none()
        );

        let path = memory_file(&workspace);
        std::fs::create_dir_all(path.parent().expect("memory parent")).expect("memory directory");
        std::fs::write(&path, "# Reviewer Memory\n\n- old guidance\n").expect("old memory");
        primary_memory_cache().force_metadata_poll();
        let first = load_primary_memory_appendix(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("first memory")
            .expect("memory appendix");
        assert!(first.contains("old guidance"));

        std::fs::write(&path, "# Reviewer Memory\n\n- new guidance\n").expect("new memory");
        primary_memory_cache().force_metadata_poll();
        let second = load_primary_memory_appendix(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("updated memory")
            .expect("updated appendix");
        assert!(second.contains("new guidance"));
    }

    #[test]
    #[serial_test::serial]
    fn primary_memory_cache_honors_ttl_refresh() {
        primary_memory_cache().clear();
        let workspace = TempDir::new().expect("workspace");
        let path = memory_file(&workspace);
        std::fs::create_dir_all(path.parent().expect("memory parent")).expect("memory directory");
        std::fs::write(&path, "# Reviewer Memory\n\n- old guidance\n").expect("old memory");

        let first = load_primary_memory_appendix(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("first memory")
            .expect("memory appendix");
        assert!(first.contains("old guidance"));
        std::fs::write(&path, "# Reviewer Memory\n\n- new guidance\n").expect("new memory");
        primary_memory_cache().force_expiration();
        let second = load_primary_memory_appendix(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("expired memory")
            .expect("expired appendix");
        assert!(second.contains("new guidance"));
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn concurrent_primary_memory_misses_return_identical_appendices() {
        primary_memory_cache().clear();
        let workspace = TempDir::new().expect("workspace");
        let path = memory_file(&workspace);
        std::fs::create_dir_all(path.parent().expect("memory parent")).expect("memory directory");
        std::fs::write(&path, "# Reviewer Memory\n\n- shared guidance\n").expect("memory");

        let (first, second) = tokio::join!(
            load_primary_memory_appendix_async(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project)),
            load_primary_memory_appendix_async(workspace.path(), "reviewer", Some(SubagentMemoryScope::Project)),
        );
        assert_eq!(first.expect("first appendix"), second.expect("second appendix"));
    }
}

#[cfg(test)]
mod memory_excerpt_tests {
    use super::{SUBAGENT_MEMORY_BYTES_LIMIT, SUBAGENT_MEMORY_LINE_LIMIT, memory_excerpt};

    #[test]
    fn byte_budget_counts_join_separators_not_trailing_newlines() {
        let separators = SUBAGENT_MEMORY_LINE_LIMIT - 1;
        let short_line_len = 127;
        let last_line_len = SUBAGENT_MEMORY_BYTES_LIMIT - separators - short_line_len * separators;
        let mut lines = vec!["x".repeat(short_line_len); separators];
        lines.push("y".repeat(last_line_len));
        let content = lines.join("\n");
        assert_eq!(content.len(), SUBAGENT_MEMORY_BYTES_LIMIT);

        let (excerpt, truncated) = memory_excerpt(&content);
        assert_eq!(excerpt, content, "a byte-exact fit must not lose its last line");
        assert!(!truncated);

        let one_byte_over = format!("{content}z");
        let (excerpt, truncated) = memory_excerpt(&one_byte_over);
        assert_eq!(excerpt.lines().count(), separators);
        assert!(truncated);
    }
}

#[cfg(test)]
mod lightweight_model_tests {
    use super::{CustomProviderConfig, VTCodeConfig, resolve_lightweight_model};

    fn config_with_gateway_declaring(declared_models: &[&str]) -> VTCodeConfig {
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.model = "gpt-5.6-luna".to_string();
        vt_cfg.custom_providers.push(CustomProviderConfig {
            name: "my-gateway".to_string(),
            models: declared_models.iter().map(|model| (*model).to_string()).collect(),
            ..Default::default()
        });
        vt_cfg
    }

    #[test]
    fn declared_custom_provider_small_model_wins_over_builtin_provider_inference() {
        let vt_cfg = config_with_gateway_declaring(&["gpt-5.6-luna"]);

        assert_eq!(resolve_lightweight_model(&vt_cfg, "my-gateway", "auto", "reviewer"), "gpt-5.6-luna");
    }

    #[test]
    fn undeclared_cross_provider_small_model_still_falls_back() {
        let vt_cfg = config_with_gateway_declaring(&["other-model"]);

        let resolved = resolve_lightweight_model(&vt_cfg, "my-gateway", "auto", "reviewer");
        assert_ne!(resolved, "gpt-5.6-luna", "an undeclared model must keep the cross-provider guard");
    }
}

#[cfg(test)]
mod model_identity_tests {
    use super::*;

    fn gateway_config(declared: &[&str]) -> VTCodeConfig {
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.custom_providers.push(CustomProviderConfig {
            name: "my-gateway".to_string(),
            models: declared.iter().map(|model| (*model).to_string()).collect(),
            ..Default::default()
        });
        vt_cfg
    }

    #[test]
    fn declared_catalog_slug_keeps_active_provider_identity() {
        let vt_cfg = gateway_config(&["gpt-5.6-sol"]);
        let sources = RuntimeModelSources::from_config(&vt_cfg);

        let parsed = parse_model_or_custom(&sources, "gpt-5.6-sol", "my-gateway").unwrap();
        assert_eq!(parsed, ModelId::Custom("my-gateway".to_string(), "gpt-5.6-sol".to_string()));

        // Control: the same slug without an active declaration stays catalog.
        let catalog = parse_model_or_custom(&sources, "gpt-5.6-sol", "unrelated-gateway").unwrap();
        assert_eq!(catalog, ModelId::GPT56Sol);
    }

    #[test]
    fn inherited_parent_model_agrees_with_cli_resolution() {
        let vt_cfg = gateway_config(&["gpt-5.6-sol"]);
        let inherited = resolve_inherit_model(&vt_cfg, "gpt-5.6-sol", "my-gateway", "reviewer").unwrap();
        let cli =
            ModelId::from_config("gpt-5.6-sol", "my-gateway", &vt_cfg.provider_overrides, &vt_cfg.custom_providers)
                .unwrap();

        assert_eq!(inherited, cli);
        assert_eq!(inherited, ModelId::Custom("my-gateway".to_string(), "gpt-5.6-sol".to_string()));
    }

    #[test]
    fn same_provider_declaration_keeps_catalog_identity() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "openai".to_string(),
            ProviderOverrideConfig {
                models: vec!["gpt-5.6-sol".to_string()],
                base_url: None,
                api_key_env: None,
            },
        );
        let sources = RuntimeModelSources {
            provider_overrides: &overrides,
            custom_providers: &[],
        };

        // Declaring a catalog slug under its own provider must not degrade the
        // picker's variant-only metadata (display name, generation sort, tiers).
        let parsed = parse_model_or_custom(&sources, "gpt-5.6-sol", "openai").unwrap();
        assert_eq!(parsed, ModelId::GPT56Sol);
    }
}
