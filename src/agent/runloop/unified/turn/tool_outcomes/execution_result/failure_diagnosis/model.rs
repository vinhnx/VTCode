//! Lightweight provider routing and strict diagnosis parsing.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use vtcode_core::llm::provider::{LLMProvider, LLMRequest, Message as LlmMessage};
use vtcode_core::llm::{
    LightweightFeature, collect_single_response, create_provider_for_model_route, resolve_lightweight_route,
};

use super::evidence::{escape_untrusted_evidence, safe_error_text};
use super::{
    DIAGNOSIS_MAX_FIELD_BYTES, DIAGNOSIS_MAX_MODEL_RESPONSE_BYTES, DIAGNOSIS_MAX_OUTPUT_TOKENS,
    DIAGNOSIS_SYSTEM_PROMPT, DIAGNOSIS_TIMEOUT, ToolFailureDiagnosis,
};
use crate::agent::runloop::unified::run_loop_context::{DiagnosisMemoEntry, DiagnosisMemoKey};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use vtcode_core::types::CompactStr;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelDiagnosis {
    observed: String,
    likely_cause: String,
    next_action: String,
}

fn diagnosis_memo_key(tool_name: &str, evidence: &str) -> DiagnosisMemoKey {
    DiagnosisMemoKey {
        tool: CompactStr::from(tool_name),
        evidence: evidence.to_string(),
    }
}

pub(super) async fn diagnose_with_optional_model(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    evidence: &str,
    fallback: ToolFailureDiagnosis,
) -> ToolFailureDiagnosis {
    // Fix-verify loops re-hit the same failure shape. Reuse the first
    // diagnosis and stop spending model calls once the per-turn budget is
    // gone — each failure otherwise adds 1-2 hidden LLM round-trips.
    let memo_key = diagnosis_memo_key(tool_name, evidence);
    if let Some(entry) = ctx.harness_state.failure_diagnosis_memo_get(&memo_key) {
        return ToolFailureDiagnosis::new(
            entry.observed.as_str(),
            entry.likely_cause.as_str(),
            entry.next_action.as_str(),
        );
    }
    if !ctx.harness_state.can_spend_failure_diagnosis_model_call() {
        return fallback;
    }

    let resolution = resolve_lightweight_route(ctx.config, ctx.vt_cfg, LightweightFeature::ToolFailureDiagnosis, None);
    if let Some(warning) = &resolution.warning {
        tracing::warn!(warning = %warning, tool = %tool_name, "tool failure diagnosis route adjusted");
    }

    ctx.harness_state.record_failure_diagnosis_model_call();
    match diagnose_with_route(ctx, &resolution.primary, evidence).await {
        Ok(raw) => {
            if let Some(diagnosis) = parse_model_diagnosis(&raw) {
                return store_memoized(ctx.harness_state, memo_key, diagnosis);
            }
            tracing::warn!(tool = %tool_name, "tool failure diagnosis returned invalid or unsafe JSON; using deterministic fallback");
        }
        Err(error) => {
            tracing::warn!(
                tool = %tool_name,
                route_provider = %resolution.primary.provider_name,
                route_model = %resolution.primary.model,
                error = %safe_error_text(&error),
                "tool failure diagnosis failed on lightweight route"
            );
        }
    }

    if let Some(fallback_route) = resolution.fallback.as_ref()
        && ctx.harness_state.can_spend_failure_diagnosis_model_call()
    {
        ctx.harness_state.record_failure_diagnosis_model_call();
        match diagnose_with_route(ctx, fallback_route, evidence).await {
            Ok(raw) => {
                if let Some(diagnosis) = parse_model_diagnosis(&raw) {
                    return store_memoized(ctx.harness_state, memo_key, diagnosis);
                }
                tracing::warn!(tool = %tool_name, "tool failure diagnosis fallback returned invalid or unsafe JSON");
            }
            Err(error) => {
                tracing::warn!(
                    tool = %tool_name,
                    route_provider = %fallback_route.provider_name,
                    route_model = %fallback_route.model,
                    error = %safe_error_text(&error),
                    "tool failure diagnosis fallback failed"
                );
            }
        }
    }

    // Memoize the deterministic fallback too: a repeated identical failure
    // must not re-spend model calls after the routes already failed once.
    store_memoized(ctx.harness_state, memo_key, fallback)
}

