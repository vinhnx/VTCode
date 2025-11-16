use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
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

pub enum LlmHandleOutcome {
    Success,
    Failure,
    Cancelled,
}

pub enum TurnResultKind {
    Completed,
    Cancelled,
    Failed,
}

// Note: the module references are kept similar to original file; compiler will resolve them.

pub struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub working_history: Vec<uni::Message>,
    pub any_write_effect: bool,
    pub turn_modified_files: BTreeSet<PathBuf>,
}

// Apply Turn Outcome: Modify the canonical conversation history and session state for the outcome
pub async fn apply_turn_outcome(
    outcome: &TurnLoopOutcome,
    conversation_history: &mut Vec<uni::Message>,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    ctrl_c_state: &Arc<CtrlCState>,
    default_placeholder: &Option<String>,
    checkpoint_manager: Option<&SnapshotManager>,
    next_checkpoint_turn: &mut usize,
    _session_stats: &mut SessionStats,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
    _pruning_ledger: &Arc<RwLock<PruningDecisionLedger>>,
) -> Result<()> {
    match outcome.result {
        TurnLoopResult::Cancelled => {
            if ctrl_c_state.is_exit_requested() {
                *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(());
            }
            renderer.line_if_not_empty(MessageStyle::Output)?;
            renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            handle.clear_input();
            handle.set_placeholder(default_placeholder.clone());
            ctrl_c_state.clear_cancel();
            *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(())
        }
        TurnLoopResult::Aborted => {
            if let Some(last) = conversation_history.last() {
                match last.role {
                    uni::MessageRole::Assistant | uni::MessageRole::Tool => {
                        let _ = conversation_history.pop();
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        TurnLoopResult::Blocked { reason: _ } => {
            *conversation_history = outcome.working_history.clone();
            handle.clear_input();
            handle.set_placeholder(default_placeholder.clone());
            Ok(())
        }
        TurnLoopResult::Completed => {
            *conversation_history = outcome.working_history.clone();
            if let Some(manager) = checkpoint_manager {
                let conversation_snapshot: Vec<SessionMessage> = outcome
                    .working_history
                    .iter()
                    .map(SessionMessage::from)
                    .collect();
                let turn_number = *next_checkpoint_turn;
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
                        *next_checkpoint_turn = meta.turn_number.saturating_add(1);
                    }
                    Ok(None) => {}
                    Err(err) => tracing::warn!(
                        "Failed to create checkpoint for turn {}: {}",
                        turn_number,
                        err
                    ),
                }
            }
            if let Some(last) = outcome.working_history.last() {
                if last.role == uni::MessageRole::Assistant {
                    let text = last.content.as_text();
                    let claims_write = text.contains("I've updated")
                        || text.contains("I have updated")
                        || text.contains("updated the `");
                    if claims_write && !outcome.any_write_effect {
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                        renderer.line(
                            MessageStyle::Info,
                            "Note: The assistant mentioned edits but no write tool ran.",
                        )?;
                    }
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
    input: &str,
    mut working_history: Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
    config: &AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_client: &mut Box<dyn uni::LLMProvider>,
    _traj: &TrajectoryLogger,
    _skip_confirmations: bool,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::mcp_events;
    use crate::agent::runloop::unified::tool_pipeline::{
        ToolExecutionStatus, execute_tool_with_timeout,
    };
    use crate::agent::runloop::unified::tool_routing::ensure_tool_permission;
    use crate::agent::runloop::unified::turn::turn_processing::{
        TurnProcessingResult, execute_llm_request, process_llm_response,
    };
    use crate::agent::runloop::unified::turn::utils::render_hook_messages;
    use vtcode_core::llm::provider as uni;
    use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed;
    let mut any_write_effect = false;
    let mut turn_modified_files = BTreeSet::new();

    // Add the user input to the working history
    working_history.push(uni::Message::user(input.to_string()));

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
                ctrl_c_state: ctx.ctrl_c_state,
                ctrl_c_notify: ctx.ctrl_c_notify,
                vt_cfg,
                context_manager: ctx.context_manager,
                last_forced_redraw: ctx.last_forced_redraw,
                input_status_state: ctx.input_status_state,
            };

        // Execute the LLM request
        let (response, _response_streamed) = execute_llm_request(
            &mut turn_processing_ctx,
            step_count,
            &config.model,
            None, // max_tokens_opt
            None, // parallel_cfg_opt
            provider_client.as_ref(),
        )
        .await?;

        // Process the LLM response
        let processing_result =
            process_llm_response(&response, ctx.renderer, working_history.len())?;

        match processing_result {
            TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text,
                reasoning: _,
            } => {
                // Add assistant message if there's any text content
                if !assistant_text.trim().is_empty() {
                    working_history.push(uni::Message::assistant(assistant_text));
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
                    ).await {
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Approved) => {
                            // Execute the tool
                            let tool_result = execute_tool_with_timeout(
                                ctx.tool_registry,
                                tool_name,
                                args_val.clone(),
                                ctx.ctrl_c_state,
                                ctx.ctrl_c_notify,
                                None, // progress_reporter
                            ).await;

                            match &tool_result {
                                ToolExecutionStatus::Success { output, stdout: _, modified_files, command_success, has_more } => {
                                    // Add successful tool result to history
                                    let content = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        content.clone(), // Keep a copy for rendering
                                        tool_name.clone(),
                                    ));

                                    // Build a ToolPipelineOutcome to leverage centralized handling
                                    let pipeline_outcome = crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(
                                        crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success {
                                            output: output.clone(),
                                            stdout: None,
                                            modified_files: modified_files.clone(),
                                            command_success: *command_success,
                                            has_more: *has_more,
                                        }
                                    );
                                    // Build a small RunLoopContext to reuse the generic `handle_pipeline_output`
                                    let (any_write, mod_files, last_stdout) = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx(
                                        &mut ctx,
                                        tool_name,
                                        &args_val,
                                        &pipeline_outcome,
                                        vt_cfg,
                                        &*local_token_budget,
                                        _traj,
                                    )
                                    .await?;
                                    if any_write { any_write_effect = true; }
                                    for f in mod_files { turn_modified_files.insert(f); }
                                    let _ = last_stdout;
                                        if let Some(hooks) = ctx.lifecycle_hooks {
                                            match hooks.run_post_tool_use(tool_name, Some(&args_val), output).await {
                                            Ok(outcome) => {
                                                render_hook_messages(ctx.renderer, &outcome.messages)?;
                                                for context in outcome.additional_context {
                                                    if !context.trim().is_empty() {
                                                        working_history.push(uni::Message::system(context));
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                ctx.renderer.line(
                                                    MessageStyle::Error,
                                                    &format!("Failed to run post-tool hooks: {}", err),
                                                )?;
                                            }
                                        }
                                    }

                                    if matches!(tool_name.as_str(), "write_file" | "edit_file" | "create_file" | "delete_file") {
                                        any_write_effect = true;
                                    }

                                    // Check if we should short-circuit for shell commands
                                    if !has_more && *command_success {
                                        use crate::agent::runloop::unified::shell::{should_short_circuit_shell, derive_recent_tool_output};
                                        if should_short_circuit_shell(input, tool_name, output) {
                                            let reply = derive_recent_tool_output(&working_history)
                                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                                            ctx.renderer.line(MessageStyle::Response, &reply)?;
                                            working_history.push(uni::Message::assistant(reply));
                                            break;
                                        }
                                    }
                                }
                                ToolExecutionStatus::Failure { error, .. } => {
                                    // Add error result to history
                                    let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
                                    ctx.renderer.line(MessageStyle::Error, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        tool_name.clone(),
                                    ));
                                }
                                ToolExecutionStatus::Timeout { error, .. } => {
                                    // Add timeout result to history
                                    let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
                                    ctx.renderer.line(MessageStyle::Error, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        tool_name.clone(),
                                    ));
                                }
                                ToolExecutionStatus::Cancelled => {
                                    // Add cancellation result to history
                                    let error_msg = format!("Tool '{}' execution cancelled", tool_name);
                                    ctx.renderer.line(MessageStyle::Info, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        tool_name.clone(),
                                    ));
                                }
                                ToolExecutionStatus::Progress(_) => {
                                    // Progress events are handled internally by the tool execution system
                                    // Just continue without adding to the conversation history
                                    continue;
                                }
                            }

                            // Handle MCP events
                            if tool_name.starts_with("mcp_") {
                                match &tool_result {
                                    ToolExecutionStatus::Success { output, .. } => {
                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())),
                                        );
                                        mcp_event.success(None);
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    ToolExecutionStatus::Failure { error, .. } => {
                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(serde_json::json!({"error": error.to_string()}).to_string()),
                                        );
                                        mcp_event.failure(Some(error.to_string()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    ToolExecutionStatus::Timeout { error, .. } => {
                                        let error_str = &error.message;
                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(serde_json::json!({"error": error_str}).to_string()),
                                        );
                                        mcp_event.failure(Some(error_str.clone()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    ToolExecutionStatus::Cancelled => {
                                        let mut mcp_event = mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            tool_name.to_string(),
                                            Some(serde_json::json!({"error": "Cancelled"}).to_string()),
                                        );
                                        mcp_event.failure(Some("Cancelled".to_string()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    ToolExecutionStatus::Progress(_) => {
                                        // Progress events are handled internally, no MCP event needed
                                    }
                                }
                            }
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
            TurnProcessingResult::TextResponse { text, reasoning: _ } => {
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
                    ).await {
                        Ok(crate::agent::runloop::unified::tool_routing::ToolPermissionFlow::Approved) => {
                            // Execute the detected tool
                            let tool_result = crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout(
                                ctx.tool_registry,
                                call_tool_name,
                                call_args_val.clone(),
                                ctx.ctrl_c_state,
                                ctx.ctrl_c_notify,
                                None, // progress_reporter
                            ).await;

                            match &tool_result {
                                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success { output, stdout: _, modified_files, command_success: _, has_more: _ } => {
                                    // Add successful tool result to history
                                    let content = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        content,
                                        call_tool_name.clone(),
                                    ));

                                    let pipeline_outcome = crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(
                                        crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success {
                                            output: output.clone(),
                                            stdout: None,
                                            modified_files: modified_files.clone(),
                                            command_success: true,
                                            has_more: false,
                                        }
                                    );
                                    let (any_write, mod_files, last_stdout) = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx(
                                        &mut ctx,
                                        call_tool_name,
                                        &call_args_val,
                                        &pipeline_outcome,
                                        vt_cfg,
                                        &*local_token_budget,
                                        _traj,
                                    )
                                    .await?;
                                    if any_write { any_write_effect = true; }
                                    for f in mod_files { turn_modified_files.insert(f); }
                                    let _ = last_stdout;

                                    // Handle lifecycle hooks for post-tool use
                                    if let Some(hooks) = ctx.lifecycle_hooks {
                                        match hooks.run_post_tool_use(call_tool_name, Some(&call_args_val), output).await {
                                            Ok(outcome) => {
                                                crate::agent::runloop::unified::turn::utils::render_hook_messages(ctx.renderer, &outcome.messages)?;
                                                for context in outcome.additional_context {
                                                    if !context.trim().is_empty() {
                                                        working_history.push(uni::Message::system(context));
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                ctx.renderer.line(
                                                    MessageStyle::Error,
                                                    &format!("Failed to run post-tool hooks: {}", err),
                                                )?;
                                            }
                                        }
                                    }

                                    if matches!(call_tool_name.as_str(), "write_file" | "edit_file" | "create_file" | "delete_file") {
                                        any_write_effect = true;
                                    }
                                }
                                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Failure { error, .. } => {
                                    // Add error result to history
                                    let error_msg = format!("Detected tool '{}' execution failed: {}", call_tool_name, error);
                                    ctx.renderer.line(MessageStyle::Error, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        call_tool_name.clone(),
                                    ));
                                }
                                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Timeout { error, .. } => {
                                    // Add timeout result to history
                                    let error_msg = format!("Detected tool '{}' timed out: {}", call_tool_name, error.message);
                                    ctx.renderer.line(MessageStyle::Error, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        call_tool_name.clone(),
                                    ));
                                }
                                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Cancelled => {
                                    // Add cancellation result to history
                                    let error_msg = format!("Detected tool '{}' execution cancelled", call_tool_name);
                                    ctx.renderer.line(MessageStyle::Info, &error_msg)?;

                                    let error_content = serde_json::json!({"error": error_msg});
                                    working_history.push(uni::Message::tool_response_with_origin(
                                        tool_call.id.clone(),
                                        error_content.to_string(),
                                        call_tool_name.clone(),
                                    ));
                                }
                                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Progress(_) => {
                                    // Progress events are handled internally by the tool execution system
                                    continue;
                                }
                            }

                            // Handle MCP events for detected tools
                            if call_tool_name.starts_with("mcp_") {
                                match &tool_result {
                                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success { output, .. } => {
                                        let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            call_tool_name.to_string(),
                                            Some(serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())),
                                        );
                                        mcp_event.success(None);
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Failure { error, .. } => {
                                        let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            call_tool_name.to_string(),
                                            Some(serde_json::json!({"error": error.to_string()}).to_string()),
                                        );
                                        mcp_event.failure(Some(error.to_string()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Timeout { error, .. } => {
                                        let error_str = &error.message;
                                        let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            call_tool_name.to_string(),
                                            Some(serde_json::json!({"error": error_str}).to_string()),
                                        );
                                        mcp_event.failure(Some(error_str.clone()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Cancelled => {
                                        let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
                                            "mcp".to_string(),
                                            call_tool_name.to_string(),
                                            Some(serde_json::json!({"error": "Cancelled"}).to_string()),
                                        );
                                        mcp_event.failure(Some("Cancelled".to_string()));
                                        ctx.mcp_panel_state.add_event(mcp_event);
                                    }
                                    crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Progress(_) => {
                                        // Progress events are handled internally, no MCP event needed
                                    }
                                }
                            }
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
                    // If no tool call was detected in the text, add it as a regular assistant response
                    working_history.push(uni::Message::assistant(text));
                    break; // If we get a text response that's not a tool call, the turn is done
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
        any_write_effect,
        turn_modified_files,
    })
}
