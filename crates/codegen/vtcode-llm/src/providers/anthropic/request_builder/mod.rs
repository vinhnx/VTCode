//! Request building for Anthropic Claude API.

mod hardening;
mod messages;
mod system;
mod thinking;
pub(crate) mod tools;

use crate::provider::{
    AnthropicOptionalStringOverride, AnthropicOptionalU32Override, AnthropicThinkingConfig, LLMError, LLMRequest,
};
use crate::providers::anthropic_types::{
    AnthropicAdvisorCaching, AnthropicAdvisorTool, AnthropicFallbackParam, AnthropicFallbacksKeyword,
    AnthropicFallbacksParam, AnthropicOutputConfig, AnthropicOutputFormat, AnthropicRequest, AnthropicTaskBudget,
    AnthropicTool, CacheControl, ThinkingConfig, ThinkingDisplay,
};
use serde_json::{Value, json};
use vtcode_config::constants::reasoning;
use vtcode_config::core::{
    AdvisorConfig, AnthropicConfig, AnthropicFallbackMode, AnthropicFallbacks, AnthropicPromptCacheSettings,
};
use vtcode_config::types::ReasoningEffortLevel;

use super::capabilities::{
    default_effort_for_model, default_max_tokens_for_model, effort_allowed_for_model, preserves_thinking_across_turns,
    rejects_forced_tool_choice, rejects_sampling, resolve_model_name, supports_assistant_prefill, supports_effort,
    supports_mid_conversation_system_messages, supports_server_side_fallback, supports_task_budget, thinking_is_on,
};
use super::prompt_cache::{get_tools_cache_ttl, messages_cache_ttl_for_profile};
use messages::{build_messages, hoist_largest_user_message};
use system::{HistorySystemPlacement, SystemPromptBuildResult, build_system_prompt};
use thinking::build_thinking_config;
pub(crate) use thinking::rewrite_thinking_for_model;
use tools::{build_tool_choice, build_tools};

#[cfg(test)]
pub(crate) use messages::tool_result_blocks;

pub(crate) struct RequestBuilderContext<'a> {
    pub(crate) prompt_cache_enabled: bool,
    pub(crate) prompt_cache_settings: &'a AnthropicPromptCacheSettings,
    pub(crate) anthropic_config: &'a AnthropicConfig,
    pub(crate) model: &'a str,
    /// Whether the endpoint accepts server-side refusal fallbacks. Only the
    /// first-party Claude API does; Bedrock, Vertex, Foundry and
    /// Anthropic-compatible third-party endpoints reject or ignore them, so
    /// the configured `provider.anthropic.fallbacks` is applied only when set.
    pub(crate) server_side_fallbacks_available: bool,
}

