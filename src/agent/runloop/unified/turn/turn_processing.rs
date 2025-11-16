use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tokio::task;

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::ui_interaction::{
    PlaceholderSpinner, stream_and_render_response,
};
#[cfg(debug_assertions)]
use tracing::debug;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::llm::TokenCounter;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::result_cache::ToolResultCache;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::AnsiRenderer;

/// Context for turn processing operations
pub(crate) struct TurnProcessingContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger: &'a Arc<tokio::sync::RwLock<DecisionTracker>>,
    pub pruning_ledger: &'a Arc<tokio::sync::RwLock<PruningDecisionLedger>>,
    pub token_budget: &'a Arc<TokenBudgetManager>,
    pub token_counter: &'a Arc<tokio::sync::RwLock<TokenCounter>>,
    pub working_history: &'a mut Vec<uni::Message>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
}

/// Execute an LLM request and return the response
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<ParallelToolConfig>,
    provider_client: &dyn uni::LLMProvider,
) -> Result<(uni::LLMResponse, bool)> {
    // Apply semantic pruning with decision tracking if configured
    if ctx.context_manager.trim_config().semantic_compression {
        let mut pruning_ledger_mut = ctx.pruning_ledger.write().await;
        ctx.context_manager.prune_with_semantic_priority(
            ctx.working_history,
            Some(&mut *pruning_ledger_mut),
            step_count,
        );
    }

    let request_history = ctx.working_history.clone();
    ctx.context_manager.reset_token_budget().await;
    let system_prompt = ctx
        .context_manager
        .build_system_prompt(&request_history, step_count)
        .await?;

    let use_streaming = provider_client.supports_streaming();
    let reasoning_effort = ctx.vt_cfg.as_ref().and_then(|cfg| {
        if provider_client.supports_reasoning_effort(active_model) {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });

    let current_tools = ctx.tools.read().await.clone();
    let request = uni::LLMRequest {
        messages: request_history.clone(),
        system_prompt: Some(system_prompt),
        tools: Some(current_tools),
        model: active_model.to_string(),
        max_tokens: max_tokens_opt.or(Some(2000)),
        temperature: Some(0.7),
        stream: use_streaming,
        tool_choice: Some(uni::ToolChoice::auto()),
        parallel_tool_calls: None,
        parallel_tool_config: if provider_client.supports_parallel_tool_config(active_model) {
            parallel_cfg_opt.clone()
        } else {
            None
        },
        reasoning_effort,
        output_format: None,
        verbosity: None,
    };

    let thinking_spinner = PlaceholderSpinner::new(
        ctx.handle,
        ctx.input_status_state.left.clone(),
        ctx.input_status_state.right.clone(),
        "Thinking...",
    );
    task::yield_now().await;

    #[cfg(debug_assertions)]
    let request_timer = Instant::now();
    #[cfg(debug_assertions)]
    {
        let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
        debug!(
            target = "vtcode::agent::llm",
            model = %request.model,
            streaming = use_streaming,
            step = step_count,
            messages = request.messages.len(),
            tools = tool_count,
            "Dispatching provider request"
        );
    }

    let llm_result = if use_streaming {
        stream_and_render_response(
            provider_client,
            request,
            &thinking_spinner,
            ctx.renderer,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
        )
        .await
    } else {
        let provider_name = provider_client.name().to_string();

        if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
            thinking_spinner.finish();
            Err(uni::LLMError::Provider(
                vtcode_core::llm::error_display::format_llm_error(
                    &provider_name,
                    "Interrupted by user",
                ),
            ))
        } else {
            let generate_future = provider_client.generate(request);
            tokio::pin!(generate_future);
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);
            let outcome = tokio::select! {
                res = &mut generate_future => {
                    thinking_spinner.finish();
                    res.map(|resp| (resp, false))
                }
                _ = &mut cancel_notifier => {
                    thinking_spinner.finish();
                    Err(uni::LLMError::Provider(vtcode_core::llm::error_display::format_llm_error(
                        &provider_name,
                        "Interrupted by user",
                    )))
                }
            };
            outcome
        }
    };

    #[cfg(debug_assertions)]
    {
        debug!(
            target = "vtcode::agent::llm",
            model = %active_model,
            streaming = use_streaming,
            step = step_count,
            elapsed_ms = request_timer.elapsed().as_millis(),
            succeeded = llm_result.is_ok(),
            "Provider request finished"
        );
    }

    let (response, response_streamed) = llm_result?;
    *ctx.working_history = request_history;
    Ok((response, response_streamed))
}

/// Derive recent tool output from conversation history
fn derive_recent_tool_output(working_history: &[uni::Message]) -> Option<String> {
    for message in working_history.iter().rev() {
        if let uni::MessageRole::Tool = message.role {
            let content = message.content.as_text();
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }
    None
}

/// Strip harmony syntax from text
fn strip_harmony_syntax(text: &str) -> String {
    // Remove harmony tool call syntax from the displayed text
    text.lines()
        .filter(|line| {
            !line.contains("<|start|>")
                && !line.contains("<|channel|>")
                && !line.contains("<|call|>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of processing a single turn
pub(crate) enum TurnProcessingResult {
    /// Turn resulted in tool calls that need to be executed
    ToolCalls {
        tool_calls: Vec<uni::ToolCall>,
        assistant_text: String,
        reasoning: Option<String>,
    },
    /// Turn resulted in a text response
    TextResponse {
        text: String,
        reasoning: Option<String>,
    },
    /// Turn resulted in no actionable output
    Empty,
    /// Turn was completed successfully
    Completed,
    /// Turn was cancelled by user
    Cancelled,
    /// Turn was aborted due to error
    Aborted,
}