fn store_memoized(
    state: &mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    memo_key: DiagnosisMemoKey,
    diagnosis: ToolFailureDiagnosis,
) -> ToolFailureDiagnosis {
    state.failure_diagnosis_memo_put(
        memo_key,
        DiagnosisMemoEntry {
            observed: CompactStr::from(diagnosis.observed.as_str()),
            likely_cause: CompactStr::from(diagnosis.likely_cause.as_str()),
            next_action: CompactStr::from(diagnosis.next_action.as_str()),
        },
    );
    diagnosis
}

async fn diagnose_with_route(
    ctx: &mut TurnProcessingContext<'_>,
    route: &vtcode_core::llm::ModelRoute,
    evidence: &str,
) -> Result<String> {
    let same_runtime_provider = !ctx.config.provider.trim().is_empty()
        && route.provider_name.eq_ignore_ascii_case(ctx.config.provider.as_str())
        && route.model == ctx.config.model;
    let response = tokio::time::timeout(DIAGNOSIS_TIMEOUT, async {
        if same_runtime_provider {
            request_diagnosis_with_provider(ctx.provider_client.as_ref(), route, evidence).await
        } else {
            let provider = create_provider_for_model_route(route, ctx.config, ctx.vt_cfg)?;
            request_diagnosis_with_provider(provider.as_ref(), route, evidence).await
        }
    })
    .await
    .context("tool failure diagnosis request timed out")?
    .context("tool failure diagnosis request failed")?;

    if response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
        bail!("tool failure diagnosis unexpectedly returned tool calls");
    }

    Ok(response.content_text().trim().to_string())
}

async fn request_diagnosis_with_provider(
    provider: &(impl LLMProvider + ?Sized),
    route: &vtcode_core::llm::ModelRoute,
    evidence: &str,
) -> Result<vtcode_core::llm::provider::LLMResponse> {
    let prompt = diagnosis_prompt(evidence);
    let schema = diagnosis_json_schema();
    let supports_native_json = provider.supports_structured_output(&route.model);
    let prompt = if supports_native_json {
        prompt
    } else {
        format!(
            "{prompt}\n\nReturn JSON only. Do not add markdown fences or explanatory text. The response must be a single JSON object that matches this schema:\n{}",
            serde_json::to_string(&schema).unwrap_or_else(|_| "{}".to_string())
        )
    };
    let request = LLMRequest {
        messages: Arc::new(vec![LlmMessage::user(prompt)]),
        system_prompt: Some(Arc::from(DIAGNOSIS_SYSTEM_PROMPT)),
        model: route.model.clone(),
        max_tokens: Some(DIAGNOSIS_MAX_OUTPUT_TOKENS),
        // Omit sampling controls so the auxiliary request remains valid for
        // models that reject explicit temperature/top-p/top-k parameters.
        temperature: None,
        stream: false,
        output_format: supports_native_json.then(|| {
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "tool_failure_diagnosis",
                    "schema": schema,
                    "strict": true,
                }
            })
        }),
        ..Default::default()
    };

    collect_single_response(provider, request)
        .await
        .context("lightweight provider diagnosis request failed")
}

pub(super) fn diagnosis_prompt(evidence: &str) -> String {
    let escaped_evidence = escape_untrusted_evidence(evidence);
    format!(
        "Analyze only the bounded result below.\n\n<untrusted_tool_evidence>\n{escaped_evidence}\n</untrusted_tool_evidence>"
    )
}

pub(super) fn diagnosis_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "observed": {"type": "string", "maxLength": DIAGNOSIS_MAX_FIELD_BYTES},
            "likely_cause": {"type": "string", "maxLength": DIAGNOSIS_MAX_FIELD_BYTES},
            "next_action": {"type": "string", "maxLength": DIAGNOSIS_MAX_FIELD_BYTES},
        },
        "required": ["observed", "likely_cause", "next_action"],
    })
}

pub(super) fn parse_model_diagnosis(raw: &str) -> Option<ToolFailureDiagnosis> {
    if raw.len() > DIAGNOSIS_MAX_MODEL_RESPONSE_BYTES {
        return None;
    }
    let parsed = serde_json::from_str::<ModelDiagnosis>(raw.trim()).ok()?;
    let diagnosis = ToolFailureDiagnosis::new(parsed.observed, parsed.likely_cause, parsed.next_action);
    (diagnosis.is_complete() && diagnosis.is_safe_model_output()).then_some(diagnosis)
}
