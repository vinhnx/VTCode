use super::*;

#[derive(Debug, Deserialize)]
struct MemorySummaryResponse {
    #[serde(default)]
    bullets: Vec<String>,
}

/// Distinguishes memory LLM call phases for model override routing.
#[derive(Debug, Clone, Copy)]
pub(super) enum MemoryPhase {
    /// Per-thread fact extraction and classification.
    Extract,
    /// Global consolidation and summary generation.
    Consolidate,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryModelRoute {
    pub(super) provider_name: String,
    pub(super) model: String,
    pub(super) temperature: f32,
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedMemoryRoutes {
    pub(super) primary: MemoryModelRoute,
    fallback: Option<MemoryModelRoute>,
    pub(super) warning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryClassificationItem {
    id: usize,
    topic: MemoryPlannedTopic,
    #[serde(default)]
    fact: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryClassificationPlan {
    #[serde(default)]
    keep: Vec<MemoryClassificationItem>,
}

fn build_memory_json_request(
    provider: &(impl LLMProvider + ?Sized),
    route: &MemoryModelRoute,
    prompt: String,
    schema_name: &str,
    schema: &serde_json::Value,
) -> Result<LLMRequest> {
    let supports_native_json = provider.supports_structured_output(&route.model);
    let prompt = if supports_native_json {
        prompt
    } else {
        let schema =
            serde_json::to_string_pretty(schema).context("failed to serialize persistent memory JSON schema")?;
        format!(
            "{prompt}\n\nReturn JSON only. Do not add markdown fences or explanatory text. The response must be a single JSON object that matches this schema:\n{schema}"
        )
    };

    Ok(LLMRequest {
        model: route.model.clone(),
        temperature: Some(route.temperature),
        output_format: supports_native_json.then(|| {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "schema": schema,
                }
            })
        }),
        messages: std::sync::Arc::new(vec![Message::user(prompt)]),
        ..Default::default()
    })
}

fn parse_memory_json_response<T>(text: &str, context: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("{context} returned empty content");
    }
    if let Ok(parsed) = serde_json::from_str::<T>(trimmed) {
        return Ok(parsed);
    }
    extract_first_json_block(trimmed)
        .and_then(|json_block| serde_json::from_str::<T>(json_block).ok())
        .with_context(|| format!("failed to parse {context} response"))
}

fn extract_first_json_block(text: &str) -> Option<&str> {
    let (start, opening) = text.char_indices().find(|(_, ch)| matches!(ch, '{' | '['))?;
    let mut stack = vec![opening];
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in text[start + opening.len_utf8()..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' => {
                if stack.pop() != Some('{') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return Some(&text[start..end]);
                }
            }
            _ => {}
        }
    }

    None
}

pub(crate) async fn classify_facts_strict(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    if candidates.is_empty() {
        return Ok(ClassifiedFacts {
            preferences: Vec::new(),
            repository_facts: Vec::new(),
        });
    }
    classify_facts_with_llm(runtime_config, vt_cfg, workspace_root, candidates).await
}

/// Try a memory LLM operation with primary route, falling back to the fallback route on error.
/// This macro expands to the full routing/fallback pattern used by all memory LLM calls.
macro_rules! try_with_memory_routes {
    ($runtime_config:expr, $vt_cfg:expr, $workspace_root:expr, $phase:expr, $provider_fn:expr) => {
        async {
            let __rt_cfg: &RuntimeAgentConfig = $runtime_config;
            let __routes = resolve_memory_model_routes(__rt_cfg, $vt_cfg, $phase);
            log_memory_route_warning(&__routes);

            let __provider = create_memory_provider(&__routes.primary, __rt_cfg, $vt_cfg)?;
            match $provider_fn(__provider.as_ref(), &__routes.primary).await {
                Ok(result) => Ok(result),
                Err(__primary_err) => {
                    let Some(__fallback) = __routes.fallback.as_ref() else {
                        return Err(__primary_err);
                    };

                    tracing::warn!(
                        model = %__routes.primary.model,
                        fallback_model = %__fallback.model,
                        error = %__primary_err,
                        "persistent memory LLM call failed on lightweight route; retrying with main model"
                    );
                    let __provider = create_memory_provider(__fallback, __rt_cfg, $vt_cfg)?;
                    $provider_fn(__provider.as_ref(), __fallback).await
                }
            }
        }
    };
}

