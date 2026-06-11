use anyhow::Result;
use std::borrow::Cow;
use std::fmt::Write as _;
use std::str::FromStr;
use std::sync::Arc;

use vtcode_core::config::{
    OpenAIPromptCacheKeyMode, PromptCachingConfig, build_openai_prompt_cache_key,
};
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::core::agent::harness_kernel::{
    HarnessRequestPlanInput, SessionToolCatalogSnapshot, build_harness_request_plan,
    stable_system_prefix_hash,
};
use vtcode_core::core::agent::runner::prompt_alignment;
use vtcode_core::llm::provider::{
    self as uni, ParallelToolConfig, prepare_responses_continuation_request,
    supports_responses_chaining,
};
use vtcode_core::prompts::{
    PromptContext, append_runtime_tool_prompt_sections, temporal::generate_temporal_context,
    upsert_harness_limits_section,
};
use vtcode_core::subagents::load_primary_memory_appendix;
use vtcode_core::tools::handlers::anthropic_native_memory_enabled_for_runtime;
use vtcode_core::{
    ActivePrimaryAgent, apply_primary_agent_prompt_context, apply_primary_agent_tool_policy,
};

use super::metrics::{ToolCatalogCacheMetrics, emit_tool_catalog_cache_metrics};
use crate::agent::runloop::unified::incremental_system_prompt::PromptCacheShapingMode;
use crate::agent::runloop::unified::run_loop_context::TurnExecutionSnapshot;
use crate::agent::runloop::unified::turn::compaction::{
    build_server_compaction_context_management, resolve_compaction_threshold,
};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

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

#[derive(Clone)]
pub(super) struct TurnRequestSnapshot {
    pub provider_name: String,
    pub planning_active: bool,
    pub full_auto: bool,
    pub auto_permission: bool,
    pub tool_free_recovery: bool,
    pub recovery_reason: Option<String>,
    pub request_user_input_enabled: bool,
    pub context_window_size: usize,
    pub turn_timeout_secs: u64,
    pub active_model: String,
    pub active_primary_agent: ActivePrimaryAgent,
    pub openai_prompt_cache_enabled: bool,
    pub openai_prompt_cache_key_mode: OpenAIPromptCacheKeyMode,
    pub prompt_cache_shaping_mode: PromptCacheShapingMode,
    pub capabilities: uni::ProviderCapabilities,
    pub execution: TurnExecutionSnapshot,
}

struct PromptAssemblyInput<'a> {
    turn: &'a TurnRequestSnapshot,
}

struct PromptAssemblyOutput {
    system_prompt: String,
    tool_snapshot: SessionToolCatalogSnapshot,
}

pub(super) struct TurnRequestBuildResult {
    pub request: uni::LLMRequest,
    pub has_tools: bool,
    pub runtime_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    pub continuation_messages: Vec<uni::Message>,
}

fn uses_out_of_band_copilot_tools(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case(vtcode_core::copilot::COPILOT_PROVIDER_KEY)
}

fn append_copilot_runtime_guidance(system_prompt: &mut String) {
    let _ = writeln!(
        system_prompt,
        "\n[GitHub Copilot Client Tools]\n- the VT Code tools named in this prompt are exposed as Copilot client tools outside the normal JSON tool list\n- when a tool is needed, emit the actual client tool call instead of describing the call in plain text\n- do not claim a tool was rejected, blocked, or unavailable unless the runtime returned that result"
    );
}

pub(super) fn capture_turn_request_snapshot(
    ctx: &mut TurnProcessingContext<'_>,
    active_model: &str,
    tool_free_recovery: bool,
) -> TurnRequestSnapshot {
    let prompt_cache_config = &ctx.config.prompt_cache;
    let planning_active = ctx.is_planning_active();
    let auto_permission = ctx.full_auto && !planning_active;
    let provider_name = ctx.provider_client.name().to_ascii_lowercase();
    let openai_prompt_cache_enabled = is_openai_prompt_cache_enabled(
        &provider_name,
        prompt_cache_config.enabled,
        prompt_cache_config.providers.openai.enabled,
    );
    let prompt_cache_shaping_mode =
        resolve_prompt_cache_shaping_mode(&provider_name, prompt_cache_config);
    let request_user_input_enabled =
        FeatureSet::from_config(ctx.vt_cfg).request_user_input_enabled(planning_active, true);
    let active_primary_agent = ctx.active_primary_agent.active().clone();
    let active_model = resolve_effective_request_model(active_model, &active_primary_agent);
    let context_window_size = ctx.provider_client.effective_context_size(&active_model);
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
    let capabilities = uni::get_cached_capabilities(&**ctx.provider_client, &active_model);

    TurnRequestSnapshot {
        provider_name,
        planning_active,
        full_auto,
        auto_permission,
        tool_free_recovery,
        recovery_reason: ctx.recovery_reason().map(str::to_string),
        request_user_input_enabled,
        context_window_size,
        turn_timeout_secs,
        active_model,
        active_primary_agent,
        openai_prompt_cache_enabled,
        openai_prompt_cache_key_mode,
        prompt_cache_shaping_mode,
        capabilities,
        execution: ctx.harness_state.execution_snapshot(),
    }
}

pub(super) fn resolve_effective_request_model(
    base_model: &str,
    active_primary_agent: &ActivePrimaryAgent,
) -> String {
    active_primary_agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("inherit"))
        .unwrap_or(base_model)
        .to_string()
}

async fn assemble_prompt(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let prompt_output = build_prompt_output(ctx, PromptAssemblyInput { turn: input.turn }).await?;

    validate_prompt_output_with_rebuild(ctx, input.turn, prompt_output).await
}