fn resolve_messages_ttl(request: &LLMRequest, ctx: &RequestBuilderContext<'_>) -> &'static str {
    if !ctx.prompt_cache_enabled {
        return "5m";
    }

    messages_cache_ttl_for_profile(ctx.prompt_cache_settings, request.prompt_cache_profile)
}

pub(crate) fn convert_to_anthropic_format(
    request: &LLMRequest,
    ctx: &RequestBuilderContext,
) -> Result<Value, LLMError> {
    let resolved_model = resolve_model_name(&request.model, ctx.model);
    let tools_ttl = if ctx.prompt_cache_enabled {
        get_tools_cache_ttl(ctx.prompt_cache_settings)
    } else {
        "5m"
    };

    let messages_ttl = resolve_messages_ttl(request, ctx);

    let tools_cache_control = if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_tool_definitions {
        Some(CacheControl {
            control_type: "ephemeral".into(),
            ttl: Some(tools_ttl.into()),
        })
    } else {
        None
    };

    let system_cache_control = if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_system_messages {
        Some(CacheControl {
            control_type: "ephemeral".into(),
            ttl: Some(tools_ttl.into()),
        })
    } else {
        None
    };

    let max_breakpoints = if ctx.prompt_cache_enabled {
        ctx.prompt_cache_settings.max_breakpoints as usize
    } else {
        0
    };
    let mut breakpoints_remaining = max_breakpoints;

    let tools_breakpoints_before = breakpoints_remaining;
    let mut tools = build_tools(request, &tools_cache_control, &mut breakpoints_remaining)?;
    let tools_breakpoints_used = tools_breakpoints_before.saturating_sub(breakpoints_remaining);

    // Inject the Anthropic server-side advisor tool when enabled and the executor
    // model forms a valid pair with the configured advisor model.
    let advisor_injected =
        if let Some(advisor_tool) = resolve_advisor_tool(resolved_model, &ctx.anthropic_config.advisor) {
            let mut built = tools.unwrap_or_default();
            built.push(advisor_tool);
            tools = Some(built);
            true
        } else {
            false
        };

    let allow_mid_conversation_system = supports_mid_conversation_system_messages(resolved_model, ctx.model);
    let history_system_placement = HistorySystemPlacement::for_route(allow_mid_conversation_system);

    let SystemPromptBuildResult {
        mut system_value,
        breakpoints_used,
        has_uncached_runtime_context,
    } = build_system_prompt(request, &system_cache_control, breakpoints_remaining, history_system_placement);
    breakpoints_remaining = breakpoints_remaining.saturating_sub(breakpoints_used);

    // When the advisor tool is active, append a system-prompt block guiding the
    // executor model on when to invoke it (per Anthropic's recommended prompt
    // for coding tasks).
    if advisor_injected {
        let advisor_guidance = concat!(
            "You have access to an advisor tool that pairs a faster executor model with a ",
            "higher-intelligence advisor model for strategic guidance mid-generation. ",
            "Use the advisor tool when you:\n",
            "- Need a second opinion on a complex architectural decision\n",
            "- Are unsure about the best approach to a multi-step problem\n",
            "- Want to validate your plan before executing many tool calls\n",
            "- Hit a blocker you cannot resolve alone\n",
            "When the advisor returns guidance, incorporate it into your response. ",
            "If the advisor suggests a different approach, weigh it against your own reasoning.",
        );
        let guidance_block = json!({
            "type": "text",
            "text": advisor_guidance,
        });
        match &mut system_value {
            Some(Value::Array(blocks)) => {
                blocks.push(guidance_block);
            }
            Some(Value::String(text)) => {
                let existing = std::mem::take(text);
                system_value = Some(Value::Array(vec![json!({ "type": "text", "text": existing }), guidance_block]));
            }
            // build_system_prompt only produces String, Array, or None —
            // this arm is defensive for forward-compatibility.
            _ => {
                system_value = Some(Value::Array(vec![guidance_block]));
            }
        }
    }

    let messages_cache_control = if ctx.prompt_cache_enabled && ctx.prompt_cache_settings.cache_user_messages {
        Some(CacheControl {
            control_type: "ephemeral".into(),
            ttl: Some(messages_ttl.into()),
        })
    } else {
        None
    };

    // Leading system messages already live in the top-level system prompt
    // (see `HistorySystemPlacement`); skip them here so each history system
    // message is rendered exactly once.
    let conversation_messages = &request.messages[history_system_placement.leading_folded_count(&request.messages)..];

    // Hoisting moves the largest user message to the front, which reorders
    // earlier turns. On preserved-thinking models that invalidates every
    // replayed thinking block (their signatures bind the exact prior prefix),
    // so the optimization is skipped there and history stays append-only.
    let needs_hoisting = request
        .coding_agent_settings
        .as_ref()
        .is_some_and(|s| s.long_context_optimization)
        && conversation_messages.len() > 1
        && !preserves_thinking_across_turns(resolved_model, ctx.model);

    // Only clone the message vector when hoisting will actually mutate it.
    // In the common case (no long-context optimization or single message),
    // borrow the original slice and skip the deep copy.
    let mut hoisted_messages: Vec<crate::provider::Message>;
    let messages_to_process: &[crate::provider::Message] = if needs_hoisting {
        hoisted_messages = conversation_messages.to_vec();
        hoist_largest_user_message(&mut hoisted_messages);
        &hoisted_messages
    } else {
        conversation_messages
    };

    let messages_breakpoints_before = breakpoints_remaining;
    let messages = build_messages(
        request,
        messages_to_process,
        &messages_cache_control,
        ctx.prompt_cache_settings,
        &mut breakpoints_remaining,
        ctx.model,
    )?;
    let messages_breakpoints_used = messages_breakpoints_before.saturating_sub(breakpoints_remaining);
    let explicit_breakpoints_used = max_breakpoints.saturating_sub(breakpoints_remaining);

    let (thinking_val, reasoning_val) = build_thinking_config(request, ctx.anthropic_config, ctx.model)?;

    let anthropic_overrides = request.anthropic_request_overrides.as_ref();
    let thinking_is_adaptive = matches!(thinking_val, Some(ThinkingConfig::Adaptive { .. }));

    let adaptive_effort = if thinking_is_adaptive && request.effort.is_none() {
        request
            .reasoning_effort
            .map(|effort| effort_from_reasoning_for_adaptive(effort).to_string())
    } else {
        None
    };
    let effort_value = if supports_effort(resolved_model, ctx.model) && thinking_is_adaptive {
        match anthropic_overrides.map(|overrides| &overrides.effort) {
            Some(AnthropicOptionalStringOverride::Explicit(effort)) => Some(effort.to_ascii_lowercase()),
            Some(AnthropicOptionalStringOverride::Omit) => None,
            _ => request
                .effort
                .as_ref()
                .map(|effort| effort.to_ascii_lowercase())
                .or_else(|| adaptive_effort.as_ref().map(|effort| effort.to_ascii_lowercase()))
                .or_else(|| {
                    // Only an explicitly configured effort overrides the
                    // model's own default (for example `medium` on Opus 5.5).
                    thinking_val.as_ref().and_then(|_| {
                        ctx.anthropic_config
                            .effort
                            .map(|effort| effort.as_str())
                            .filter(|effort| effort_allowed_for_model(resolved_model, ctx.model, effort))
                            .or_else(|| default_effort_for_model(resolved_model, ctx.model))
                            .map(str::to_string)
                    })
                }),
        }
    } else {
        None
    };
    let task_budget = if supports_task_budget(resolved_model, ctx.model) {
        match anthropic_overrides.map(|overrides| &overrides.task_budget_tokens) {
            Some(AnthropicOptionalU32Override::Explicit(total)) => {
                Some(AnthropicTaskBudget { budget_type: "tokens".to_string(), total: *total })
            }
            Some(AnthropicOptionalU32Override::Omit) => None,
            _ => ctx
                .anthropic_config
                .task_budget_tokens
                .map(|total| AnthropicTaskBudget { budget_type: "tokens".to_string(), total }),
        }
    } else {
        None
    };
    let fallbacks = build_fallbacks(request, ctx, resolved_model, thinking_val.as_ref(), effort_value.as_deref());
    // Forced tool use (`any`/`tool`) is rejected when thinking is on and, on
    // some models, unconditionally. Fallback entries inherit the top-level
    // `tool_choice`, so every fallback model must accept it as well.
    let forced_tool_choice_allowed = !thinking_is_on(thinking_val.as_ref(), resolved_model, ctx.model)
        && !rejects_forced_tool_choice(resolved_model, ctx.model)
        && fallbacks.as_ref().is_none_or(|fallbacks| {
            fallbacks.models().iter().all(|fb| {
                let fallback_thinking = fb.thinking.as_ref().or(thinking_val.as_ref());
                !thinking_is_on(fallback_thinking, &fb.model, ctx.model)
                    && !rejects_forced_tool_choice(&fb.model, ctx.model)
            })
        });
    let final_tool_choice = build_tool_choice(request, forced_tool_choice_allowed);
    let output_format = request
        .output_format
        .as_ref()
        .map(|schema| AnthropicOutputFormat::JsonSchema { schema: schema.clone() });
    let output_config = if effort_value.is_some() || task_budget.is_some() || output_format.is_some() {
        Some(AnthropicOutputConfig {
            effort: effort_value,
            task_budget,
            format: output_format,
        })
    } else {
        None
    };

    // Fallback entries cannot override sampling, so the top-level temperature
    // is sent to every fallback model too; drop it when any of them would
    // reject it.
    let fallback_rejects_sampling = fallbacks.as_ref().is_some_and(|fallbacks| {
        fallbacks.models().iter().any(|fb| {
            rejects_sampling(&fb.model, ctx.model)
                || fb.thinking.as_ref().is_some_and(|thinking| {
                    !matches!(thinking, ThinkingConfig::Disabled | ThinkingConfig::BetweenTools)
                })
        })
    });
    let effective_temperature =
        if thinking_val.is_some() || rejects_sampling(resolved_model, ctx.model) || fallback_rejects_sampling {
            None
        } else {
            request.temperature
        };

    let top_level_cache_control =
        if ctx.prompt_cache_enabled && explicit_breakpoints_used < max_breakpoints && !has_uncached_runtime_context {
            let ttl = if messages_breakpoints_used > 0 {
                messages_ttl
            } else if breakpoints_used > 0 || tools_breakpoints_used > 0 {
                tools_ttl
            } else {
                messages_ttl
            };

            Some(CacheControl {
                control_type: "ephemeral".into(),
                ttl: Some(ttl.into()),
            })
        } else {
            None
        };

    let mut anthropic_request = AnthropicRequest {
        model: resolved_model.to_string(),
        max_tokens: request
            .max_tokens
            .unwrap_or_else(|| default_max_tokens_for_model(resolved_model, ctx.model, thinking_val.is_some())),
        cache_control: top_level_cache_control,
        messages,
        system: system_value,
        temperature: effective_temperature,
        tools,
        tool_choice: final_tool_choice,
        thinking: thinking_val,
        reasoning: reasoning_val,
        output_config: output_config.map(Into::into),
        context_management: request.context_management.clone(),
        fallbacks,
        fallback_credit_token: request.fallback_credit_token.clone(),
        stream: request.stream,
    };

    hardening::strip_globally_orphaned_tool_blocks(&mut anthropic_request.messages);
    hardening::enforce_tool_use_result_adjacency(&mut anthropic_request.messages);
    hardening::hoist_tool_results_to_front(&mut anthropic_request.messages);
    hardening::guard_trailing_assistant_message(
        &mut anthropic_request.messages,
        supports_assistant_prefill(resolved_model, ctx.model),
    );

    serde_json::to_value(anthropic_request).map_err(|e| LLMError::Provider {
        message: format!("Serialization error: {e}"),
        metadata: None,
    })
}

/// Builds the server-side `fallbacks` parameter.
///
/// Fallbacks are only sent on the first-party API, for models whose profile
/// supports server-side fallbacks, and never on a credit-token retry. Within
/// those gates, request-level `fallbacks` win over `provider.anthropic.fallbacks`:
/// an empty request list sends nothing, and an invalid one (too long, blank or
/// repeated models, zero `max_tokens`) is dropped with a warning. Otherwise
/// `"default"` sends the keyword form, an explicit list sends entries, and
/// `"off"` (or an invalid list, which config validation reports) sends nothing.
fn build_fallbacks(
    request: &LLMRequest,
    ctx: &RequestBuilderContext<'_>,
    resolved_model: &str,
    primary_thinking: Option<&ThinkingConfig>,
    effort: Option<&str>,
) -> Option<AnthropicFallbacksParam> {
    // A credit-token retry already targets the fallback model; asking for
    // further fallbacks would change the prompt-shaping fields the token was
    // issued for.
    if request.fallback_credit_token.is_some()
        || !ctx.server_side_fallbacks_available
        || !supports_server_side_fallback(resolved_model, ctx.model)
    {
        return None;
    }

    if let Some(fallbacks) = request.fallbacks.as_ref() {
        if fallbacks.is_empty() {
            return None;
        }
        if let Some(reason) = AnthropicFallbacks::entries_validation_error(
            "fallbacks",
            fallbacks.iter().map(|fb| (fb.model.as_str(), fb.max_tokens)),
        ) {
            tracing::warn!(%reason, "request-level fallbacks dropped: invalid entries");
            return None;
        }
        let entries = fallbacks
            .iter()
            .map(|fb| (fb.model.trim(), fb.max_tokens, fb.thinking.as_ref().map(fallback_thinking_config)));
        return Some(AnthropicFallbacksParam::Models(sanitize_fallback_entries(
            entries,
            primary_thinking,
            effort,
            ctx.model,
        )));
    }

    match &ctx.anthropic_config.fallbacks {
        AnthropicFallbacks::Mode(AnthropicFallbackMode::Default) => {
            Some(AnthropicFallbacksParam::Mode(AnthropicFallbacksKeyword::Default))
        }
        AnthropicFallbacks::Mode(AnthropicFallbackMode::Off) => None,
        configured @ AnthropicFallbacks::Models(targets) => {
            if configured.validation_error("provider.anthropic.fallbacks").is_some() {
                return None;
            }
            let entries = targets.iter().map(|target| (target.model.trim(), target.max_tokens, None));
            Some(AnthropicFallbacksParam::Models(sanitize_fallback_entries(
                entries,
                primary_thinking,
                effort,
                ctx.model,
            )))
        }
    }
}

/// Makes explicit `fallbacks` entries valid for their own models.
///
/// The API merges each entry into the primary request, and the merged request
/// must be valid as a direct request to the entry's model. An explicit thinking
/// override is rewritten for its model; without one, the entry inherits the
/// primary thinking config, so an explicit valid override is added whenever
/// the fallback model would reject the inherited config.
fn sanitize_fallback_entries<'m>(
    entries: impl Iterator<Item = (&'m str, Option<u32>, Option<ThinkingConfig>)>,
    primary_thinking: Option<&ThinkingConfig>,
    effort: Option<&str>,
    default_model: &str,
) -> Vec<AnthropicFallbackParam> {
    entries
        .map(|(model, max_tokens, explicit_thinking)| {
            let thinking = match explicit_thinking {
                Some(explicit) => {
                    Some(rewrite_thinking_for_model(&explicit, model, default_model, effort).unwrap_or(explicit))
                }
                None => primary_thinking
                    .and_then(|inherited| rewrite_thinking_for_model(inherited, model, default_model, effort)),
            };
            AnthropicFallbackParam { model: model.to_string(), max_tokens, thinking }
        })
        .collect()
}

fn fallback_thinking_config(thinking: &AnthropicThinkingConfig) -> ThinkingConfig {
    match thinking {
        AnthropicThinkingConfig::Disabled => ThinkingConfig::Disabled,
        AnthropicThinkingConfig::Enabled { budget_tokens, display } => ThinkingConfig::Enabled {
            budget_tokens: *budget_tokens,
            display: parse_thinking_display(display.as_deref()),
        },
        AnthropicThinkingConfig::Adaptive { display } => ThinkingConfig::Adaptive {
            display: parse_thinking_display(display.as_deref()),
        },
    }
}

fn parse_thinking_display(display: Option<&str>) -> Option<ThinkingDisplay> {
    match display? {
        "summarized" => Some(ThinkingDisplay::Summarized),
        "omitted" => Some(ThinkingDisplay::Omitted),
        "updates" => Some(ThinkingDisplay::Updates),
        _ => None,
    }
}

fn effort_from_reasoning_for_adaptive(effort: ReasoningEffortLevel) -> &'static str {
    match effort {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => reasoning::LOW,
        _ => effort.as_str(),
    }
}