async fn classify_facts_with_llm(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    let rt_cfg =
        runtime_config.ok_or_else(|| anyhow!("runtime config is required for persistent memory LLM routing"))?;
    try_with_memory_routes!(rt_cfg, vt_cfg, workspace_root, MemoryPhase::Extract, |provider, route| {
        classify_facts_with_provider(provider, route, workspace_root, candidates)
    })
    .await
}

pub(super) async fn classify_facts_with_provider(
    provider: &(impl LLMProvider + ?Sized),
    route: &MemoryModelRoute,
    workspace_root: &Path,
    candidates: &[GroundedFactRecord],
) -> Result<ClassifiedFacts> {
    let payload = candidates
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            json!({
                "id": index,
                "source": fact.source,
                "fact": fact.fact,
            })
        })
        .collect::<Vec<_>>();

    let schema = json!({
        "type": "object",
        "properties": {
            "keep": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"},
                        "topic": {
                            "type": "string",
                            "enum": ["preferences", "repository_facts"]
                        },
                        "fact": {"type": "string"}
                    },
                    "required": ["id", "topic", "fact"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["keep"],
        "additionalProperties": false
    });
    let request = build_memory_json_request(
        provider,
        route,
        format!(
            "Classify VT Code memory evidence. Keep only durable reusable preferences or repository facts. Rewrite each kept fact into one concise canonical sentence. Drop transient, conversational, or noisy entries by omitting them.\n\nWorkspace: {}\nCandidates:\n{}",
            workspace_root.display(),
            serde_json::to_string_pretty(&payload).context("failed to serialize memory classification payload")?
        ),
        "memory_classification",
        &schema,
    )?;

    let response = collect_single_response(provider, request)
        .await
        .context("persistent memory classification LLM request failed")?;
    let content = response
        .content
        .context("persistent memory classification returned no content")?;
    let parsed =
        parse_memory_json_response::<MemoryClassificationPlan>(content.trim(), "persistent memory classification")?;

    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    for item in parsed.keep {
        let candidate = candidates
            .get(item.id)
            .ok_or_else(|| anyhow!("memory classification referenced unknown candidate id {}", item.id))?;
        let normalized_fact = normalize_whitespace(item.fact.as_deref().unwrap_or(&candidate.fact));
        if normalized_fact.is_empty() || looks_like_legacy_prompt(&normalized_fact) {
            continue;
        }
        let topic = match item.topic {
            MemoryPlannedTopic::Preferences => MemoryTopic::Preferences,
            MemoryPlannedTopic::RepositoryFacts => MemoryTopic::RepositoryFacts,
        };
        let record = GroundedFactRecord {
            fact: truncate_for_fact(&normalized_fact, 180),
            source: {
                let (_existing_topic, display_source) = decode_topic_source(&candidate.source);
                encode_topic_source(topic, &display_source)
            },
        };
        match topic {
            MemoryTopic::Preferences => preferences.push(record),
            MemoryTopic::RepositoryFacts => repository_facts.push(record),
        };
    }

    Ok(ClassifiedFacts { preferences, repository_facts })
}

