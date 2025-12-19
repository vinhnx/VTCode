use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tokio::task;

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::extract_action_from_messages;
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
use vtcode_core::utils::ansi::MessageStyle;

/// Context for turn processing operations
#[allow(dead_code)]
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
    /// Cached tool definitions for efficient reuse (HP-3 optimization)
    pub cached_tools: &'a Option<Arc<Vec<uni::ToolDefinition>>>,
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

    // HP-9: Lazy context window enforcement - only when needed
    if ctx.context_manager.should_enforce_context(ctx.working_history) {
        let _ = ctx
            .context_manager
            .enforce_context_window(ctx.working_history);
    }
    // HP-1: Eliminate unnecessary clone - work directly on working_history
    ctx.context_manager.reset_token_budget().await;
    let system_prompt = ctx
        .context_manager
        .build_system_prompt(ctx.working_history, step_count)
        .await?;

    let use_streaming = provider_client.supports_streaming();
    let reasoning_effort = ctx.vt_cfg.as_ref().and_then(|cfg| {
        if provider_client.supports_reasoning_effort(active_model) {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });

    // HP-3: Use cached tools instead of acquiring lock and cloning
    let current_tools = ctx.cached_tools.as_ref().map(|arc| (**arc).clone());
    let has_tools = current_tools.is_some();
    let parallel_config =
        if has_tools && provider_client.supports_parallel_tool_config(active_model) {
            parallel_cfg_opt.clone()
        } else {
            None
        };
    let tool_choice = if has_tools {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };
    let request = uni::LLMRequest {
        // HP-1: Single clone only when building LLMRequest
        messages: ctx.working_history.iter().cloned().collect(),
        system_prompt: Some(system_prompt),
        tools: current_tools,
        model: active_model.to_string(),
        max_tokens: max_tokens_opt.or(Some(2000)),
        temperature: Some(0.7),
        stream: use_streaming,
        tool_choice,
        parallel_tool_calls: None,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        output_format: None,
        verbosity: None,
    };

    let action_suggestion = extract_action_from_messages(ctx.working_history);
    let thinking_spinner = PlaceholderSpinner::new(
        ctx.handle,
        ctx.input_status_state.left.clone(),
        ctx.input_status_state.right.clone(),
        action_suggestion,
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

    if let Err(err) = provider_client.validate_request(&request) {
        thinking_spinner.finish();
        return Err(anyhow::Error::new(err));
    }

    let mut llm_result = if use_streaming {
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
            Err(uni::LLMError::Provider {
                message: vtcode_core::llm::error_display::format_llm_error(
                    &provider_name,
                    "Interrupted by user",
                ),
                metadata: None,
            })
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
                    Err(uni::LLMError::Provider {
                        message: vtcode_core::llm::error_display::format_llm_error(
                            &provider_name,
                            "Interrupted by user",
                        ),
                        metadata: None,
                    })
                }
            };
            outcome
        }
    };

    // Prevent agent from giving up with "Complex. Probably stop." or similar
    if let Ok((response, _)) = &mut llm_result
        && let Some(reasoning) = &response.reasoning
            && is_giving_up_reasoning(reasoning) {
                #[cfg(debug_assertions)]
                eprintln!(
                    "Detected giving-up reasoning '{}', replacing with constructive reasoning",
                    reasoning
                );

                // Log the original reasoning for debugging
                tracing::warn!(
                    target = "vtcode::agent::reasoning",
                    original_reasoning = %reasoning,
                    "Agent attempted to give up, replacing with constructive reasoning"
                );

                // Replace with constructive reasoning that encourages continuation
                response.reasoning = Some(get_constructive_reasoning(reasoning));
            }

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

    let (response, response_streamed) = match llm_result {
        Ok(result) => result,
        Err(error) => {
            // Finish spinner before returning error to remove it from transcript
            thinking_spinner.finish();
            return Err(anyhow::Error::new(error));
        }
    };
    // HP-1: No restoration needed - working_history was never modified
    Ok((response, response_streamed))
}

// Use `strip_harmony_syntax` and `derive_recent_tool_output` helpers from other modules

/// Check if reasoning contains giving-up language
fn is_giving_up_reasoning(reasoning: &str) -> bool {
    let lower = reasoning.to_lowercase();
    // Check for patterns indicating the agent wants to give up
    lower.contains("complex") && lower.contains("stop")
        || lower.contains("probably stop")
        || lower.contains("give up")
        || lower.contains("can't continue")
        || lower.contains("unable to continue")
        || lower.contains("too complex") && lower.contains("stop")
}

/// Replace giving-up reasoning with constructive reasoning
fn get_constructive_reasoning(original: &str) -> String {
    // Analyze what the agent was trying to do
    let lower = original.to_lowercase();

    if lower.contains("pdf") || lower.contains("file") || lower.contains("path") {
        "Analyzing file system issue and exploring alternative approaches to generate the PDF successfully.".to_string()
    } else if lower.contains("tool") || lower.contains("execute") || lower.contains("code") {
        "Encountered tool execution challenges, switching to alternative strategies and verifying environment setup.".to_string()
    } else if lower.contains("permission") || lower.contains("access") {
        "Addressing permission/access issues and finding workable solutions within constraints."
            .to_string()
    } else {
        "Encountered complexity but continuing with systematic problem-solving approach."
            .to_string()
    }
}

/// Result of processing a single turn
#[allow(dead_code)]
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

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
) -> Result<TurnProcessingResult> {
    use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
    use vtcode_core::llm::provider as uni;

    let mut final_text = response.content.clone();
    let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
    let mut interpreted_textual_call = false;

    // Strip harmony syntax from displayed content if present
    if let Some(ref text) = final_text
        && (text.contains("<|start|>") || text.contains("<|channel|>") || text.contains("<|call|>"))
    {
        let cleaned = strip_harmony_syntax(text);
        if !cleaned.trim().is_empty() {
            final_text = Some(cleaned);
        } else {
            final_text = None;
        }
    }

    if tool_calls.is_empty()
        && let Some(text) = final_text.clone()
        && !text.trim().is_empty()
        && let Some((name, args)) =
            crate::agent::runloop::text_tools::detect_textual_tool_call(&text)
    {
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
        let code_blocks = crate::agent::runloop::text_tools::extract_code_fence_blocks(&text);
        if !code_blocks.is_empty() {
            crate::agent::runloop::tool_output::render_code_fence_blocks(renderer, &code_blocks)?;
            renderer.line(MessageStyle::Output, "")?;
        }
        let (headline, _) =
            crate::agent::runloop::unified::tool_summary::describe_tool_action(&name, &args);
        let notice = if headline.is_empty() {
            format!(
                "Detected {} request",
                crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name)
            )
        } else {
            format!("Detected {headline}")
        };
        renderer.line(MessageStyle::Info, &notice)?;
        let call_id = format!("call_textual_{}", conversation_len);
        tool_calls.push(uni::ToolCall::function(
            call_id.clone(),
            name.clone(),
            args_json.clone(),
        ));
        interpreted_textual_call = true;
        final_text = None;
    }

    // Build result
    if !tool_calls.is_empty() {
        let assistant_text = if interpreted_textual_call {
            String::new()
        } else {
            final_text.clone().unwrap_or_default()
        };
        return Ok(TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning: response.reasoning.clone(),
        });
    }

    if let Some(text) = final_text
        && !text.trim().is_empty()
    {
        return Ok(TurnProcessingResult::TextResponse {
            text,
            reasoning: response.reasoning.clone(),
        });
    }

    Ok(TurnProcessingResult::Empty)
}