/// Whether the given model is an Anthropic model eligible to act as an advisor
/// executor. Server-side advisor tooling is only available on Anthropic models.
///
/// Model ids may carry a `-YYYYMMDD` version pin; this strips the single known
/// dated suffix before checking against the supported set. Keep this in lockstep
/// with `vtcode_config::constants::models::anthropic::normalize_model_id` so the
/// executor check and the advisor-pair validation agree on normalization.
fn is_anthropic_executor_model(model: &str) -> bool {
    use vtcode_config::constants::models::anthropic::{SUPPORTED_MODELS, normalize_model_id};
    let normalized = normalize_model_id(model);
    SUPPORTED_MODELS.contains(&normalized)
}

/// Single source of truth for the Anthropic server-side advisor tool.
///
/// Resolves and builds the `advisor_20260301` tool, or returns `None` when the
/// advisor is disabled, the executor is not an eligible Anthropic model, or the
/// executor/advisor model pair fails `validate_advisor_pair`. Both the request
/// builder (tool injection) and the provider (beta-header gating) call this so
/// the two can never disagree.
pub(crate) fn resolve_advisor_tool(executor: &str, advisor: &AdvisorConfig) -> Option<AnthropicTool> {
    if !advisor.enabled {
        return None;
    }

    if !is_anthropic_executor_model(executor) {
        return None;
    }

    let advisor_model = if advisor.model.is_empty() {
        vtcode_config::constants::models::anthropic::default_advisor_model(executor).to_string()
    } else {
        advisor.model.clone()
    };

    if let Err(reason) = vtcode_config::constants::models::anthropic::validate_advisor_pair(executor, &advisor_model) {
        tracing::warn!(%reason, "advisor tool disabled: invalid model pair");
        return None;
    }

    // Validate max_tokens if specified (API minimum is 1024).
    if let Some(max_tokens) = advisor.max_tokens
        && max_tokens < 1024
    {
        tracing::warn!(max_tokens, "advisor tool disabled: max_tokens must be >= 1024");
        return None;
    }

    let caching = advisor.caching.and_then(|c| {
        c.enabled.then_some(AnthropicAdvisorCaching {
            cache_type: "ephemeral".to_string(),
            ttl: c.ttl.as_str().to_string(),
        })
    });

    Some(AnthropicTool::Advisor(AnthropicAdvisorTool {
        tool_type: "advisor_20260301".to_string(),
        name: "advisor".to_string(),
        model: advisor_model,
        max_uses: advisor.max_uses,
        max_tokens: advisor.max_tokens,
        caching,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        AnthropicRequestOverrides, AnthropicThinkingModeOverride, FallbackModel, Message, ToolChoice,
    };
    use vtcode_config::constants::models::anthropic;
    use vtcode_config::core::AdvisorConfig;

    fn convert(request: &LLMRequest) -> Value {
        convert_with(request, &AnthropicConfig::default(), false)
    }

    /// Converts against the first-party API, where server-side fallbacks apply.
    fn convert_first_party(request: &LLMRequest) -> Value {
        convert_with(request, &AnthropicConfig::default(), true)
    }

    fn convert_with(request: &LLMRequest, anthropic_config: &AnthropicConfig, first_party: bool) -> Value {
        let prompt_cache_settings = AnthropicPromptCacheSettings::default();
        let ctx = RequestBuilderContext {
            prompt_cache_enabled: false,
            prompt_cache_settings: &prompt_cache_settings,
            anthropic_config,
            model: anthropic::DEFAULT_MODEL,
            server_side_fallbacks_available: first_party,
        };
        convert_to_anthropic_format(request, &ctx).expect("payload conversion")
    }

    fn user_request(model: &str) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        }
    }

    fn config_with_fallbacks(fallbacks: AnthropicFallbacks) -> AnthropicConfig {
        AnthropicConfig { fallbacks, ..AnthropicConfig::default() }
    }

    #[test]
    fn default_config_requests_default_fallbacks_on_supporting_models() {
        for model in [
            anthropic::CLAUDE_OPUS_5_5,
            anthropic::CLAUDE_OPUS_5,
            anthropic::CLAUDE_FABLE_5,
            anthropic::CLAUDE_FABLE_5_1,
        ] {
            let payload = convert_with(&user_request(model), &AnthropicConfig::default(), true);
            assert_eq!(payload["fallbacks"], json!("default"), "{model}");
        }
    }

    #[test]
    fn config_fallbacks_are_omitted_for_unsupported_models_off_and_third_party_endpoints() {
        let default_config = AnthropicConfig::default();
        for model in [anthropic::CLAUDE_SONNET_5, "claude-3-5-haiku-latest", "MiniMax-M2"] {
            let payload = convert_with(&user_request(model), &default_config, true);
            assert!(payload.get("fallbacks").is_none(), "{model} has no fallback profile");
        }

        let off = config_with_fallbacks(AnthropicFallbacks::Mode(AnthropicFallbackMode::Off));
        let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &off, true);
        assert!(payload.get("fallbacks").is_none(), "\"off\" sends nothing");

        let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &default_config, false);
        assert!(payload.get("fallbacks").is_none(), "non-first-party endpoints send nothing");
    }

    #[test]
    fn credit_token_retry_does_not_request_further_fallbacks() {
        let mut request = user_request(anthropic::CLAUDE_OPUS_5);
        request.fallback_credit_token = Some("tok".to_string());
        let payload = convert_with(&request, &AnthropicConfig::default(), true);
        assert!(payload.get("fallbacks").is_none());
        assert_eq!(payload["fallback_credit_token"], "tok");
    }

    #[test]
    fn configured_fallback_list_is_sanitized_per_model() {
        use vtcode_config::core::AnthropicFallbackTarget;
        let config = config_with_fallbacks(AnthropicFallbacks::Models(vec![
            AnthropicFallbackTarget {
                model: " claude-opus-4-8 ".to_string(),
                max_tokens: Some(32_000),
            },
            AnthropicFallbackTarget {
                model: anthropic::CLAUDE_OPUS_5.to_string(),
                max_tokens: None,
            },
        ]));
        let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &config, true);

        // Opus 5.5 defaults to `display: "updates"`, which neither fallback
        // model accepts, so each entry gets an explicit valid override.
        assert_eq!(payload["thinking"], json!({ "type": "adaptive", "display": "updates" }));
        assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
        assert_eq!(payload["fallbacks"][0]["max_tokens"], 32_000);
        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
        assert_eq!(payload["fallbacks"][1]["model"], anthropic::CLAUDE_OPUS_5);
        assert!(payload["fallbacks"][1].get("max_tokens").is_none());
        assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
    }

    #[test]
    fn invalid_configured_fallback_list_sends_nothing() {
        use vtcode_config::core::AnthropicFallbackTarget;
        let target = AnthropicFallbackTarget {
            model: "claude-opus-4-8".to_string(),
            max_tokens: None,
        };
        let config = config_with_fallbacks(AnthropicFallbacks::Models(vec![target.clone(), target]));
        let payload = convert_with(&user_request(anthropic::CLAUDE_OPUS_5_5), &config, true);
        assert!(payload.get("fallbacks").is_none());
    }

    #[test]
    fn request_fallbacks_override_config_fallbacks() {
        let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback("claude-opus-4-8", None)]);
        let payload = convert_with(&request, &AnthropicConfig::default(), true);
        assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
    }

    fn fallback(model: &str, thinking: Option<AnthropicThinkingConfig>) -> FallbackModel {
        FallbackModel {
            model: model.to_string(),
            max_tokens: None,
            thinking,
        }
    }

    fn request_with_fallbacks(model: &str, fallbacks: Vec<FallbackModel>) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            fallbacks: Some(fallbacks),
            ..Default::default()
        }
    }

    fn fallback_thinking(payload: &Value, index: usize) -> &Value {
        &payload["fallbacks"][index]["thinking"]
    }

    #[test]
    fn inherited_updates_display_is_dropped_for_fallbacks_without_progress_updates() {
        // Opus 5.5 defaults to `display: "updates"`; Sonnet 5 rejects it.
        let request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5_5,
            vec![
                fallback(anthropic::CLAUDE_SONNET_5, None),
                fallback(anthropic::CLAUDE_FABLE_5, None),
            ],
        );
        let payload = convert_first_party(&request);

        assert_eq!(payload["thinking"], json!({ "type": "adaptive", "display": "updates" }));
        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
        assert!(payload["fallbacks"][1].get("thinking").is_none(), "Fable 5 accepts the inherited display");
    }

    #[test]
    fn explicit_updates_display_is_parsed_for_fallbacks() {
        let request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5,
            vec![
                fallback(
                    anthropic::CLAUDE_OPUS_5_5,
                    Some(AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
                ),
                fallback(
                    anthropic::CLAUDE_OPUS_5,
                    Some(AnthropicThinkingConfig::Adaptive { display: Some("updates".to_string()) }),
                ),
            ],
        );
        let payload = convert_first_party(&request);

        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive", "display": "updates" }));
        assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
    }

    #[test]
    fn fallback_manual_budget_becomes_adaptive_for_models_without_budget_support() {
        let request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5,
            vec![fallback(
                anthropic::CLAUDE_OPUS_5_5,
                Some(AnthropicThinkingConfig::Enabled {
                    budget_tokens: 8192,
                    display: Some("summarized".to_string()),
                }),
            )],
        );
        let payload = convert_first_party(&request);

        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive", "display": "summarized" }));
    }

    #[test]
    fn fallback_disabled_thinking_becomes_adaptive_for_adaptive_only_models() {
        let request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5,
            vec![
                fallback(anthropic::CLAUDE_OPUS_5_5, Some(AnthropicThinkingConfig::Disabled)),
                fallback(anthropic::CLAUDE_FABLE_5_1, Some(AnthropicThinkingConfig::Disabled)),
                fallback(anthropic::CLAUDE_SONNET_5, Some(AnthropicThinkingConfig::Disabled)),
            ],
        );
        let payload = convert_first_party(&request);

        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
        assert_eq!(fallback_thinking(&payload, 1), &json!({ "type": "adaptive" }));
        // Sonnet 5 accepts disabled thinking, so the override is kept.
        assert_eq!(fallback_thinking(&payload, 2), &json!({ "type": "disabled" }));
    }

    #[test]
    fn fallback_inheriting_rejected_disabled_thinking_gets_explicit_adaptive() {
        let mut request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5,
            vec![
                fallback(anthropic::CLAUDE_OPUS_5_5, None),
                fallback(anthropic::CLAUDE_OPUS_5, None),
            ],
        );
        request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
            thinking_mode: AnthropicThinkingModeOverride::Disabled,
            ..Default::default()
        });
        let payload = convert_first_party(&request);

        assert_eq!(payload["thinking"], json!({ "type": "disabled" }));
        // Opus 5.5 would inherit the rejected `disabled` config.
        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "adaptive" }));
        // Opus 5 accepts disabled thinking at its default effort, so it inherits.
        assert!(payload["fallbacks"][1].get("thinking").is_none());
    }

    #[test]
    fn fallback_without_override_inherits_valid_primary_thinking() {
        let request =
            request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)]);
        let payload = convert_first_party(&request);

        assert_eq!(payload["thinking"]["type"], "adaptive");
        assert!(payload["fallbacks"][0].get("thinking").is_none());
    }

    #[test]
    fn fallback_thinking_is_kept_for_unprofiled_models() {
        let request = request_with_fallbacks(
            anthropic::CLAUDE_OPUS_5,
            vec![fallback(
                "claude-unlisted-model",
                Some(AnthropicThinkingConfig::Enabled { budget_tokens: 4096, display: None }),
            )],
        );
        let payload = convert_first_party(&request);

        assert_eq!(fallback_thinking(&payload, 0), &json!({ "type": "enabled", "budget_tokens": 4096 }));
    }

    #[test]
    fn request_fallbacks_are_gated_like_config_fallbacks() {
        let fallbacks = || vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)];

        // Models without server-side fallback support never send them, so a
        // fallback that rejects sampling cannot strip the primary temperature.
        for model in ["claude-unlisted-model", anthropic::CLAUDE_SONNET_5] {
            let mut request = request_with_fallbacks(model, fallbacks());
            request.temperature = Some(0.2);
            let payload = convert_first_party(&request);
            assert!(payload.get("fallbacks").is_none(), "{model}: {payload}");
        }
        let mut request = request_with_fallbacks("claude-unlisted-model", fallbacks());
        request.temperature = Some(0.2);
        let payload = convert_first_party(&request);
        assert!(payload["temperature"].as_f64().is_some_and(|t| (t - 0.2).abs() < 1e-6), "payload: {payload}");

        let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks());
        assert!(convert(&request).get("fallbacks").is_none(), "non-first-party endpoints send nothing");

        let mut request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks());
        request.fallback_credit_token = Some("tok".to_string());
        assert!(convert_first_party(&request).get("fallbacks").is_none(), "credit-token retries send nothing");
    }

    #[test]
    fn empty_request_fallbacks_send_nothing() {
        let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, Vec::new());
        let payload = convert_first_party(&request);

        assert!(payload.get("fallbacks").is_none(), "payload: {payload}");
    }

    #[test]
    fn invalid_request_fallbacks_send_nothing() {
        let too_many = vec![
            fallback("claude-opus-4-8", None),
            fallback(anthropic::CLAUDE_OPUS_5_5, None),
            fallback(anthropic::CLAUDE_FABLE_5, None),
            fallback(anthropic::CLAUDE_FABLE_5_1, None),
        ];
        let duplicated = vec![fallback("claude-opus-4-8", None), fallback(" claude-opus-4-8 ", None)];
        let blank = vec![fallback("  ", None)];
        let zero_max_tokens = vec![FallbackModel {
            max_tokens: Some(0),
            ..fallback("claude-opus-4-8", None)
        }];
        for fallbacks in [too_many, duplicated, blank, zero_max_tokens] {
            let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, fallbacks);
            let payload = convert_first_party(&request);
            assert!(payload.get("fallbacks").is_none(), "payload: {payload}");
        }
    }

    #[test]
    fn request_fallback_models_are_trimmed() {
        let request = request_with_fallbacks(anthropic::CLAUDE_OPUS_5, vec![fallback(" claude-opus-4-8 ", None)]);
        let payload = convert_first_party(&request);

        assert_eq!(payload["fallbacks"][0]["model"], "claude-opus-4-8");
    }

    #[test]
    fn temperature_is_kept_when_no_fallback_rejects_sampling() {
        let mut request = request_with_fallbacks("claude-unlisted-model", vec![fallback("claude-other-model", None)]);
        request.temperature = Some(0.2);
        let payload = convert(&request);

        assert!(payload["temperature"].as_f64().is_some_and(|t| (t - 0.2).abs() < 1e-6), "payload: {payload}");
    }

    fn plain_request(model: &str) -> LLMRequest {
        LLMRequest {
            model: model.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            ..Default::default()
        }
    }

    #[test]
    fn claude_5_models_default_to_64k_max_tokens() {
        for model in [
            anthropic::CLAUDE_OPUS_5_5,
            anthropic::CLAUDE_SONNET_5,
            anthropic::CLAUDE_OPUS_5,
        ] {
            let payload = convert(&plain_request(model));
            assert_eq!(payload["max_tokens"], 64_000, "{model}: {payload}");
        }
    }

    #[test]
    fn opus_5_5_keeps_64k_max_tokens_when_thinking_field_is_omitted() {
        // Opus 5.5 rejects `disabled`, so a Disabled override omits the field;
        // the model still thinks adaptively and needs the full default budget.
        let mut request = plain_request(anthropic::CLAUDE_OPUS_5_5);
        request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
            thinking_mode: AnthropicThinkingModeOverride::Disabled,
            ..Default::default()
        });
        let payload = convert(&request);

        assert!(payload.get("thinking").is_none(), "payload: {payload}");
        assert_eq!(payload["max_tokens"], 64_000, "payload: {payload}");
    }

    #[test]
    fn explicit_max_tokens_is_kept_for_claude_5_models() {
        let mut request = plain_request(anthropic::CLAUDE_OPUS_5_5);
        request.max_tokens = Some(2048);
        let payload = convert(&request);

        assert_eq!(payload["max_tokens"], 2048, "payload: {payload}");
    }

    #[test]
    fn unprofiled_model_without_thinking_keeps_legacy_max_tokens_default() {
        let payload = convert(&plain_request("claude-unlisted-model"));

        assert!(payload.get("thinking").is_none(), "payload: {payload}");
        assert_eq!(payload["max_tokens"], 4096, "payload: {payload}");
    }

    #[test]
    fn coding_agent_settings_never_inject_prompt_scaffolding() {
        // Settings serialized by older builds still deserialize; the removed
        // role / XML-tag / <thinking>-<answer> scaffolding fields are ignored.
        let settings: crate::provider::CodingAgentSettings = serde_json::from_value(json!({
            "force_xml_tags": true,
            "role_specialization": "Senior Software Architect",
            "enforce_structured_thought": true,
            "long_context_optimization": false
        }))
        .expect("legacy settings deserialize");
        let mut request = plain_request(anthropic::CLAUDE_SONNET_5);
        request.system_prompt = Some(std::sync::Arc::from("Base prompt"));
        request.coding_agent_settings = Some(Box::new(settings));
        let payload = convert(&request);

        let system = payload["system"].to_string();
        assert!(system.contains("Base prompt"), "system: {system}");
        for scaffold in [
            "You are Senior Software Architect",
            "XML tags",
            "<thinking>",
            "<answer>",
        ] {
            assert!(!system.contains(scaffold), "unexpected {scaffold:?} in system: {system}");
        }
    }

    fn forced_tool_request(model: &str, disable_thinking: bool) -> LLMRequest {
        let mut request = plain_request(model);
        request.tool_choice = Some(ToolChoice::any());
        if disable_thinking {
            request.anthropic_request_overrides = Some(AnthropicRequestOverrides {
                thinking_mode: AnthropicThinkingModeOverride::Disabled,
                ..Default::default()
            });
        }
        request
    }

    #[test]
    fn forced_tool_choice_is_downgraded_for_models_that_reject_it_even_without_thinking() {
        for model in [anthropic::CLAUDE_OPUS_5_5, anthropic::CLAUDE_FABLE_5_1] {
            let payload = convert(&forced_tool_request(model, true));

            assert!(payload.get("thinking").is_none(), "{model}: {payload}");
            assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "{model}: {payload}");
        }
    }

    #[test]
    fn forced_tool_choice_is_kept_when_thinking_is_disabled() {
        let payload = convert(&forced_tool_request(anthropic::CLAUDE_SONNET_5, true));

        assert_eq!(payload["thinking"], json!({"type": "disabled"}), "payload: {payload}");
        assert_eq!(payload["tool_choice"], json!({"type": "any"}), "payload: {payload}");
    }

    #[test]
    fn forced_tool_choice_is_downgraded_when_thinking_is_on() {
        let payload = convert(&forced_tool_request(anthropic::CLAUDE_SONNET_5, false));

        assert_eq!(payload["thinking"]["type"], "adaptive", "payload: {payload}");
        assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
    }

    #[test]
    fn forced_tool_choice_is_kept_for_unprofiled_models_without_thinking() {
        let mut request = plain_request("claude-unlisted-model");
        request.tool_choice = Some(ToolChoice::function("get_weather".to_string()));
        let payload = convert(&request);

        assert_eq!(payload["tool_choice"], json!({"type": "tool", "name": "get_weather"}), "payload: {payload}");
    }

    #[test]
    fn forced_tool_choice_is_downgraded_when_a_fallback_model_rejects_it() {
        let mut request = forced_tool_request(anthropic::CLAUDE_OPUS_5, true);
        let payload = convert_first_party(&request);
        assert_eq!(payload["tool_choice"], json!({"type": "any"}), "baseline payload: {payload}");

        request.fallbacks = Some(vec![fallback(anthropic::CLAUDE_OPUS_5_5, None)]);
        let payload = convert_first_party(&request);

        assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
    }

    #[test]
    fn forced_tool_choice_is_downgraded_when_a_fallback_thinks() {
        let mut request = forced_tool_request(anthropic::CLAUDE_OPUS_5, true);
        request.fallbacks = Some(vec![fallback(
            "claude-opus-4-8",
            Some(AnthropicThinkingConfig::Adaptive { display: None }),
        )]);
        let payload = convert_first_party(&request);

        assert_eq!(payload["tool_choice"], json!({"type": "auto"}), "payload: {payload}");
    }

    fn advisor_config(enabled: bool, model: &str, max_uses: Option<u32>) -> AdvisorConfig {
        AdvisorConfig {
            enabled,
            model: model.to_string(),
            max_uses,
            max_tokens: None,
            caching: None,
        }
    }

    #[test]
    fn resolve_advisor_tool_disabled_returns_none() {
        let cfg = advisor_config(false, "", None);
        assert!(resolve_advisor_tool("claude-sonnet-5", &cfg).is_none());
    }

    #[test]
    fn resolve_advisor_tool_non_anthropic_executor_returns_none() {
        let cfg = advisor_config(true, "", None);
        assert!(resolve_advisor_tool("gpt-4o", &cfg).is_none());
    }

    #[test]
    fn resolve_advisor_tool_defaults_to_valid_pair() {
        let cfg = advisor_config(true, "", None);
        let tool = resolve_advisor_tool("claude-sonnet-5", &cfg);
        assert!(matches!(tool, Some(AnthropicTool::Advisor(_))));
        if let Some(AnthropicTool::Advisor(t)) = tool {
            assert_eq!(t.model, "claude-opus-5");
            assert_eq!(t.name, "advisor");
            assert_eq!(t.tool_type, "advisor_20260301");
        }
    }

    #[test]
    fn resolve_advisor_tool_invalid_pair_returns_none() {
        // Advisor less capable than the executor must be rejected.
        let cfg = advisor_config(true, "claude-sonnet-5", None);
        assert!(resolve_advisor_tool("claude-opus-5", &cfg).is_none());
    }

    #[test]
    fn resolve_advisor_tool_accepts_self_advising_model() {
        let cfg = advisor_config(true, "claude-fable-5", None);
        assert!(resolve_advisor_tool("claude-fable-5", &cfg).is_some());
        // Fable may only advise Fable.
        assert!(resolve_advisor_tool("claude-opus-5", &cfg).is_none());
    }

    #[test]
    fn resolve_advisor_tool_supports_dated_executor_suffix() {
        let cfg = advisor_config(true, "", None);
        // A dated pin for a currently supported model must not break the
        // supported-model check.
        assert!(resolve_advisor_tool("claude-sonnet-5-20251001", &cfg).is_some());
    }
}