async fn build_prompt_output(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let mut system_prompt = ctx
        .context_manager
        .build_system_prompt(
            crate::agent::runloop::unified::context_manager::SystemPromptParams {
                full_auto: input.turn.full_auto,
                auto_permission: input.turn.auto_permission,
                planning_active: input.turn.planning_active,
                request_user_input_enabled: input.turn.request_user_input_enabled,
            },
        )
        .await?;

    append_active_primary_agent_skills(&mut system_prompt, ctx, &input.turn.active_primary_agent);

    upsert_harness_limits_section(
        &mut system_prompt,
        input.turn.execution.max_tool_calls,
        input.turn.execution.max_tool_wall_clock_secs,
        input.turn.execution.max_tool_retries,
    );

    let tool_snapshot = if input.turn.tool_free_recovery {
        let _ = writeln!(
            system_prompt,
            "\n[Recovery Mode]\n- tools_disabled: true\n- answer_mode: summarize only from evidence already collected in this turn\n- if evidence is incomplete, say so explicitly\n- do_not_request_more_tools: true\n- keep_response_brief: true"
        );
        if let Some(reason) = input.turn.recovery_reason.as_deref() {
            let _ = writeln!(system_prompt, "- recovery_reason: {}", reason);
        }
        SessionToolCatalogSnapshot::new(
            ctx.tool_catalog.current_version(),
            ctx.tool_catalog.current_epoch(),
            input.turn.planning_active,
            input.turn.request_user_input_enabled,
            None,
            false,
        )
    } else if !input.turn.capabilities.tools {
        SessionToolCatalogSnapshot::new(
            ctx.tool_catalog.current_version(),
            ctx.tool_catalog.current_epoch(),
            input.turn.planning_active,
            input.turn.request_user_input_enabled,
            None,
            false,
        )
    } else {
        let base_snapshot = ctx
            .tool_catalog
            .filtered_snapshot_with_stats(
                ctx.tools,
                input.turn.planning_active,
                input.turn.request_user_input_enabled,
            )
            .await;
        apply_primary_agent_policy_to_tool_snapshot(base_snapshot, &input.turn.active_primary_agent)
    };

    append_runtime_tool_prompt_sections(
        &mut system_prompt,
        &tool_snapshot,
        !input.turn.prompt_cache_shaping_mode.is_enabled(),
    );

    if tool_snapshot.has_tools() && uses_out_of_band_copilot_tools(&input.turn.provider_name) {
        append_copilot_runtime_guidance(&mut system_prompt);
    }

    Ok(PromptAssemblyOutput {
        system_prompt,
        tool_snapshot,
    })
}

fn apply_primary_agent_policy_to_tool_snapshot(
    snapshot: SessionToolCatalogSnapshot,
    active_primary_agent: &ActivePrimaryAgent,
) -> SessionToolCatalogSnapshot {
    let filtered = apply_primary_agent_tool_policy(snapshot.snapshot, active_primary_agent);
    SessionToolCatalogSnapshot::new(
        snapshot.version,
        snapshot.epoch,
        snapshot.planning_active,
        snapshot.request_user_input_enabled,
        filtered,
        snapshot.cache_hit,
    )
}

fn active_primary_agent_prompt_context(
    ctx: &TurnProcessingContext<'_>,
    agent: &ActivePrimaryAgent,
) -> PromptContext {
    let mut prompt_context = PromptContext::from_workspace_tools(
        ctx.config.workspace.as_path(),
        std::iter::empty::<String>(),
    );
    apply_primary_agent_prompt_context(&mut prompt_context, agent);
    prompt_context
}

fn append_active_primary_agent_skills(
    system_prompt: &mut String,
    ctx: &TurnProcessingContext<'_>,
    agent: &ActivePrimaryAgent,
) {
    if agent.skills.is_empty() {
        return;
    }

    let prompt_context = active_primary_agent_prompt_context(ctx, agent);
    let mut lines = Vec::new();
    lines.push("## Active Primary Agent Skills".to_string());
    lines.push("These skills are scoped to the active primary agent for this request.".to_string());

    if prompt_context.available_skill_metadata.is_empty() {
        for skill in &agent.skills {
            lines.push(format!("- {skill}"));
        }
    } else {
        let mut skills = prompt_context.available_skill_metadata;
        skills.sort_by(|left, right| left.name.cmp(&right.name));
        for skill in skills {
            lines.push(format!("- {}: {}", skill.name, skill.description));
        }
    }

    let _ = writeln!(system_prompt, "\n{}", lines.join("\n"));
}

fn validate_prompt_output_alignment(
    prompt_output: &PromptAssemblyOutput,
    turn: &TurnRequestSnapshot,
) -> Result<(), prompt_alignment::AlignmentError> {
    prompt_alignment::validate_prompt_catalog_alignment(
        &prompt_output.system_prompt,
        &prompt_output.tool_snapshot,
        turn.planning_active,
        turn.request_user_input_enabled,
    )
}

async fn validate_prompt_output_with_rebuild(
    ctx: &mut TurnProcessingContext<'_>,
    turn: &TurnRequestSnapshot,
    prompt_output: PromptAssemblyOutput,
) -> Result<PromptAssemblyOutput> {
    let rebuild_turn = turn.clone();
    prompt_alignment::rebuild_once_on_alignment_mismatch(
        ctx,
        prompt_output,
        move |ctx| {
            let turn = rebuild_turn.clone();
            Box::pin(
                async move { build_prompt_output(ctx, PromptAssemblyInput { turn: &turn }).await },
            )
        },
        |_, prompt_output| validate_prompt_output_alignment(prompt_output, turn),
        "prompt/catalog alignment mismatch during unified request assembly; rebuilding prompt",
        "prompt/catalog alignment mismatch persisted after unified prompt rebuild",
    )
    .await
}

fn resolve_context_management(
    ctx: &TurnProcessingContext<'_>,
    turn: &TurnRequestSnapshot,
    active_model: &str,
) -> Option<serde_json::Value> {
    let Some(vt_cfg) = ctx.vt_cfg else {
        return resolve_server_compaction_context_management(turn, None, None);
    };

    if turn.provider_name.eq_ignore_ascii_case("anthropic") {
        return build_anthropic_context_management(vt_cfg, turn, active_model);
    }

    resolve_server_compaction_context_management(
        turn,
        Some(vt_cfg),
        vt_cfg.agent.harness.auto_compaction_threshold_tokens,
    )
}

fn resolve_server_compaction_context_management(
    turn: &TurnRequestSnapshot,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
    configured_threshold: Option<u64>,
) -> Option<serde_json::Value> {
    let features = FeatureSet::from_config(vt_cfg);
    if !features.auto_compaction_enabled(turn.capabilities.responses_compaction) {
        return None;
    }

    build_server_compaction_context_management(configured_threshold, turn.context_window_size)
}