pub(crate) async fn summarize_memory(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> Option<String> {
    let runtime_config = runtime_config?;
    try_with_memory_routes!(runtime_config, vt_cfg, workspace_root, MemoryPhase::Consolidate, |provider, route| {
        summarize_memory_with_provider(provider, route, workspace_root, preferences, repository_facts, notes)
    })
    .await
    .ok()
}

pub(super) async fn summarize_memory_with_provider(
    provider: &(impl LLMProvider + ?Sized),
    route: &MemoryModelRoute,
    workspace_root: &Path,
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> Result<String> {
    let schema = json!({
        "type": "object",
        "properties": {
            "bullets": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["bullets"],
        "additionalProperties": false
    });
    let request = build_memory_json_request(
        provider,
        route,
        format!(
            "Write a concise VT Code persistent memory summary for startup injection. Return 4-10 short bullets only. Focus on stable preferences, repository facts, and durable user-authored notes.\n\nWorkspace: {}\nPreferences:\n{}\n\nRepository facts:\n{}\n\nNotes:\n{}",
            workspace_root.display(),
            facts_for_prompt(preferences),
            facts_for_prompt(repository_facts),
            notes_for_prompt(notes),
        ),
        "memory_summary",
        &schema,
    )?;

    let response = collect_single_response(provider, request)
        .await
        .context("persistent memory summary LLM request failed")?
        .content
        .context("persistent memory summary returned no content")?;
    let parsed = parse_memory_json_response::<MemorySummaryResponse>(response.trim(), "persistent memory summary")?;
    let bullets = parsed
        .bullets
        .into_iter()
        .map(|bullet| normalize_whitespace(&bullet))
        .filter(|bullet| !bullet.is_empty())
        .take(MEMORY_HIGHLIGHT_LIMIT)
        .collect::<Vec<_>>();
    if bullets.is_empty() {
        bail!("persistent memory summary returned no bullets");
    }

    Ok(render_memory_summary_bullets(&bullets))
}

pub(crate) async fn plan_memory_operation(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    expected_kind: MemoryOpKind,
    request: &str,
    supplemental_answer: Option<&str>,
    candidates: &[MemoryOpCandidate],
) -> Result<MemoryOpPlan> {
    try_with_memory_routes!(runtime_config, vt_cfg, workspace_root, MemoryPhase::Extract, |provider, route| {
        plan_memory_operation_with_provider(
            provider,
            route,
            workspace_root,
            expected_kind.clone(),
            request,
            supplemental_answer,
            candidates,
        )
    })
    .await
}

pub(super) async fn plan_memory_operation_with_provider(
    provider: &(impl LLMProvider + ?Sized),
    route: &MemoryModelRoute,
    workspace_root: &Path,
    expected_kind: MemoryOpKind,
    request: &str,
    supplemental_answer: Option<&str>,
    candidates: &[MemoryOpCandidate],
) -> Result<MemoryOpPlan> {
    let payload =
        serde_json::to_string_pretty(candidates).context("failed to serialize memory operation candidates")?;
    let supplemental = supplemental_answer.unwrap_or("").trim();
    let schema = json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["remember", "forget", "ask_missing", "noop"]
            },
            "facts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "topic": {
                            "type": "string",
                            "enum": ["preferences", "repository_facts"]
                        },
                        "fact": {"type": "string"},
                        "source": {"type": "string"}
                    },
                    "required": ["topic", "fact"],
                    "additionalProperties": false
                }
            },
            "selected_ids": {
                "type": "array",
                "items": {"type": "integer"}
            },
            "missing": {
                "type": ["object", "null"],
                "properties": {
                    "field": {"type": "string"},
                    "prompt": {"type": "string"}
                },
                "required": ["field", "prompt"],
                "additionalProperties": false
            },
            "message": {"type": ["string", "null"]}
        },
        "required": ["kind", "facts", "selected_ids", "missing", "message"],
        "additionalProperties": false
    });
    let llm_request = build_memory_json_request(
        provider,
        route,
        format!(
            "Plan a VT Code persistent memory operation.\n\nExpected operation: {:?}\nWorkspace: {}\nUser request: {}\nSupplemental answer: {}\nCurrent candidates:\n{}\n\nRules:\n- Never echo the raw request back as a saved fact.\n- For remember: extract only durable canonical facts. If a required value is missing, return ask_missing.\n- For forget: choose only ids from Current candidates. Do not invent ids.\n- For ask_missing: include one concise field label and one concise human-facing prompt.\n- For noop: do not include facts or selected ids.\n- Saved facts must be standalone sentences, not imperative prompts.",
            expected_kind,
            workspace_root.display(),
            request.trim(),
            if supplemental.is_empty() {
                "(none)"
            } else {
                supplemental
            },
            payload
        ),
        "memory_operation_plan",
        &schema,
    )?;

    let response = collect_single_response(provider, llm_request)
        .await
        .context("persistent memory planner LLM request failed")?;
    let content = response.content.context("persistent memory planner returned no content")?;
    let plan = parse_memory_json_response::<MemoryOpPlan>(content.trim(), "persistent memory planner")?;
    validate_memory_op_plan(&plan, expected_kind, candidates)?;
    Ok(plan)
}

