use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::TokenCounter;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry};
use vtcode_core::ui::tui::{InlineHandle, InlineSession};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionMessage;

use crate::agent::runloop::unified::turn::run_loop::TurnLoopResult;
// Using `tool_output_handler::handle_pipeline_output_from_turn_ctx` adapter where needed

use crate::agent::runloop::mcp_events;
use vtcode_core::config::types::AgentConfig;

#[allow(dead_code)]
pub enum LlmHandleOutcome {
    Success,
    Failure,
    Cancelled,
}

#[allow(dead_code)]
pub enum TurnResultKind {
    Completed,
    Cancelled,
    Failed,
}

// Note: the module references are kept similar to original file; compiler will resolve them.

pub struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub working_history: Vec<uni::Message>,
    pub turn_modified_files: BTreeSet<PathBuf>,
}

// Apply Turn Outcome: Modify the canonical conversation history and session state for the outcome
pub struct TurnOutcomeContext<'a> {
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub default_placeholder: &'a Option<String>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub next_checkpoint_turn: &'a mut usize,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
}

// Apply Turn Outcome: Modify the canonical conversation history and session state for the outcome
pub async fn apply_turn_outcome(
    outcome: &TurnLoopOutcome,
    ctx: TurnOutcomeContext<'_>,
) -> Result<()> {
    match outcome.result {
        TurnLoopResult::Cancelled => {
            if ctx.ctrl_c_state.is_exit_requested() {
                *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(());
            }
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.ctrl_c_state.clear_cancel();
            *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(())
        }
        TurnLoopResult::Aborted => {
            if let Some(last) = ctx.conversation_history.last() {
                match last.role {
                    uni::MessageRole::Assistant | uni::MessageRole::Tool => {
                        let _ = ctx.conversation_history.pop();
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        TurnLoopResult::Blocked { reason: _ } => {
            *ctx.conversation_history = outcome.working_history.clone();
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            Ok(())
        }
        TurnLoopResult::Completed => {
            *ctx.conversation_history = outcome.working_history.clone();
            if let Some(manager) = ctx.checkpoint_manager {
                let conversation_snapshot: Vec<SessionMessage> = outcome
                    .working_history
                    .iter()
                    .map(SessionMessage::from)
                    .collect();
                let turn_number = *ctx.next_checkpoint_turn;
                let description = outcome
                    .working_history
                    .last()
                    .map(|msg| msg.content.as_text())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match manager
                    .create_snapshot(
                        turn_number,
                        description.as_str(),
                        &conversation_snapshot,
                        &outcome.turn_modified_files,
                    )
                    .await
                {
                    Ok(Some(meta)) => {
                        *ctx.next_checkpoint_turn = meta.turn_number.saturating_add(1);
                    }
                    Ok(None) => {}
                    Err(err) => tracing::warn!(
                        "Failed to create checkpoint for turn {}: {}",
                        turn_number,
                        err
                    ),
                }
            }
            Ok(())
        }
    }
}

pub struct TurnLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut InlineSession,
    pub session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub pruning_ledger: &'a Arc<RwLock<PruningDecisionLedger>>,
    pub token_budget: &'a Arc<TokenBudgetManager>,
    pub token_counter: &'a Arc<RwLock<TokenCounter>>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse (HP-3 optimization)
    pub cached_tools: &'a Option<Arc<Vec<uni::ToolDefinition>>>,
    pub ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
    pub ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
}

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_loop(
    _input: &str,
    mut working_history: Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
    config: &AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_client: &mut Box<dyn uni::LLMProvider>,
    traj: &TrajectoryLogger,
    _skip_confirmations: bool,
    full_auto: bool,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref;
    use crate::agent::runloop::unified::tool_routing::ensure_tool_permission;
    use crate::agent::runloop::unified::turn::turn_processing::{
        TurnProcessingResult, execute_llm_request, process_llm_response,
    };
    use vtcode_core::llm::provider as uni;
    use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed;
    let mut turn_modified_files = BTreeSet::new();

    // NOTE: The user input is already in working_history from the caller (session_loop or run_loop)
    // Do NOT add it again here, as it will cause duplicate messages in the conversation

    // Process up to max_tool_loops iterations to handle tool calls
    let max_tool_loops = vt_cfg
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|&value| value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS);

    let mut step_count = 0;

    loop {
        step_count += 1;

        // Check if we've reached the maximum number of tool loops
        if step_count > max_tool_loops {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Reached maximum tool loops ({})", max_tool_loops),
            )?;
            // When hitting max loops, this is still considered a completed turn
            // (the turn ended normally, just reached the loop limit)
            break;
        }

        // Prepare turn processing context
        let local_token_budget = ctx.token_budget.clone();
        let mut turn_processing_ctx =
            crate::agent::runloop::unified::turn::turn_processing::TurnProcessingContext {
                renderer: ctx.renderer,
                handle: ctx.handle,
                session_stats: ctx.session_stats,
                mcp_panel_state: ctx.mcp_panel_state,
                tool_result_cache: ctx.tool_result_cache,
                approval_recorder: ctx.approval_recorder,
                decision_ledger: ctx.decision_ledger,
                pruning_ledger: ctx.pruning_ledger,
                token_budget: &local_token_budget,
                token_counter: ctx.token_counter,
                working_history: &mut working_history,
                tool_registry: ctx.tool_registry,
                tools: ctx.tools,
                cached_tools: ctx.cached_tools,
                ctrl_c_state: ctx.ctrl_c_state,
                ctrl_c_notify: ctx.ctrl_c_notify,
                vt_cfg,
                context_manager: ctx.context_manager,
                last_forced_redraw: ctx.last_forced_redraw,
                input_status_state: ctx.input_status_state,
                full_auto,
            };

        // Execute the LLM request
        let (response, response_streamed) = match execute_llm_request(
            &mut turn_processing_ctx,
            step_count,
            &config.model,
            None, // max_tokens_opt
            None, // parallel_cfg_opt
            provider_client.as_ref(),
        )
        .await
        {
            Ok(val) => val,
            Err(err) => {
                ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
                ctx.renderer
                    .line(MessageStyle::Error, &format!("LLM request failed: {}", err))?;
                working_history.push(uni::Message::assistant(format!("Request failed: {}", err)));
                result = TurnLoopResult::Aborted;
                break;
            }
        };

        // Process the LLM response
        let processing_result =
            process_llm_response(&response, ctx.renderer, working_history.len())?;

        match processing_result {
            TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text,
                reasoning,
            } => {
                if !response_streamed {
                    if !assistant_text.trim().is_empty() {
                        ctx.renderer.line(MessageStyle::Response, &assistant_text)?;
                    }
                    if let Some(reasoning_text) = reasoning.as_ref()
                        && !reasoning_text.trim().is_empty()
                    {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!(" {}", reasoning_text),
                        )?;
                    }
                }
                // Note: reasoning already rendered during streaming; don't fabricate announcements
                // Add assistant message if there's any text content, and attach reasoning if present
                if !assistant_text.trim().is_empty() {
                    let msg = uni::Message::assistant(assistant_text);
                    let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
                        msg.with_reasoning(Some(reasoning_text))
                    } else {
                        msg
                    };
                    working_history.push(msg_with_reasoning);
                } else if let Some(reasoning_text) = reasoning {
                    // If no assistant text but reasoning exists, create assistant message with just reasoning
                    working_history.push(
                        uni::Message::assistant(String::new()).with_reasoning(Some(reasoning_text)),
                    );
                }

                // Handle tool calls if any exist
                for tool_call in &tool_calls {
                    let function = tool_call
                        .function
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
                    let tool_name = &function.name;
                    let args_val = tool_call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));

                    // Ensure tool permission
                    match ensure_tool_permission(
                        ctx.tool_registry,
                        tool_name,
                        Some(&args_val),
                        ctx.renderer,
                        ctx.handle,
                        ctx.session,
                        ctx.default_placeholder.clone(),
                        ctx.ctrl_c_state,
                        ctx.ctrl_c_notify,
                        ctx.lifecycle_hooks,
                        None, // justification
                        Some(ctx.approval_recorder.as_ref()),
                        Some(ctx.decision_ledger),
                        Some(ctx.tool_permission_cache),
                        vt_cfg.map(|cfg| cfg.security.hitl_notification_bell).unwrap_or(true),
                    ).await {
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Approved) => {
                            // Create progress reporter and spinner for the tool execution
                            let progress_reporter = ProgressReporter::new();
                            let _spinner = PlaceholderSpinner::with_progress(
                                ctx.handle,
                                ctx.input_status_state.left.clone(),
                                ctx.input_status_state.right.clone(),
                                format!("Executing {}...", tool_name),
                                Some(&progress_reporter),
                            );

                            // Set up streaming callback for PTY tools (like run_pty_cmd)
                            let progress_reporter_clone = progress_reporter.clone();
                            ctx.tool_registry.set_progress_callback(Arc::new(move |_name, output| {
                                let reporter = progress_reporter_clone.clone();
                                let output_owned = output.to_string();
                                tokio::spawn(async move {
                                    if let Some(last_line) = output_owned.lines().last() {
                                        let clean_line = vtcode_core::utils::ansi_parser::strip_ansi(last_line);
                                        let trimmed = clean_line.trim();
                                        if !trimmed.is_empty() {
                                            // Show the last line of output in the status line
                                            reporter.set_message(trimmed.to_string()).await;
                                        }
                                    }
                                });
                            }));

                            // Execute the tool
                            let tool_result = execute_tool_with_timeout_ref(
                                ctx.tool_registry,
                                tool_name,
                                &args_val,
                                ctx.ctrl_c_state,
                                ctx.ctrl_c_notify,
                                Some(&progress_reporter),
                            ).await;

                            // Clear the callback after execution
                            ctx.tool_registry.clear_progress_callback();

                            // Handle tool execution result using shared logic
                            crate::agent::runloop::unified::turn::tool_handling::handle_tool_execution_result(
                                &mut ctx,
                                tool_call.id.clone(),
                                tool_name,
                                &args_val,
                                &tool_result,
                                &mut working_history,
                                &mut turn_modified_files,
                                vt_cfg,
                                &local_token_budget,
                                traj,
                            ).await?;
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Denied) => {
                            // Tool permission denied - add denial result to history
                            let denial = ToolExecutionError::new(
                                tool_name.clone(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", tool_name),
                            ).to_json_value();

                            working_history.push(uni::Message::tool_response_with_origin(
                                tool_call.id.clone(),
                                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                                tool_name.clone(),
                            ));
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Exit) => {
                            *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                            result = TurnLoopResult::Cancelled;
                            break; // Exit the loop instead of early return
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Interrupted) => {
                            result = TurnLoopResult::Cancelled;
                            break; // Exit the loop instead of early return
                        }
                        Err(err) => {
                            // Error evaluating policy
                            let err_json = serde_json::json!({
                                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
                            });
                            working_history.push(uni::Message::tool_response_with_origin(
                                tool_call.id.clone(),
                                err_json.to_string(),
                                tool_name.clone(),
                            ));
                        }
                    }
                }
            }
            TurnProcessingResult::TextResponse { text, reasoning } => {
                if !response_streamed {
                    if !text.trim().is_empty() {
                        ctx.renderer.line(MessageStyle::Response, &text)?;
                    }
                    if let Some(reasoning_text) = reasoning.as_ref()
                        && !reasoning_text.trim().is_empty()
                    {
                        ctx.renderer.line(
                            MessageStyle::Info,
                            &format!(" {}", reasoning_text),
                        )?;
                    }
                }
                // Note: reasoning already rendered during streaming; don't fabricate announcements
                // Check if the text response contains textual tool calls to execute
                if let Some((tool_name, args)) =
                    crate::agent::runloop::text_tools::detect_textual_tool_call(&text)
                {
                    let args_json = serde_json::json!(&args);

                    // Create a tool call from the detected textual command
                    let tool_call_str = format!("call_textual_{}", working_history.len());
                    let tool_call = uni::ToolCall::function(
                        tool_call_str,
                        tool_name.clone(),
                        serde_json::to_string(&args_json).unwrap_or_else(|_| "{}".to_string()),
                    );

                    // Process the detected tool call
                    let function = tool_call
                        .function
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
                    let call_tool_name = &function.name;
                    let call_args_val = tool_call
                        .parsed_arguments()
                        .unwrap_or_else(|_| serde_json::json!({}));

                    // Render information about the detected tool call
                    use crate::agent::runloop::unified::tool_summary::{
                        describe_tool_action, humanize_tool_name,
                    };
                    let (headline, _) = describe_tool_action(call_tool_name, &call_args_val);
                    let notice = if headline.is_empty() {
                        format!("Detected {} request", humanize_tool_name(call_tool_name))
                    } else {
                        format!("Detected {headline}")
                    };
                    ctx.renderer.line(MessageStyle::Info, &notice)?;

                    // Ensure tool permission
                    match crate::agent::runloop::unified::tool_routing::ensure_tool_permission(
                        ctx.tool_registry,
                        call_tool_name,
                        Some(&call_args_val),
                        ctx.renderer,
                        ctx.handle,
                        ctx.session,
                        ctx.default_placeholder.clone(),
                        ctx.ctrl_c_state,
                        ctx.ctrl_c_notify,
                        ctx.lifecycle_hooks,
                        None, // justification
                        Some(ctx.approval_recorder.as_ref()),
                        Some(ctx.decision_ledger),
                        Some(ctx.tool_permission_cache),
                        vt_cfg.map(|cfg| cfg.security.hitl_notification_bell).unwrap_or(true),
                    ).await {
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Approved) => {
                            // Execute the detected tool
                            let tool_result = crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref(
                                ctx.tool_registry,
                                call_tool_name,
                                &call_args_val,
                                ctx.ctrl_c_state,
                                ctx.ctrl_c_notify,
                                None, // progress_reporter
                            ).await;

                            // Handle tool execution result using shared logic
                            crate::agent::runloop::unified::turn::tool_handling::handle_tool_execution_result(
                                &mut ctx,
                                tool_call.id.clone(),
                                call_tool_name,
                                &call_args_val,
                                &tool_result,
                                &mut working_history,
                                &mut turn_modified_files,
                                vt_cfg,
                                &local_token_budget,
                                traj,
                            ).await?;
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Denied) => {
                            // Tool permission denied - add denial result to history
                            let denial = vtcode_core::tools::registry::ToolExecutionError::new(
                                call_tool_name.clone(),
                                vtcode_core::tools::registry::ToolErrorType::PolicyViolation,
                                format!("Detected tool '{}' execution denied by policy", call_tool_name),
                            ).to_json_value();

                            working_history.push(uni::Message::tool_response_with_origin(
                                tool_call.id.clone(),
                                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                                call_tool_name.clone(),
                            ));
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Exit) => {
                            *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                            result = TurnLoopResult::Cancelled;
                            break; // Exit the loop instead of early return
                        }
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Interrupted) => {
                            result = TurnLoopResult::Cancelled;
                            break; // Exit the loop instead of early return
                        }
                        Err(err) => {
                            // Error evaluating policy
                            let err_json = serde_json::json!({
                                "error": format!("Failed to evaluate policy for detected tool '{}': {}", call_tool_name, err)
                            });
                            working_history.push(uni::Message::tool_response_with_origin(
                                tool_call.id.clone(),
                                err_json.to_string(),
                                call_tool_name.clone(),
                            ));
                        }
                    }
                } else {
                    // If no tool call was detected in the text, check if it's just thinking/planning

                    let msg = uni::Message::assistant(text.clone());
                    let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
                        msg.with_reasoning(Some(reasoning_text))
                    } else {
                        msg
                    };

                    if !text.is_empty() || msg_with_reasoning.reasoning.is_some() {
                        working_history.push(msg_with_reasoning);
                    }

                    break; // If we get a real text response that's not a tool call, the turn is done
                }
            }
            TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
                // If there's no actionable content, we can break the loop
                break;
            }
            TurnProcessingResult::Cancelled => {
                *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
                result = TurnLoopResult::Cancelled;
                break; // Exit the loop instead of early return
            }
            TurnProcessingResult::Aborted => {
                result = TurnLoopResult::Aborted;
                break; // Exit the loop instead of early return
            }
        }

        // Check for Ctrl+C interruption
        if ctx.ctrl_c_state.is_exit_requested() {
            *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
            result = TurnLoopResult::Cancelled;
            break; // Exit the loop instead of early return
        }
    }

    // Final outcome with the correct result status
    Ok(TurnLoopOutcome {
        result,
        working_history,
        turn_modified_files,
    })
}