fn build_anthropic_context_management(
    vt_cfg: &vtcode_core::config::loader::VTCodeConfig,
    turn: &TurnRequestSnapshot,
    active_model: &str,
) -> Option<serde_json::Value> {
    if !turn.capabilities.context_edits {
        return None;
    }

    let mut edits = Vec::new();
    let clearing = &vt_cfg.agent.harness.tool_result_clearing;
    if clearing.enabled {
        let mut edit = serde_json::Map::from_iter([
            (
                "type".to_string(),
                serde_json::Value::String("clear_tool_uses_20250919".to_string()),
            ),
            (
                "trigger".to_string(),
                serde_json::json!({
                    "type": "input_tokens",
                    "value": clearing.trigger_tokens,
                }),
            ),
            (
                "keep".to_string(),
                serde_json::json!({
                    "type": "tool_uses",
                    "value": clearing.keep_tool_uses,
                }),
            ),
            (
                "clear_at_least".to_string(),
                serde_json::json!({
                    "type": "input_tokens",
                    "value": clearing.clear_at_least_tokens,
                }),
            ),
            (
                "clear_tool_inputs".to_string(),
                serde_json::Value::Bool(clearing.clear_tool_inputs),
            ),
        ]);

        if anthropic_native_memory_enabled_for_runtime(
            vtcode_core::config::models::Provider::from_str(&turn.provider_name).ok(),
            active_model,
            Some(vt_cfg),
        ) {
            edit.insert(
                "exclude_tools".to_string(),
                serde_json::json!([vtcode_core::config::constants::tools::MEMORY]),
            );
        }

        edits.push(serde_json::Value::Object(edit));
    }

    if vt_cfg.agent.harness.auto_compaction_enabled
        && let Some(trigger_tokens) = resolve_compaction_threshold(
            vt_cfg.agent.harness.auto_compaction_threshold_tokens,
            turn.context_window_size,
        )
    {
        let mut compact_edit = serde_json::Map::new();
        compact_edit.insert(
            "type".to_string(),
            serde_json::Value::String("compact_20260112".to_string()),
        );
        compact_edit.insert(
            "trigger".to_string(),
            serde_json::json!({
                "type": "input_tokens",
                "value": trigger_tokens,
            }),
        );

        if let Some(instructions) = &vt_cfg.agent.harness.auto_compaction_instructions {
            compact_edit.insert(
                "instructions".to_string(),
                serde_json::Value::String(instructions.clone()),
            );
        }

        if vt_cfg.agent.harness.auto_compaction_pause_after {
            compact_edit.insert(
                "pause_after_compaction".to_string(),
                serde_json::Value::Bool(true),
            );
        }

        edits.push(serde_json::Value::Object(compact_edit));
    }

    (!edits.is_empty()).then(|| {
        serde_json::json!({
            "edits": edits,
        })
    })
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
    provider_supports_responses_compaction: bool,
    active_model: &str,
    response_request_id: Option<&str>,
    messages: &[uni::Message],
) {
    if supports_responses_chaining(provider_name, provider_supports_responses_compaction) {
        session_stats.set_previous_response_chain(
            provider_name,
            active_model,
            response_request_id,
            messages,
        );
    }
}

fn prepare_responses_request_history<'a>(
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    provider_name: &str,
    provider_supports_responses_compaction: bool,
    active_model: &str,
    messages: &'a [uni::Message],
) -> (Cow<'a, [uni::Message]>, Option<String>) {
    let prepared = prepare_responses_continuation_request(
        provider_name,
        provider_supports_responses_compaction,
        messages,
        session_stats.previous_response_chain_for(provider_name, active_model),
    );
    if prepared.clear_stale_chain {
        session_stats.clear_previous_response_chain_for(provider_name, active_model);
    }

    (prepared.messages, prepared.previous_response_id)
}

fn prepend_request_context_message(
    mut messages: Vec<uni::Message>,
    context_message: Option<uni::Message>,
) -> Vec<uni::Message> {
    let Some(context_message) = context_message else {
        return messages;
    };

    let mut request_messages = Vec::with_capacity(messages.len() + 1);
    request_messages.push(context_message);
    request_messages.append(&mut messages);
    request_messages
}

fn inject_request_context_messages(
    messages: Vec<uni::Message>,
    editor_context_message: Option<uni::Message>,
    primary_agent_context_message: Option<uni::Message>,
) -> Vec<uni::Message> {
    let messages = prepend_request_context_message(messages, editor_context_message);
    insert_after_latest_user_message(messages, primary_agent_context_message)
}

fn insert_after_latest_user_message(
    mut messages: Vec<uni::Message>,
    context_message: Option<uni::Message>,
) -> Vec<uni::Message> {
    let Some(context_message) = context_message else {
        return messages;
    };

    let insert_at = messages
        .iter()
        .rposition(|message| message.role == uni::MessageRole::User)
        .map_or(messages.len(), |index| index + 1);
    messages.insert(insert_at, context_message);
    messages
}

fn resolve_effective_reasoning_effort(
    cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
    turn_snapshot: &TurnRequestSnapshot,
) -> Option<vtcode_core::config::types::ReasoningEffortLevel> {
    if !turn_snapshot.capabilities.reasoning_effort || turn_snapshot.tool_free_recovery {
        return None;
    }

    turn_snapshot
        .active_primary_agent
        .reasoning_effort
        .as_deref()
        .and_then(vtcode_core::config::types::ReasoningEffortLevel::parse)
        .or_else(|| cfg.map(|cfg| cfg.agent.reasoning_effort))
}

async fn request_primary_agent_context_message(
    ctx: &TurnProcessingContext<'_>,
    turn_snapshot: &TurnRequestSnapshot,
    tool_snapshot: &SessionToolCatalogSnapshot,
    reasoning_effort: Option<vtcode_core::config::types::ReasoningEffortLevel>,
) -> uni::Message {
    let agent = &turn_snapshot.active_primary_agent;
    let block = render_primary_agent_runtime_context(
        ctx,
        turn_snapshot,
        tool_snapshot,
        agent,
        reasoning_effort,
    )
    .await;
    uni::Message::user(block)
}

