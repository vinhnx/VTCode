use anyhow::Result;
use serde_json::json;
use std::fmt::Write as _;
use std::sync::Arc;

use vtcode_core::config::{OpenAIPromptCacheKeyMode, PromptCachingConfig};
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::prompts::upsert_harness_limits_section;

use crate::agent::runloop::unified::incremental_system_prompt::PromptCacheShapingMode;
use crate::agent::runloop::unified::run_loop_context::TurnExecutionSnapshot;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::turn_helpers::supports_responses_chaining;

use super::metrics::emit_tool_catalog_cache_metrics;
use super::retry::resolve_compaction_threshold;

pub(super) fn is_openai_prompt_cache_enabled(
    provider_name: &str,
    global_prompt_cache_enabled: bool,
    openai_prompt_cache_enabled: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openai")
        && global_prompt_cache_enabled
        && openai_prompt_cache_enabled
}

pub(super) fn resolve_prompt_cache_shaping_mode(
    provider_name: &str,
    prompt_cache: &PromptCachingConfig,
) -> PromptCacheShapingMode {
    if !prompt_cache.cache_friendly_prompt_shaping
        || !prompt_cache.is_provider_enabled(provider_name)
    {
        return PromptCacheShapingMode::Disabled;
    }

    if matches!(
        provider_name.to_ascii_lowercase().as_str(),
        "anthropic" | "minimax"
    ) {
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    } else {
        PromptCacheShapingMode::TrailingRuntimeContext
    }
}

pub(super) fn build_openai_prompt_cache_key(
    openai_prompt_cache_enabled: bool,
    prompt_cache_key_mode: &OpenAIPromptCacheKeyMode,
    run_id: &str,
) -> Option<String> {
    if !openai_prompt_cache_enabled {
        return None;
    }

    match prompt_cache_key_mode {
        OpenAIPromptCacheKeyMode::Session => Some(format!("vtcode:openai:{run_id}")),
        OpenAIPromptCacheKeyMode::Off => None,
    }
}

pub(super) struct TurnRequestSnapshot {
    pub provider_name: String,
    pub plan_mode: bool,
    pub full_auto: bool,
    pub request_user_input_enabled: bool,
    pub context_window_size: usize,
    pub turn_timeout_secs: u64,
    pub openai_prompt_cache_enabled: bool,
    pub openai_prompt_cache_key_mode: OpenAIPromptCacheKeyMode,
    pub prompt_cache_shaping_mode: PromptCacheShapingMode,
    pub capabilities: uni::ProviderCapabilities,
    pub execution: TurnExecutionSnapshot,
}

struct PromptAssemblyInput<'a> {
    step_count: usize,
    active_model: &'a str,
    turn: &'a TurnRequestSnapshot,
}

struct PromptAssemblyOutput {
    system_prompt: String,
    current_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    has_tools: bool,
}

pub(super) struct TurnRequestBuildResult {
    pub request: uni::LLMRequest,
    pub has_tools: bool,
}

pub(super) fn capture_turn_request_snapshot(
    ctx: &mut TurnProcessingContext<'_>,
    active_model: &str,
) -> TurnRequestSnapshot {
    let prompt_cache_config = &ctx.config.prompt_cache;
    let plan_mode = ctx.session_stats.is_plan_mode();
    let provider_name = ctx.provider_client.name().to_ascii_lowercase();
    let openai_prompt_cache_enabled = is_openai_prompt_cache_enabled(
        &provider_name,
        prompt_cache_config.enabled,
        prompt_cache_config.providers.openai.enabled,
    );
    let prompt_cache_shaping_mode =
        resolve_prompt_cache_shaping_mode(&provider_name, prompt_cache_config);
    let request_user_input_enabled =
        FeatureSet::from_config(ctx.vt_cfg).request_user_input_enabled(plan_mode, true);
    let context_window_size = ctx.provider_client.effective_context_size(active_model);
    let turn_timeout_secs = ctx
        .vt_cfg
        .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs)
        .unwrap_or(300);
    let openai_prompt_cache_key_mode = prompt_cache_config
        .providers
        .openai
        .prompt_cache_key_mode
        .clone();
    let full_auto = ctx.full_auto;
    let capabilities = uni::get_cached_capabilities(&**ctx.provider_client, active_model);

    TurnRequestSnapshot {
        provider_name,
        plan_mode,
        full_auto,
        request_user_input_enabled,
        context_window_size,
        turn_timeout_secs,
        openai_prompt_cache_enabled,
        openai_prompt_cache_key_mode,
        prompt_cache_shaping_mode,
        capabilities,
        execution: ctx.harness_state.execution_snapshot(),
    }
}