fn validate_memory_op_plan(
    plan: &MemoryOpPlan,
    expected_kind: MemoryOpKind,
    candidates: &[MemoryOpCandidate],
) -> Result<()> {
    match plan.kind {
        MemoryOpKind::Remember => {
            if expected_kind != MemoryOpKind::Remember {
                bail!("memory planner returned remember for a non-remember request");
            }
            if plan.facts.is_empty() {
                bail!("memory planner returned remember with no facts");
            }
            if plan.facts.iter().any(|f| normalize_whitespace(&f.fact).is_empty()) {
                bail!("memory planner returned an empty fact");
            }
        }
        MemoryOpKind::Forget => {
            if expected_kind != MemoryOpKind::Forget {
                bail!("memory planner returned forget for a non-forget request");
            }
            let valid_ids: BTreeSet<_> = candidates.iter().map(|c| c.id).collect();
            if plan.selected_ids.iter().any(|id| !valid_ids.contains(id)) {
                bail!("memory planner selected an unknown memory candidate");
            }
        }
        MemoryOpKind::AskMissing => {
            let m = plan
                .missing
                .as_ref()
                .ok_or_else(|| anyhow!("memory planner returned ask_missing without a prompt"))?;
            if normalize_whitespace(&m.field).is_empty() || normalize_whitespace(&m.prompt).is_empty() {
                bail!("memory planner returned an incomplete missing-field request");
            }
        }
        MemoryOpKind::Noop => {}
    }
    if matches!(plan.kind, MemoryOpKind::AskMissing | MemoryOpKind::Noop)
        && (!plan.facts.is_empty() || !plan.selected_ids.is_empty())
    {
        bail!("memory planner returned extra mutations for a non-mutating plan");
    }
    Ok(())
}

fn facts_for_prompt(facts: &[GroundedFactRecord]) -> String {
    if facts.is_empty() {
        return "- none".to_string();
    }
    facts
        .iter()
        .map(|f| {
            let (_, s) = decode_topic_source(&f.source);
            format!("- [{}] {}", s, f.fact)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn notes_for_prompt(notes: &[MemoryNoteSummary]) -> String {
    if notes.is_empty() {
        return "- none".to_string();
    }
    notes
        .iter()
        .map(|n| {
            let preview = if n.highlights.is_empty() {
                "no extracted highlights".to_string()
            } else {
                n.highlights.join("; ")
            };
            format!("- [{}] {}", n.relative_path, preview)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn resolve_memory_model_routes(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    phase: MemoryPhase,
) -> ResolvedMemoryRoutes {
    // Check for a phase-specific model override from MemoriesConfig.
    let model_override = vt_cfg.and_then(|cfg| {
        let memories = &cfg.agent.persistent_memory.memories;
        match phase {
            MemoryPhase::Extract => memories.extract_model.as_deref(),
            MemoryPhase::Consolidate => memories.consolidation_model.as_deref(),
        }
    });

    let resolution = resolve_lightweight_route(runtime_config, vt_cfg, LightweightFeature::Memory, model_override);
    let primary = memory_model_route_from_resolution(&resolution.primary, runtime_config, vt_cfg);
    let fallback = resolution
        .fallback
        .as_ref()
        .map(|r| memory_model_route_from_resolution(r, runtime_config, vt_cfg));
    ResolvedMemoryRoutes { primary, fallback, warning: resolution.warning }
}

fn memory_model_route_from_resolution(
    route: &crate::llm::ModelRoute,
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> MemoryModelRoute {
    let temperature = if route.model == runtime_config.model
        && route.provider_name.eq_ignore_ascii_case(&runtime_provider_name(runtime_config))
    {
        0.0
    } else {
        vt_cfg.map(|cfg| cfg.agent.small_model.temperature).unwrap_or(0.0)
    };
    MemoryModelRoute {
        provider_name: route.provider_name.clone(),
        model: route.model.clone(),
        temperature,
    }
}

#[cold]
fn log_memory_route_warning(routes: &ResolvedMemoryRoutes) {
    if let Some(warning) = &routes.warning {
        tracing::warn!(warning = %warning, "persistent memory route adjusted");
    }
}

fn create_memory_provider(
    route: &MemoryModelRoute,
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Box<dyn LLMProvider>> {
    create_provider_for_model_route(
        &crate::llm::ModelRoute {
            provider_name: route.provider_name.clone(),
            model: route.model.clone(),
        },
        runtime_config,
        vt_cfg,
    )
    .context("Failed to initialize persistent memory LLM provider")
}

fn runtime_provider_name(runtime_config: &RuntimeAgentConfig) -> String {
    if !runtime_config.provider.trim().is_empty() {
        return runtime_config.provider.to_lowercase();
    }
    infer_provider_from_model(&runtime_config.model)
        .map(|p| p.to_string().to_lowercase())
        .unwrap_or_else(|| "gemini".to_string())
}
