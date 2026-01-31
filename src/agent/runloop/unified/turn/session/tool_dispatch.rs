use anyhow::Result;
use std::collections::{BTreeSet, HashMap};

use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use crate::agent::runloop::unified::turn::session::interaction_loop::{
    InteractionLoopContext, InteractionOutcome,
};
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::{
    handle_single_tool_call, ToolOutcomeContext,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;

pub(crate) struct DirectToolContext<'a, 'b> {
    pub interaction_ctx: &'b mut InteractionLoopContext<'a>,
    pub input_status_state: &'b mut InputStatusState,
}

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    // Check for bash mode with '!' prefix or explicit 'run' command
    let (tool_name_str, args, input_str) = if input.starts_with('!') {
        let bash_command = input.trim_start_matches('!').trim();
        if bash_command.is_empty() {
            return Ok(None);
        }
        (
            "bash".to_string(),
            serde_json::json!({"command": bash_command}),
            input,
        )
    } else if let Some((name, tool_args)) =
        crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
    {
        (name.to_string(), tool_args, input)
    } else {
        return Ok(None);
    };

    // Construct HarnessTurnState (simplified for direct execution)
    let mut harness_state = HarnessTurnState::new(
        TurnRunId(SessionId::new().0),
        TurnId(SessionId::new().0),
        100, // max tool calls
        300, // max duration
        3,   // max retries
    );

    // Construct TurnProcessingContext to leverage unified execution handlers
    let mut tp_ctx = TurnProcessingContext {
        renderer: ctx.interaction_ctx.renderer,
        handle: ctx.interaction_ctx.handle,
        session_stats: ctx.interaction_ctx.session_stats,
        auto_exit_plan_mode_attempted: &mut false,
        mcp_panel_state: ctx.interaction_ctx.mcp_panel_state,
        tool_result_cache: ctx.interaction_ctx.tool_result_cache,
        approval_recorder: ctx.interaction_ctx.approval_recorder,
        decision_ledger: ctx.interaction_ctx.decision_ledger,
        working_history: ctx.interaction_ctx.conversation_history,
        tool_registry: ctx.interaction_ctx.tool_registry,
        tools: ctx.interaction_ctx.tools,
        cached_tools: ctx.interaction_ctx.cached_tools,
        ctrl_c_state: ctx.interaction_ctx.ctrl_c_state,
        ctrl_c_notify: ctx.interaction_ctx.ctrl_c_notify,
        vt_cfg: ctx.interaction_ctx.vt_cfg.as_ref(),
        context_manager: ctx.interaction_ctx.context_manager,
        last_forced_redraw: ctx.interaction_ctx.last_forced_redraw,
        input_status_state: ctx.input_status_state,
        session: ctx.interaction_ctx.session,
        lifecycle_hooks: ctx.interaction_ctx.lifecycle_hooks,
        default_placeholder: ctx.interaction_ctx.default_placeholder,
        tool_permission_cache: ctx.interaction_ctx.tool_permission_cache,
        safety_validator: ctx.interaction_ctx.safety_validator,
        provider_client: ctx.interaction_ctx.provider_client,
        full_auto: ctx.interaction_ctx.full_auto,
        circuit_breaker: ctx.interaction_ctx.circuit_breaker,
        tool_health_tracker: ctx.interaction_ctx.tool_health_tracker,
        rate_limiter: ctx.interaction_ctx.rate_limiter,
        telemetry: ctx.interaction_ctx.telemetry,
        autonomous_executor: ctx.interaction_ctx.autonomous_executor,
        error_recovery: ctx.interaction_ctx.error_recovery,
        harness_state: &mut harness_state,
        harness_emitter: ctx.interaction_ctx.harness_emitter,
    };

    let mut repeated_tool_attempts = HashMap::new();
    let mut turn_modified_files = BTreeSet::new();

    let mut t_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        traj: ctx.interaction_ctx.traj,
    };

    // 1. Display user message and push to history
    display_user_message(t_ctx.ctx.renderer, input_str)?;
    t_ctx
        .ctx
        .working_history
        .push(uni::Message::user(input_str.to_string()));

    // 2. Inject assistant message with tool call to keep history valid for LLM
    let tool_call_id = format!(
        "direct_{}_{}",
        tool_name_str,
        t_ctx.ctx.working_history.len()
    );
    let tool_call = uni::ToolCall::function(
        tool_call_id.clone(),
        tool_name_str.clone(),
        serde_json::to_string(&args).unwrap_or_default(),
    );
    t_ctx
        .ctx
        .working_history
        .push(uni::Message::assistant_with_tools(
            String::new(),
            vec![tool_call],
        ));

    // 3. Execute through unified pipeline to ensure safety, metrics, and consistent output
    let outcome = handle_single_tool_call(&mut t_ctx, tool_call_id, &tool_name_str, args).await?;

    // 4. Cleanup UI and return outcome
    t_ctx.ctx.handle.clear_input();
    if let Some(placeholder) = t_ctx.ctx.default_placeholder {
        t_ctx.ctx.handle.set_placeholder(Some(placeholder.to_string()));
    }

    if let Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)) = outcome {
        return Ok(Some(InteractionOutcome::Exit {
            reason: crate::hooks::lifecycle::SessionEndReason::Exit,
        }));
    }

    let follow_up = format!(
        "I ran `{}`. How would you like to proceed?",
        tool_name_str
    );
    Ok(Some(InteractionOutcome::Continue { input: follow_up }))
}