async fn render_primary_agent_runtime_context(
    ctx: &TurnProcessingContext<'_>,
    turn_snapshot: &TurnRequestSnapshot,
    tool_snapshot: &SessionToolCatalogSnapshot,
    agent: &ActivePrimaryAgent,
    reasoning_effort: Option<vtcode_core::config::types::ReasoningEffortLevel>,
) -> String {
    let mut lines = Vec::new();
    lines.push("## Active Primary Agent Runtime State".to_string());
    lines.push(format!("- Active agent: {}", agent.display_name));
    lines.push(format!("- Spec name: {}", agent.identity.name));
    lines.push(format!("- Request model: {}", turn_snapshot.active_model));
    if let Some(model) = agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("inherit"))
    {
        lines.push(format!("- Agent model: {model}"));
    }
    if let Some(effort) = reasoning_effort {
        lines.push(format!("- Request reasoning effort: {}", effort.as_str()));
    }
    if let Some(raw_effort) = agent
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|effort| !effort.is_empty())
    {
        lines.push(format!("- Agent reasoning effort: {raw_effort}"));
    }
    lines.push(format!(
        "- Session state: planning_workflow={}, auto_permission={}, full_auto={}",
        turn_snapshot.planning_active, turn_snapshot.auto_permission, turn_snapshot.full_auto
    ));
    lines.push(format!(
        "- Active primary permission default: {}",
        permission_default_label(agent.permissions.default)
    ));
    lines.push(format!(
        "- Effective request tools: {}",
        render_tool_names(tool_snapshot)
    ));
    if let Some(tools) = agent.tools.as_ref().filter(|tools| !tools.is_empty()) {
        lines.push(format!(
            "- Primary-agent tool allow-list: {}",
            tools.join(", ")
        ));
    }
    if !agent.disallowed_tools.is_empty() {
        lines.push(format!(
            "- Primary-agent disallowed tools: {}",
            agent.disallowed_tools.join(", ")
        ));
    }
    if !agent.skills.is_empty() {
        let prompt_context = active_primary_agent_prompt_context(ctx, agent);
        if prompt_context.available_skill_metadata.is_empty() {
            lines.push(format!(
                "- Active primary skills: {}",
                agent.skills.join(", ")
            ));
        } else {
            let mut names = prompt_context
                .available_skill_metadata
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>();
            names.sort_unstable();
            lines.push(format!("- Active primary skills: {}", names.join(", ")));
        }
    }
    if let Some(cfg) = ctx.vt_cfg
        && cfg.agent.include_temporal_context
    {
        lines.push(
            generate_temporal_context(cfg.agent.temporal_context_use_utc)
                .trim()
                .to_string(),
        );
    }

    lines.push("### Instructions".to_string());
    lines.push(agent.instructions.trim().to_string());
    if let Some(memory_appendix) = active_primary_agent_memory_appendix(ctx, agent) {
        lines.push("### Memory Appendix".to_string());
        lines.push(memory_appendix);
    }
    lines.join("\n")
}

fn active_primary_agent_memory_appendix(
    ctx: &TurnProcessingContext<'_>,
    agent: &ActivePrimaryAgent,
) -> Option<String> {
    match load_primary_memory_appendix(
        ctx.config.workspace.as_path(),
        agent.identity.name.as_str(),
        agent.memory,
    ) {
        Ok(appendix) => appendix,
        Err(err) => {
            tracing::warn!(
                agent_name = %agent.identity.name,
                error = %err,
                "Failed to load active primary-agent memory appendix"
            );
            None
        }
    }
}