async fn assemble_prompt(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let mut system_prompt = ctx
        .context_manager
        .build_system_prompt(
            ctx.working_history,
            input.step_count,
            crate::agent::runloop::unified::context_manager::SystemPromptParams {
                full_auto: input.turn.full_auto,
                plan_mode: input.turn.plan_mode,
                context_window_size: Some(input.turn.context_window_size),
                prompt_cache_shaping_mode: input.turn.prompt_cache_shaping_mode,
            },
        )
        .await?;

    upsert_harness_limits_section(
        &mut system_prompt,
        input.turn.execution.max_tool_calls,
        input.turn.execution.max_tool_wall_clock_secs,
        input.turn.execution.max_tool_retries,
    );

    let tool_snapshot = ctx
        .tool_catalog
        .filtered_snapshot_with_stats(
            ctx.tools,
            input.turn.plan_mode,
            input.turn.request_user_input_enabled,
        )
        .await;
    let current_tools = tool_snapshot.snapshot;
    let has_tools = current_tools.is_some();
    emit_tool_catalog_cache_metrics(
        ctx,
        input.step_count,
        input.active_model,
        tool_snapshot.cache_hit,
        input.turn.plan_mode,
        input.turn.request_user_input_enabled,
        current_tools.as_ref().map_or(0, |defs| defs.len()),
    );

    if let Some(defs) = current_tools.as_ref()
        && !input.turn.prompt_cache_shaping_mode.is_enabled()
    {
        let _ = writeln!(
            system_prompt,
            "\n[Runtime Tool Catalog]\n- version: {}\n- epoch: {}\n- available_tools: {}",
            ctx.tool_catalog.current_version(),
            ctx.tool_catalog.current_epoch(),
            defs.len()
        );
    }

    Ok(PromptAssemblyOutput {
        system_prompt,
        current_tools,
        has_tools,
    })
}

fn resolve_context_management(
    ctx: &TurnProcessingContext<'_>,
    active_model: &str,
) -> Option<serde_json::Value> {
    let harness_config = ctx.vt_cfg.map(|cfg| &cfg.agent.harness);
    let supports_server_compaction = ctx
        .provider_client
        .supports_responses_compaction(active_model);
    let features = FeatureSet::from_config(ctx.vt_cfg);
    if features.auto_compaction_enabled(supports_server_compaction) {
        let context_size = ctx.provider_client.effective_context_size(active_model);
        let configured_threshold =
            harness_config.and_then(|cfg| cfg.auto_compaction_threshold_tokens);

        resolve_compaction_threshold(configured_threshold, context_size).map(|compact_threshold| {
            json!([{
                "type": "compaction",
                "compact_threshold": compact_threshold,
            }])
        })
    } else {
        None
    }
}

pub(super) fn interrupted_provider_error(provider_name: &str) -> anyhow::Error {
    anyhow::Error::new(uni::LLMError::Provider {
        message: vtcode_core::llm::error_display::format_llm_error(
            provider_name,
            "Interrupted by user",
        ),
        metadata: None,
    })
}

pub(super) fn update_previous_response_chain_after_success(
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    provider_name: &str,
    active_model: &str,
    response_request_id: Option<&str>,
) {
    if supports_responses_chaining(provider_name) {
        session_stats.set_previous_response_chain(provider_name, active_model, response_request_id);
    } else {
        session_stats.clear_previous_response_chain();
    }
}

pub(super) async fn build_turn_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    turn_snapshot: &TurnRequestSnapshot,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
    use_streaming: bool,
) -> Result<TurnRequestBuildResult> {
    let prompt_output = assemble_prompt(
        ctx,
        PromptAssemblyInput {
            step_count,
            active_model,
            turn: turn_snapshot,
        },
    )
    .await?;

    let reasoning_effort = ctx.vt_cfg.and_then(|cfg| {
        if turn_snapshot.capabilities.reasoning_effort {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });
    let temperature = if reasoning_effort.is_some()
        && matches!(
            turn_snapshot.provider_name.as_str(),
            "anthropic" | "minimax"
        ) {
        None
    } else {
        Some(0.7)
    };
    let parallel_config =
        if prompt_output.has_tools && turn_snapshot.capabilities.parallel_tool_config {
            parallel_cfg_opt
        } else {
            None
        };
    let tool_choice = if prompt_output.has_tools {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };

    let metadata = match ctx.turn_metadata().await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "Turn metadata collection failed");
            None
        }
    };
    let prompt_cache_key = build_openai_prompt_cache_key(
        turn_snapshot.openai_prompt_cache_enabled,
        &turn_snapshot.openai_prompt_cache_key_mode,
        &ctx.harness_state.run_id.0,
    );
    let previous_response_id = if supports_responses_chaining(&turn_snapshot.provider_name) {
        ctx.session_stats
            .previous_response_id_for(&turn_snapshot.provider_name, active_model)
    } else {
        None
    };
    let context_management = resolve_context_management(ctx, active_model);
    let normalized_messages = ctx
        .context_manager
        .normalize_history_for_request(ctx.working_history);

    let request = uni::LLMRequest {
        messages: normalized_messages,
        system_prompt: Some(Arc::new(prompt_output.system_prompt)),
        tools: prompt_output.current_tools,
        model: active_model.to_string(),
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        metadata,
        context_management,
        previous_response_id,
        prompt_cache_key,
        ..Default::default()
    };

    Ok(TurnRequestBuildResult {
        request,
        has_tools: prompt_output.has_tools,
    })
}
