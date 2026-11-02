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

    if response.finish_reason != vtcode_core::llm::provider::FinishReason::Stop
        || response.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty())
    {
        bail!("tool failure diagnosis returned an incomplete or tool-call response");
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
    let mut request = LLMRequest {
        messages: Arc::new(vec![LlmMessage::user(prompt)]),
        system_prompt: Some(Arc::from(DIAGNOSIS_SYSTEM_PROMPT)),
        model: route.model.clone(),
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
    request.set_auxiliary_output_budget(provider, DIAGNOSIS_MAX_OUTPUT_TOKENS);

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
    let diagnosis = ToolFailureDiagnosis::new(
        parsed.observed,
        parsed.likely_cause,
        super::evidence::bounded_field(&parsed.next_action),
    );
    (diagnosis.is_complete() && diagnosis.is_safe_model_output()).then_some(diagnosis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use vtcode_core::config::types::ReasoningEffortLevel;
    use vtcode_core::llm::provider::{FinishReason, LLMError, LLMResponse};

    struct DiagnosisProvider {
        finish: FinishReason,
    }

    #[async_trait::async_trait]
    impl LLMProvider for DiagnosisProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_non_streaming(&self, _: &str) -> bool {
            true
        }
        fn supports_reasoning(&self, _: &str) -> bool {
            true
        }
        fn supported_reasoning_efforts(&self, _: &str) -> &'static [&'static str] {
            &["low", "high"]
        }
        async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
            assert_eq!(request.max_tokens, Some(2208));
            assert_eq!(request.reasoning_effort, Some(ReasoningEffortLevel::Low));
            assert_eq!(request.temperature, None);
            assert!(request.tools.is_none());
            Ok(LLMResponse {
                content: Some(r#"{"observed":"exit 127","likely_cause":"command unavailable","next_action":"Check command availability"}"#.into()),
                finish_reason: self.finish.clone(),
                ..Default::default()
            })
        }
        fn supported_models(&self) -> Vec<String> {
            vec![]
        }
        fn validate_request(&self, _: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn diagnosis_reserves_reasoning_and_rejects_truncated_json() {
        for finish in [FinishReason::Stop, FinishReason::Length] {
            let expected_complete = finish == FinishReason::Stop;
            let mut backing = TestTurnProcessingBacking::new(4).await;
            backing.set_provider(Box::new(DiagnosisProvider { finish }));
            let mut ctx = backing.turn_processing_context();
            let route = vtcode_core::llm::ModelRoute {
                provider_name: ctx.config.provider.clone(),
                model: ctx.config.model.clone(),
            };
            let result = diagnose_with_route(&mut ctx, &route, "exit 127: command not found").await;
            if expected_complete {
                let parsed = parse_model_diagnosis(&result.unwrap()).unwrap();
                assert_eq!(parsed.observed, "exit 127");
            } else {
                assert!(result.is_err(), "complete-looking JSON cannot override a truncated finish reason");
            }
        }
    }
}