fn render_tool_names(tool_snapshot: &SessionToolCatalogSnapshot) -> String {
    let Some(tools) = tool_snapshot.snapshot.as_deref() else {
        return "none".to_string();
    };
    if tools.is_empty() {
        return "none".to_string();
    }

    tools
        .iter()
        .map(|tool| {
            tool.function
                .as_ref()
                .map(|function| function.name.as_str())
                .unwrap_or(tool.tool_type.as_str())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn permission_default_label(
    default: vtcode_config::core::permissions::PermissionDefault,
) -> &'static str {
    match default {
        vtcode_config::core::permissions::PermissionDefault::Ask => "ask",
        vtcode_config::core::permissions::PermissionDefault::Allow => "allow",
        vtcode_config::core::permissions::PermissionDefault::Auto => "auto",
        vtcode_config::core::permissions::PermissionDefault::Deny => "deny",
    }
}

pub(super) async fn build_turn_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    _active_model: &str,
    turn_snapshot: &TurnRequestSnapshot,
    max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
    use_streaming: bool,
) -> Result<TurnRequestBuildResult> {
    let request_model = turn_snapshot.active_model.as_str();
    let prompt_output = assemble_prompt(
        ctx,
        PromptAssemblyInput {
            turn: turn_snapshot,
        },
    )
    .await?;

    let reasoning_effort = resolve_effective_reasoning_effort(ctx.vt_cfg, turn_snapshot);
    let temperature = if reasoning_effort.is_some()
        && matches!(
            turn_snapshot.provider_name.as_str(),
            "anthropic" | "minimax"
        ) {
        None
    } else {
        Some(0.7)
    };
    let parallel_config = if prompt_output.tool_snapshot.has_tools()
        && !turn_snapshot.tool_free_recovery
        && turn_snapshot.capabilities.parallel_tool_config
    {
        parallel_cfg_opt
    } else {
        None
    };
    let use_out_of_band_copilot_tools =
        uses_out_of_band_copilot_tools(&turn_snapshot.provider_name);
    let tool_choice = if turn_snapshot.tool_free_recovery {
        Some(uni::ToolChoice::none())
    } else if use_out_of_band_copilot_tools {
        None
    } else if prompt_output.tool_snapshot.has_tools() {
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
        ctx.session_stats.prompt_cache_lineage_id(),
    );
    let stable_prefix_hash = stable_system_prefix_hash(&prompt_output.system_prompt);
    let tool_catalog_hash = prompt_output.tool_snapshot.tool_catalog_hash;
    let prefix_change_reason = ctx.session_stats.record_prompt_cache_fingerprint(
        request_model,
        stable_prefix_hash,
        tool_catalog_hash,
    );
    emit_tool_catalog_cache_metrics(
        ctx,
        ToolCatalogCacheMetrics {
            step_count,
            model: request_model,
            cache_hit: prompt_output.tool_snapshot.cache_hit,
            planning_active: turn_snapshot.planning_active,
            request_user_input_enabled: turn_snapshot.request_user_input_enabled,
            available_tools: prompt_output.tool_snapshot.available_tools(),
            stable_prefix_hash,
            tool_catalog_hash,
            prefix_change_reason,
        },
    );
    let context_management = resolve_context_management(ctx, turn_snapshot, request_model);
    let continuation_messages = ctx
        .context_manager
        .normalize_history_for_request(ctx.working_history);
    let (request_messages, previous_response_id) = prepare_responses_request_history(
        ctx.session_stats,
        &turn_snapshot.provider_name,
        turn_snapshot.capabilities.responses_compaction,
        request_model,
        &continuation_messages,
    );
    let request_messages = request_messages.into_owned();
    let primary_agent_context_message = Some(
        request_primary_agent_context_message(
            ctx,
            turn_snapshot,
            &prompt_output.tool_snapshot,
            reasoning_effort,
        )
        .await,
    );
    let request_messages = inject_request_context_messages(
        request_messages,
        ctx.context_manager.request_editor_context_message(),
        primary_agent_context_message,
    );
    let request_plan = build_harness_request_plan(HarnessRequestPlanInput {
        messages: request_messages,
        system_prompt: prompt_output.system_prompt,
        tools: if use_out_of_band_copilot_tools {
            None
        } else {
            prompt_output.tool_snapshot.snapshot.clone()
        },
        model: turn_snapshot.active_model.clone(),
        max_tokens: max_tokens_opt,
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        verbosity: None,
        metadata,
        context_management,
        previous_response_id,
        prompt_cache_key,
        prompt_cache_profile: ctx.session_stats.prompt_cache_profile(),
        tool_catalog_hash,
    });

    Ok(TurnRequestBuildResult {
        request: request_plan.request,
        has_tools: prompt_output.tool_snapshot.has_tools(),
        runtime_tools: prompt_output.tool_snapshot.snapshot,
        continuation_messages,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use serde_json::json;
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
    use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::types::ReasoningEffortLevel;
    use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
    use vtcode_core::llm::provider::{self as uni, ToolDefinition};
    use vtcode_core::prompts::append_runtime_tool_prompt_sections;
    use vtcode_core::{EditorContextSnapshot, EditorFileContext};

    use super::{
        PromptAssemblyOutput, build_turn_request, capture_turn_request_snapshot,
        stable_system_prefix_hash, update_previous_response_chain_after_success,
        validate_prompt_output_alignment,
    };
    use crate::agent::runloop::unified::turn::compaction::build_server_compaction_context_management;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    fn test_primary_agent_spec(name: &str, prompt: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: prompt.to_string(),
            tools: Some(vec!["unified_search".to_string()]),
            disallowed_tools: vec!["shell".to_string()],
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: vtcode_config::AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
        }
    }

    fn named_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(
            name.to_string(),
            format!("{name} tool"),
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        )
    }

    fn request_tool_names(request: &uni::LLMRequest) -> Vec<String> {
        request
            .tools
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect()
    }

    fn non_runtime_request_messages(request: &uni::LLMRequest) -> Vec<uni::Message> {
        request
            .messages
            .iter()
            .filter(|message| !is_primary_agent_runtime_context_message(message))
            .cloned()
            .collect()
    }

    fn is_primary_agent_runtime_context_message(message: &uni::Message) -> bool {
        message.role == uni::MessageRole::User
            && message
                .content
                .as_text()
                .starts_with("## Active Primary Agent Runtime State")
    }

    #[tokio::test]
    async fn recovery_request_omits_tools_and_disables_tool_choice() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.select_primary_agent_from_specs(
            &[vtcode_config::builtin_primary_build_agent()],
            "build",
        );
        backing
            .add_tool_definition(ToolDefinition::function(
                "unified_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    }
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.reasoning_effort = ReasoningEffortLevel::High;
        ctx.vt_cfg = Some(&vt_cfg);
        ctx.activate_recovery("loop detector");

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", true);
        let mut normal_snapshot = snapshot.clone();
        normal_snapshot.tool_free_recovery = false;
        normal_snapshot.capabilities.reasoning_effort = true;

        let normal_built = build_turn_request(
            &mut ctx,
            1,
            "noop-model",
            &normal_snapshot,
            Some(320),
            None,
            false,
        )
        .await
        .expect("normal request should build");
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("recovery request should build");

        assert_eq!(
            normal_built.request.reasoning_effort,
            Some(ReasoningEffortLevel::High)
        );
        assert!(built.request.reasoning_effort.is_none());
        assert!(!built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(matches!(
            built.request.tool_choice,
            Some(uni::ToolChoice::None)
        ));
        assert_eq!(built.request.max_tokens, Some(320));

        let system_prompt = built
            .request
            .system_prompt
            .as_ref()
            .expect("system prompt")
            .as_str();
        assert!(system_prompt.contains("[Recovery Mode]"));
        assert!(system_prompt.contains("do_not_request_more_tools: true"));
        assert!(system_prompt.contains("recovery_reason: loop detector"));
        assert!(!system_prompt.contains("<budget:token_budget>"));
    }

    #[tokio::test]
    async fn text_only_provider_request_omits_tools_and_tool_choice() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "unified_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    }
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.capabilities.tools = false;
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("text-only request should build");

        assert!(!built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(built.request.tool_choice.is_none());

        let system_prompt = built
            .request
            .system_prompt
            .as_ref()
            .expect("system prompt")
            .as_str();
        assert!(!system_prompt.contains("[Runtime Tool Catalog]"));
    }

    #[tokio::test]
    async fn copilot_request_keeps_runtime_tools_out_of_band() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "unified_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    }
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "copilot-gpt-5.4", false);
        snapshot.provider_name = vtcode_core::copilot::COPILOT_PROVIDER_KEY.to_string();
        snapshot.capabilities.tools = true;
        let built = build_turn_request(
            &mut ctx,
            1,
            "copilot-gpt-5.4",
            &snapshot,
            Some(320),
            None,
            true,
        )
        .await
        .expect("copilot request should build");

        assert!(built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(built.request.tool_choice.is_none());
        assert_eq!(
            built.runtime_tools.as_ref().map(|tools| tools.len()),
            Some(1)
        );

        let system_prompt = built
            .request
            .system_prompt
            .as_ref()
            .expect("system prompt")
            .as_str();
        assert!(system_prompt.contains("[GitHub Copilot Client Tools]"));
        assert!(system_prompt.contains("emit the actual client tool call"));
    }

    #[tokio::test]
    async fn openai_responses_chain_uses_incremental_history_only() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history
            .push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "openai",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("openai request should build");

        assert_eq!(
            built.request.previous_response_id.as_deref(),
            Some("resp_123")
        );
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![uni::Message::user("continue".to_string())]
        );
    }

    #[tokio::test]
    async fn compatible_provider_responses_chain_uses_incremental_history_only() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history
            .push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "mycorp",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.provider_name = "mycorp".to_string();
        snapshot.capabilities.responses_compaction = true;
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("compatible provider request should build");

        assert_eq!(
            built.request.previous_response_id.as_deref(),
            Some("resp_123")
        );
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![uni::Message::user("continue".to_string())]
        );
    }

    #[tokio::test]
    async fn non_openai_responses_chain_keeps_full_history() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history
            .push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "gemini",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.provider_name = "gemini".to_string();
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("gemini request should build");

        assert_eq!(
            built.request.previous_response_id.as_deref(),
            Some("resp_123")
        );
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn request_build_moves_editor_context_out_of_system_prompt() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager.set_workspace_root(workspace.path());
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/main.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );
        ctx.working_history
            .push(uni::Message::user("hello".to_string()));

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build");

        let system_prompt = built
            .request
            .system_prompt
            .as_ref()
            .expect("system prompt")
            .as_str();
        assert!(!system_prompt.contains("## Active Editor Context"));
        let non_runtime_messages = non_runtime_request_messages(&built.request);
        assert_eq!(non_runtime_messages.len(), 2);
        assert_eq!(non_runtime_messages[0].role, uni::MessageRole::User);
        assert!(
            non_runtime_messages[0]
                .content
                .as_text()
                .contains("## Active Editor Context")
        );
        assert!(
            non_runtime_messages[0]
                .content
                .as_text()
                .contains("- Active file: src/main.rs")
        );
        assert!(
            non_runtime_messages[0]
                .content
                .as_text()
                .contains("- Language: Rust")
        );
        assert_eq!(
            non_runtime_messages[1],
            uni::Message::user("hello".to_string())
        );
        assert_eq!(
            built.continuation_messages,
            vec![uni::Message::user("hello".to_string())]
        );
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_is_request_only_after_latest_user() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "unified_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" }
                    }
                }),
            ))
            .await;
        let spec = test_primary_agent_spec("planner", "Plan carefully before editing.");
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(built.request.messages.len(), 2);
        assert_eq!(
            built.request.messages[0],
            uni::Message::user("hello".to_string())
        );
        assert_eq!(built.request.messages[1].role, uni::MessageRole::User);
        let runtime_context = built.request.messages[1].content.as_text();
        assert!(runtime_context.contains("## Active Primary Agent Runtime State"));
        assert!(runtime_context.contains("- Active agent: planner"));
        assert!(runtime_context.contains("- Effective request tools: unified_search"));
        assert!(runtime_context.contains(
            "- Session state: planning_workflow=false, auto_permission=false, full_auto=false"
        ));
        assert!(runtime_context.contains("- Active primary permission default: deny"));
        assert!(runtime_context.contains("Plan carefully before editing."));
        assert_eq!(
            built.continuation_messages,
            vec![uni::Message::user("hello".to_string())]
        );
    }

    #[tokio::test]
    async fn active_primary_agent_memory_appendix_uses_canonical_name_for_alias_selection() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let workspace = backing.workspace_path().to_path_buf();
        std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/reviewer"))
            .expect("canonical memory dir");
        std::fs::write(
            workspace.join(".vtcode/agent-memory/reviewer/MEMORY.md"),
            "# Reviewer Memory\n\n- Canonical reviewer memory.\n",
        )
        .expect("canonical memory");
        std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/critic"))
            .expect("alias memory dir");
        std::fs::write(
            workspace.join(".vtcode/agent-memory/critic/MEMORY.md"),
            "# Critic Memory\n\n- Alias memory must not load.\n",
        )
        .expect("alias memory");

        let mut spec = test_primary_agent_spec("reviewer", "Review carefully.");
        spec.memory = Some(SubagentMemoryScope::Project);
        spec.aliases = vec!["critic".to_string()];
        backing.select_primary_agent_from_specs(&[spec], "critic");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        let runtime_context = built.request.messages[1].content.as_text();
        assert!(runtime_context.contains("### Memory Appendix"));
        assert!(runtime_context.contains("Primary-agent memory file:"));
        assert!(runtime_context.contains(".vtcode/agent-memory/reviewer/MEMORY.md"));
        assert!(runtime_context.contains("Canonical reviewer memory."));
        assert!(!runtime_context.contains("Alias memory must not load."));
        assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
        assert!(!runtime_context.contains("Read and maintain `MEMORY.md`"));
    }

    #[tokio::test]
    async fn active_primary_agent_missing_memory_is_noop_and_does_not_expand_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(named_tool("unified_search"))
            .await;
        backing
            .add_tool_definition(named_tool("unified_file"))
            .await;
        let workspace = backing.workspace_path().to_path_buf();
        let memory_dir = workspace.join(".vtcode/agent-memory/planner");

        let mut spec = test_primary_agent_spec("planner", "Plan carefully.");
        spec.memory = Some(SubagentMemoryScope::Project);
        spec.tools = Some(vec!["unified_search".to_string()]);
        spec.disallowed_tools = Vec::new();
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        let runtime_context = built.request.messages[1].content.as_text();
        assert!(!runtime_context.contains("### Memory Appendix"));
        assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
        assert!(!memory_dir.exists());
        assert_eq!(request_tool_names(&built.request), vec!["unified_search"]);
    }

    #[tokio::test]
    async fn active_primary_agent_memory_appendix_is_replaced_on_switch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let workspace = backing.workspace_path().to_path_buf();
        for (agent, memory) in [
            ("planner", "Planner-only durable note."),
            ("reviewer", "Reviewer-only durable note."),
        ] {
            let memory_dir = workspace.join(".vtcode/agent-memory").join(agent);
            std::fs::create_dir_all(&memory_dir).expect("memory dir");
            std::fs::write(memory_dir.join("MEMORY.md"), format!("- {memory}\n")).expect("memory");
        }

        let mut planner = test_primary_agent_spec("planner", "Planner instructions.");
        planner.memory = Some(SubagentMemoryScope::Project);
        let mut reviewer = test_primary_agent_spec("reviewer", "Reviewer instructions.");
        reviewer.memory = Some(SubagentMemoryScope::Project);

        backing.select_primary_agent_from_specs(std::slice::from_ref(&planner), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&reviewer), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.clear();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_runtime = first_built.request.messages[1].content.as_text();
        let second_runtime = second_built.request.messages[1].content.as_text();
        assert!(first_runtime.contains("Planner-only durable note."));
        assert!(!first_runtime.contains("Reviewer-only durable note."));
        assert!(second_runtime.contains("Reviewer-only durable note."));
        assert!(!second_runtime.contains("Planner-only durable note."));
    }

    #[tokio::test]
    async fn active_primary_agent_tool_allow_list_intersects_baseline_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(named_tool("unified_search"))
            .await;
        backing
            .add_tool_definition(named_tool("unified_file"))
            .await;
        backing
            .add_tool_definition(named_tool("unified_exec"))
            .await;
        let mut spec = test_primary_agent_spec("planner", "Use limited tools.");
        spec.tools = Some(vec![
            "unified_search".to_string(),
            "missing_tool".to_string(),
        ]);
        spec.disallowed_tools = Vec::new();
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(request_tool_names(&built.request), vec!["unified_search"]);
        assert!(built.has_tools);
    }

    #[tokio::test]
    async fn active_primary_agent_deny_list_applies_after_allow_list() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(named_tool("unified_search"))
            .await;
        backing
            .add_tool_definition(named_tool("unified_file"))
            .await;
        let mut spec = test_primary_agent_spec("planner", "Use deterministic tools.");
        spec.tools = Some(vec![
            "unified_search".to_string(),
            "unified_file".to_string(),
        ]);
        spec.disallowed_tools = vec!["unified_search".to_string()];
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(request_tool_names(&built.request), vec!["unified_file"]);
        assert!(
            built.request.messages[1]
                .content
                .as_text()
                .contains("- Effective request tools: unified_file")
        );
    }

    #[tokio::test]
    async fn unconstrained_primary_agent_falls_back_to_baseline_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.select_primary_agent_from_specs(
            &[vtcode_config::builtin_primary_build_agent()],
            "build",
        );
        backing
            .add_tool_definition(named_tool("unified_search"))
            .await;
        backing
            .add_tool_definition(named_tool("unified_file"))
            .await;

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(
            request_tool_names(&built.request),
            vec!["unified_search", "unified_file"]
        );
        assert_eq!(
            built.continuation_messages,
            vec![uni::Message::user("hello".to_string())]
        );
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_does_not_stale_response_chains() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let spec = test_primary_agent_spec("planner", "Use the active primary agent.");
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.extend(prior_messages.clone());
            ctx.working_history
                .push(uni::Message::user("continue".to_string()));
            ctx.session_stats.set_previous_response_chain(
                "openai",
                "noop-model",
                Some("resp_123"),
                &prior_messages,
            );
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(
            built.request.previous_response_id.as_deref(),
            Some("resp_123")
        );
        assert_eq!(built.request.messages.len(), 2);
        assert_eq!(
            built.request.messages[0],
            uni::Message::user("continue".to_string())
        );
        assert!(
            built.request.messages[1]
                .content
                .as_text()
                .contains("## Active Primary Agent Runtime State")
        );
        assert_eq!(
            built.continuation_messages,
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_keeps_stable_prompt_cache_friendly() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.include_temporal_context = true;
        cfg.prompt_cache.cache_friendly_prompt_shaping = true;
        let cfg = Box::leak(Box::new(cfg));
        let first = test_primary_agent_spec("planner", "Planner instructions.");
        let second = test_primary_agent_spec("reviewer", "Reviewer instructions.");

        backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history.clear();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_system = first_built.request.system_prompt.as_ref().expect("system");
        let second_system = second_built.request.system_prompt.as_ref().expect("system");
        assert_eq!(first_system, second_system);
        assert_eq!(
            stable_system_prefix_hash(first_system),
            stable_system_prefix_hash(second_system)
        );
        assert!(!first_system.contains("Planner instructions."));
        assert!(!second_system.contains("Reviewer instructions."));
        assert!(!first_system.contains("Current date and time"));

        let first_runtime = first_built.request.messages[1].content.as_text();
        let second_runtime = second_built.request.messages[1].content.as_text();
        assert!(first_runtime.contains("Planner instructions."));
        assert!(second_runtime.contains("Reviewer instructions."));
        assert!(first_runtime.contains("Current date and time"));
        assert!(second_runtime.contains("Current date and time"));
    }

    #[tokio::test]
    async fn active_primary_agent_skills_are_request_scoped_on_switch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut first = test_primary_agent_spec("planner", "Planner instructions.");
        first.skills = vec!["alpha".to_string()];
        let mut second = test_primary_agent_spec("reviewer", "Reviewer instructions.");
        second.skills = vec!["beta".to_string()];

        backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.clear();
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_system = first_built.request.system_prompt.as_ref().expect("system");
        let second_system = second_built.request.system_prompt.as_ref().expect("system");
        assert!(first_system.contains("## Active Primary Agent Skills"));
        assert!(first_system.contains("- alpha"));
        assert!(!first_system.contains("- beta"));
        assert!(second_system.contains("## Active Primary Agent Skills"));
        assert!(second_system.contains("- beta"));
        assert!(!second_system.contains("- alpha"));

        let first_runtime = first_built.request.messages[1].content.as_text();
        let second_runtime = second_built.request.messages[1].content.as_text();
        assert!(first_runtime.contains("- Active primary skills: alpha"));
        assert!(second_runtime.contains("- Active primary skills: beta"));
    }

    #[tokio::test]
    async fn primary_agent_model_and_reasoning_feed_request_metadata() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut spec = test_primary_agent_spec("planner", "Use agent metadata.");
        spec.model = Some("overlay-model".to_string());
        spec.reasoning_effort = Some("high".to_string());
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let mut cfg = VTCodeConfig::default();
        cfg.agent.reasoning_effort = ReasoningEffortLevel::Medium;
        let cfg = Box::leak(Box::new(cfg));
        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history
                .push(uni::Message::user("hello".to_string()));
            let mut snapshot = capture_turn_request_snapshot(&mut ctx, "base-model", false);
            assert_eq!(snapshot.active_model, "overlay-model");
            snapshot.capabilities.reasoning_effort = true;
            build_turn_request(&mut ctx, 1, "base-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(built.request.model, "overlay-model");
        assert_eq!(
            built.request.reasoning_effort,
            Some(ReasoningEffortLevel::High)
        );
        let runtime_context = built.request.messages[1].content.as_text();
        assert!(runtime_context.contains("- Request model: overlay-model"));
        assert!(runtime_context.contains("- Request reasoning effort: high"));
    }

    #[tokio::test]
    async fn editor_context_does_not_stale_openai_response_chains() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager.set_workspace_root(workspace.path());
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/main.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );
        ctx.working_history
            .push(uni::Message::user("hello".to_string()));

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let first =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build");
        update_previous_response_chain_after_success(
            ctx.session_stats,
            "openai",
            false,
            "noop-model",
            Some("resp_123"),
            &first.continuation_messages,
        );

        ctx.working_history
            .push(uni::Message::user("continue".to_string()));
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/lib.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );

        let second =
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build");

        assert_eq!(
            second.request.previous_response_id.as_deref(),
            Some("resp_123")
        );
        let non_runtime_messages = non_runtime_request_messages(&second.request);
        assert_eq!(non_runtime_messages.len(), 2);
        assert_eq!(non_runtime_messages[0].role, uni::MessageRole::User);
        assert!(
            non_runtime_messages[0]
                .content
                .as_text()
                .contains("## Active Editor Context")
        );
        assert!(
            non_runtime_messages[0]
                .content
                .as_text()
                .contains("- Active file: src/lib.rs")
        );
        assert_eq!(
            non_runtime_messages[1],
            uni::Message::user("continue".to_string())
        );
        assert_eq!(
            second.continuation_messages,
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string()),
            ]
        );
    }

    #[test]
    fn server_supported_request_build_keeps_context_management_payload() {
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(512);

        let payload = build_server_compaction_context_management(
            cfg.agent.harness.auto_compaction_threshold_tokens,
            2_000,
        );

        assert_eq!(
            payload,
            Some(json!([{
                "type": "compaction",
                "compact_threshold": 512,
            }]))
        );
    }

    #[tokio::test]
    async fn anthropic_request_build_combines_clearing_and_compaction_when_enabled() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.provider = "anthropic".to_string();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(100_000);
        cfg.agent.harness.tool_result_clearing.enabled = true;
        cfg.agent.harness.tool_result_clearing.trigger_tokens = 120_000;
        cfg.agent.harness.tool_result_clearing.keep_tool_uses = 5;
        cfg.agent.harness.tool_result_clearing.clear_at_least_tokens = 40_000;
        cfg.provider.anthropic.memory.enabled = true;
        let cfg = Box::leak(Box::new(cfg));

        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-sonnet-4-6", false);
        snapshot.provider_name = "anthropic".to_string();
        snapshot.capabilities.context_edits = true;

        let built = build_turn_request(
            &mut ctx,
            1,
            "claude-sonnet-4-6",
            &snapshot,
            Some(320),
            None,
            false,
        )
        .await
        .expect("anthropic request should build");

        assert_eq!(
            built.request.context_management,
            Some(json!({
                "edits": [{
                    "type": "clear_tool_uses_20250919",
                    "trigger": { "type": "input_tokens", "value": 120000 },
                    "keep": { "type": "tool_uses", "value": 5 },
                    "clear_at_least": { "type": "input_tokens", "value": 40000 },
                    "clear_tool_inputs": false,
                    "exclude_tools": ["memory"],
                }, {
                    "type": "compact_20260112",
                    "trigger": { "type": "input_tokens", "value": 100000 },
                }]
            }))
        );

        let mut compaction_only_cfg = VTCodeConfig::default();
        compaction_only_cfg.agent.provider = "anthropic".to_string();
        compaction_only_cfg.agent.harness.auto_compaction_enabled = true;
        compaction_only_cfg
            .agent
            .harness
            .auto_compaction_threshold_tokens = Some(90_000);
        ctx.vt_cfg = Some(Box::leak(Box::new(compaction_only_cfg)));
        let built = build_turn_request(
            &mut ctx,
            1,
            "claude-sonnet-4-6",
            &snapshot,
            Some(320),
            None,
            false,
        )
        .await
        .expect("compaction-only anthropic request should build");
        assert_eq!(
            built.request.context_management,
            Some(json!({
                "edits": [{
                    "type": "compact_20260112",
                    "trigger": { "type": "input_tokens", "value": 90000 },
                }]
            }))
        );

        ctx.vt_cfg = Some(Box::leak(Box::new(VTCodeConfig::default())));
        let built = build_turn_request(
            &mut ctx,
            1,
            "claude-sonnet-4-6",
            &snapshot,
            Some(320),
            None,
            false,
        )
        .await
        .expect("disabled anthropic request should build");
        assert!(built.request.context_management.is_none());
    }

    #[tokio::test]
    async fn openai_request_build_keeps_existing_compaction_payload() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(512);
        let cfg = Box::leak(Box::new(cfg));

        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "gpt-5", false);
        snapshot.capabilities.responses_compaction = true;

        let built = build_turn_request(&mut ctx, 1, "gpt-5", &snapshot, Some(320), None, false)
            .await
            .expect("openai request should build");

        assert_eq!(
            built.request.context_management,
            Some(json!([{
                "type": "compaction",
                "compact_threshold": 512,
            }]))
        );
    }

    #[tokio::test]
    async fn prompt_alignment_detects_stale_runtime_tool_catalog_metadata() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        let turn = capture_turn_request_snapshot(&mut ctx, "noop-model", false);

        let make_snapshot = || {
            SessionToolCatalogSnapshot::new(
                7,
                11,
                turn.planning_active,
                turn.request_user_input_enabled,
                Some(Arc::new(Vec::new())),
                false,
            )
        };

        let misaligned_prompt = format!(
            "Base prompt\n[Runtime Tool Catalog]\n- version: 1\n- epoch: 11\n- available_tools: 0\n- request_user_input_enabled: {}\n",
            turn.request_user_input_enabled
        );
        let misaligned_output = PromptAssemblyOutput {
            system_prompt: misaligned_prompt,
            tool_snapshot: make_snapshot(),
        };

        let aligned_snapshot = make_snapshot();
        let mut aligned_prompt = "Base prompt".to_string();
        append_runtime_tool_prompt_sections(&mut aligned_prompt, &aligned_snapshot, true);
        let aligned_output = PromptAssemblyOutput {
            system_prompt: aligned_prompt,
            tool_snapshot: aligned_snapshot,
        };

        let err = validate_prompt_output_alignment(&misaligned_output, &turn)
            .expect_err("stale runtime metadata should be rejected");
        assert!(err.should_rebuild_runtime_prompt());
        validate_prompt_output_alignment(&aligned_output, &turn)
            .expect("aligned runtime metadata should pass");
    }

    #[test]
    fn stable_prefix_hash_ignores_runtime_only_changes() {
        let first = "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-22T00:00:00Z\n- retries: 1";
        let second = "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-23T00:00:00Z\n- retries: 4";

        assert_eq!(
            stable_system_prefix_hash(first),
            stable_system_prefix_hash(second)
        );
    }
}
