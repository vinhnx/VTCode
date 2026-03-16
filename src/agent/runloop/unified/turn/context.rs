use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tokio::sync::RwLock;
use vtcode_commons::diff_paths::{
    is_diff_addition_line, is_diff_deletion_line, is_diff_header_line,
};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::exec::events::{
    ItemCompletedEvent, ItemStartedEvent, PlanDeltaEvent, PlanItem, ThreadEvent, ThreadItem,
    ThreadItemDetails,
};
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::providers::ReasoningSegment;
use vtcode_core::tools::handlers::plan_mode::{PlanLifecyclePhase, persist_plan_draft};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::InlineHandle;

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::state::CtrlCState;

const AUTONOMOUS_CONTINUE_DIRECTIVE: &str = "Do not stop with intent-only updates. Execute the next concrete action now, then report completion or blocker.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InterimTextContinuationDecision {
    should_continue: bool,
    reason: &'static str,
    is_interim_progress: bool,
    last_user_follow_up: bool,
    recent_tool_activity: bool,
    last_user_requested_progressive_work: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Exit,
    Blocked { reason: Option<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedAssistantToolCall {
    raw_call: uni::ToolCall,
    parsed_args: Option<serde_json::Value>,
    args_error: Option<String>,
    is_parallel_safe: bool,
    is_command_execution: bool,
}

impl PreparedAssistantToolCall {
    pub(crate) fn new(raw_call: uni::ToolCall) -> Self {
        let tool_name = raw_call
            .function
            .as_ref()
            .map(|function| function.name.as_str())
            .unwrap_or(raw_call.call_type.as_str());

        let (parsed_args, args_error, is_parallel_safe, is_command_execution) = if raw_call
            .function
            .is_none()
        {
            (
                None,
                Some("tool call missing function details".to_string()),
                false,
                false,
            )
        } else {
            match raw_call.parsed_arguments() {
                Ok(args) => {
                    let is_parallel_safe =
                        vtcode_core::tools::tool_intent::is_parallel_safe_call(tool_name, &args);
                    let is_command_execution =
                        vtcode_core::tools::tool_intent::is_command_run_tool_call(tool_name, &args);
                    (Some(args), None, is_parallel_safe, is_command_execution)
                }
                Err(err) => (None, Some(err.to_string()), false, false),
            }
        };

        Self {
            raw_call,
            parsed_args,
            args_error,
            is_parallel_safe,
            is_command_execution,
        }
    }

    pub(crate) fn raw_call(&self) -> &uni::ToolCall {
        &self.raw_call
    }

    pub(crate) fn into_raw_call(self) -> uni::ToolCall {
        self.raw_call
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.raw_call.id
    }

    pub(crate) fn tool_name(&self) -> &str {
        self.raw_call
            .function
            .as_ref()
            .map(|function| function.name.as_str())
            .unwrap_or(self.raw_call.call_type.as_str())
    }

    pub(crate) fn args(&self) -> Option<&serde_json::Value> {
        self.parsed_args.as_ref()
    }

    pub(crate) fn args_error(&self) -> Option<&str> {
        self.args_error.as_deref()
    }

    pub(crate) fn is_parallel_safe(&self) -> bool {
        self.is_parallel_safe
    }

    pub(crate) fn is_command_execution(&self) -> bool {
        self.is_command_execution
    }
}

/// Result of processing a single turn
pub(crate) enum TurnProcessingResult {
    /// Turn resulted in tool calls that need to be executed
    ToolCalls {
        tool_calls: Vec<PreparedAssistantToolCall>,
        assistant_text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
    },
    /// Turn resulted in a text response
    TextResponse {
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        proposed_plan: Option<String>,
    },
    /// Turn resulted in no actionable output
    Empty,
}

pub(crate) enum TurnHandlerOutcome {
    Continue,
    Break(TurnLoopResult),
}

pub(crate) struct TurnOutcomeContext<'a> {
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub completed_turn_prompt: Option<&'a str>,
    pub completed_turn_prompt_message_index: Option<usize>,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub default_placeholder: &'a Option<String>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub next_checkpoint_turn: &'a mut usize,
    pub session_end_reason: &'a mut SessionEndReason,
    pub turn_elapsed: Duration,
    pub show_turn_timer: bool,
    pub workspace: &'a std::path::Path,
    pub session_id: &'a str,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
}

pub(crate) struct ToolContext<'a> {
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub tool_permission_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub safety_validator: &'a Arc<
        tokio::sync::RwLock<
            crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator,
        >,
    >,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
}

pub(crate) struct LLMContext<'a> {
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub config: &'a mut vtcode_core::config::types::AgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
}

pub(crate) struct UIContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut vtcode_tui::InlineSession,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
}

pub(crate) struct TurnProcessingState<'a> {
    pub session_stats: &'a mut SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub working_history: &'a mut Vec<uni::Message>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub full_auto: bool,
    pub harness_state: &'a mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

pub(crate) struct TurnProcessingContextParts<'a> {
    pub tool: ToolContext<'a>,
    pub llm: LLMContext<'a>,
    pub ui: UIContext<'a>,
    pub state: TurnProcessingState<'a>,
}

/// Context for turn processing operations
pub(crate) struct TurnProcessingContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub working_history: &'a mut Vec<uni::Message>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub session: &'a mut vtcode_tui::InlineSession,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub safety_validator: &'a Arc<
        tokio::sync::RwLock<
            crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator,
        >,
    >,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub config: &'a mut vtcode_core::config::types::AgentConfig,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub full_auto: bool,
    // Phase 4 Integration
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub harness_state: &'a mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a> TurnProcessingContext<'a> {
    pub(crate) fn from_parts(parts: TurnProcessingContextParts<'a>) -> Self {
        let TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        } = parts;

        Self {
            renderer: ui.renderer,
            handle: ui.handle,
            session_stats: state.session_stats,
            auto_exit_plan_mode_attempted: state.auto_exit_plan_mode_attempted,
            mcp_panel_state: state.mcp_panel_state,
            tool_result_cache: tool.tool_result_cache,
            approval_recorder: tool.approval_recorder,
            decision_ledger: llm.decision_ledger,
            working_history: state.working_history,
            turn_metadata_cache: state.turn_metadata_cache,
            tool_registry: tool.tool_registry,
            tools: tool.tools,
            tool_catalog: tool.tool_catalog,
            ctrl_c_state: ui.ctrl_c_state,
            ctrl_c_notify: ui.ctrl_c_notify,
            vt_cfg: llm.vt_cfg,
            context_manager: llm.context_manager,
            last_forced_redraw: ui.last_forced_redraw,
            input_status_state: ui.input_status_state,
            session: ui.session,
            lifecycle_hooks: ui.lifecycle_hooks,
            default_placeholder: ui.default_placeholder,
            tool_permission_cache: tool.tool_permission_cache,
            safety_validator: tool.safety_validator,
            provider_client: llm.provider_client,
            config: llm.config,
            traj: llm.traj,
            full_auto: state.full_auto,
            circuit_breaker: tool.circuit_breaker,
            tool_health_tracker: tool.tool_health_tracker,
            rate_limiter: tool.rate_limiter,
            telemetry: tool.telemetry,
            autonomous_executor: tool.autonomous_executor,
            error_recovery: tool.error_recovery,
            harness_state: state.harness_state,
            harness_emitter: state.harness_emitter,
            steering_receiver: state.steering_receiver,
        }
    }

    pub(crate) fn parts_mut(&mut self) -> TurnProcessingContextParts<'_> {
        let tool = ToolContext {
            tool_result_cache: self.tool_result_cache,
            approval_recorder: self.approval_recorder,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_catalog: self.tool_catalog,
            tool_permission_cache: self.tool_permission_cache,
            safety_validator: self.safety_validator,
            circuit_breaker: self.circuit_breaker,
            tool_health_tracker: self.tool_health_tracker,
            rate_limiter: self.rate_limiter,
            telemetry: self.telemetry,
            autonomous_executor: self.autonomous_executor,
            error_recovery: self.error_recovery,
        };
        let llm = LLMContext {
            provider_client: self.provider_client,
            config: self.config,
            vt_cfg: self.vt_cfg,
            context_manager: self.context_manager,
            decision_ledger: self.decision_ledger,
            traj: self.traj,
        };
        let ui = UIContext {
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state: self.input_status_state,
        };
        let state = TurnProcessingState {
            session_stats: self.session_stats,
            auto_exit_plan_mode_attempted: self.auto_exit_plan_mode_attempted,
            mcp_panel_state: self.mcp_panel_state,
            working_history: self.working_history,
            turn_metadata_cache: self.turn_metadata_cache,
            full_auto: self.full_auto,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
            steering_receiver: self.steering_receiver,
        };

        TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        }
    }

    /// Creates a TurnLoopContext from this TurnProcessingContext.
    /// This is used when calling handle_tool_execution_result which requires TurnLoopContext.
    pub(crate) fn as_turn_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_> {
        let TurnProcessingContextParts {
            tool: tool_ctx,
            llm: llm_ctx,
            ui: ui_ctx,
            state,
        } = self.parts_mut();

        crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext::new(
            ui_ctx.renderer,
            ui_ctx.handle,
            ui_ctx.session,
            state.session_stats,
            state.auto_exit_plan_mode_attempted,
            state.mcp_panel_state,
            tool_ctx.tool_result_cache,
            tool_ctx.approval_recorder,
            llm_ctx.decision_ledger,
            tool_ctx.tool_registry,
            tool_ctx.tools,
            tool_ctx.tool_catalog,
            ui_ctx.ctrl_c_state,
            ui_ctx.ctrl_c_notify,
            llm_ctx.context_manager,
            ui_ctx.last_forced_redraw,
            ui_ctx.input_status_state,
            ui_ctx.lifecycle_hooks,
            ui_ctx.default_placeholder,
            tool_ctx.tool_permission_cache,
            tool_ctx.safety_validator,
            tool_ctx.circuit_breaker,
            tool_ctx.tool_health_tracker,
            tool_ctx.rate_limiter,
            tool_ctx.telemetry,
            tool_ctx.autonomous_executor,
            tool_ctx.error_recovery,
            state.harness_state,
            state.harness_emitter,
            llm_ctx.config,
            llm_ctx.vt_cfg,
            state.turn_metadata_cache,
            llm_ctx.provider_client,
            llm_ctx.traj,
            state.full_auto,
            state.steering_receiver,
        )
    }

    /// Creates a RunLoopContext directly from this TurnProcessingContext,
    /// skipping the intermediate TurnLoopContext conversion.
    pub(crate) fn as_run_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
        let TurnProcessingContextParts {
            tool: tool_ctx,
            llm: llm_ctx,
            ui: ui_ctx,
            state,
        } = self.parts_mut();

        crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
            ui_ctx.renderer,
            ui_ctx.handle,
            tool_ctx.tool_registry,
            tool_ctx.tools,
            tool_ctx.tool_result_cache,
            tool_ctx.tool_permission_cache,
            llm_ctx.decision_ledger,
            state.session_stats,
            state.mcp_panel_state,
            tool_ctx.approval_recorder,
            ui_ctx.session,
            Some(tool_ctx.safety_validator),
            llm_ctx.traj,
            state.harness_state,
            state.harness_emitter,
        )
    }

    pub(crate) fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        response_streamed: bool,
        phase: Option<uni::AssistantPhase>,
    ) -> anyhow::Result<()> {
        let mut text = text;
        let detail_reasoning = reasoning_details.as_deref().and_then(
            vtcode_core::llm::providers::common::extract_reasoning_text_from_serialized_details,
        );
        if should_suppress_redundant_diff_recap(self.working_history, &text) {
            text.clear();
        }
        let has_visible_text = !text.trim().is_empty();
        if !reasoning.is_empty()
            || reasoning_details
                .as_ref()
                .is_some_and(|details| !details.is_empty())
        {
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "reasoning_observed",
                run_id = %self.harness_state.run_id.0,
                turn_id = %self.harness_state.turn_id.0,
                phase = match phase {
                    Some(uni::AssistantPhase::Commentary) => "commentary",
                    Some(uni::AssistantPhase::FinalAnswer) => "final_answer",
                    None => "unspecified",
                },
                reasoning_segments = reasoning.len(),
                reasoning_details = reasoning_details.as_ref().map_or(0, Vec::len),
                has_detail_reasoning = detail_reasoning.is_some(),
                has_visible_text,
                response_streamed,
                "turn metric"
            );
        }

        if !response_streamed {
            use vtcode_core::utils::ansi::MessageStyle;
            if !text.trim().is_empty() {
                self.renderer.line(MessageStyle::Response, &text)?;
            }
            let mut rendered_reasoning = detail_reasoning
                .is_some()
                .then(|| Vec::with_capacity(reasoning.len()));

            for segment in &reasoning {
                if let Some(stage) = &segment.stage {
                    self.handle.set_reasoning_stage(Some(stage.clone()));
                }

                let reasoning_text = &segment.text;
                if !reasoning_text.trim().is_empty() {
                    let duplicates_content =
                        has_visible_text && reasoning_duplicates_content(reasoning_text, &text);
                    if !duplicates_content {
                        let cleaned_for_display =
                            vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                        if cleaned_for_display.trim().is_empty() {
                            continue;
                        }
                        self.renderer
                            .line(MessageStyle::Reasoning, &cleaned_for_display)?;
                        if let Some(rendered_reasoning) = rendered_reasoning.as_mut() {
                            rendered_reasoning.push(cleaned_for_display);
                        }
                    }
                }
            }

            if let Some(detail_text) = detail_reasoning.as_deref() {
                let cleaned_detail = vtcode_core::llm::providers::clean_reasoning_text(detail_text);
                let duplicates_content =
                    has_visible_text && reasoning_duplicates_content(&cleaned_detail, &text);
                let duplicates_rendered =
                    rendered_reasoning
                        .as_ref()
                        .is_some_and(|rendered_reasoning| {
                            rendered_reasoning.iter().any(|existing: &String| {
                                reasoning_duplicates_content(existing, &cleaned_detail)
                                    || reasoning_duplicates_content(&cleaned_detail, existing)
                            })
                        });
                if !cleaned_detail.is_empty() && !duplicates_content && !duplicates_rendered {
                    self.renderer
                        .line(MessageStyle::Reasoning, &cleaned_detail)?;
                }
            }
            // Clear reasoning stage after rendering
            self.handle.set_reasoning_stage(None);
        }

        let combined_reasoning = build_combined_reasoning(&reasoning, detail_reasoning.as_deref());
        let include_reasoning = combined_reasoning
            .as_deref()
            .is_some_and(|combined_reasoning| {
                !reasoning_duplicates_content(combined_reasoning, &text)
            });
        let msg = uni::Message::assistant(text).with_phase(phase);
        let mut msg_with_reasoning = if include_reasoning {
            msg.with_reasoning(combined_reasoning)
        } else {
            msg
        };

        if let Some(details) = reasoning_details.filter(|d| !d.is_empty()) {
            let payload = details
                .into_iter()
                .map(|detail| parse_reasoning_detail_value(&detail))
                .collect::<Vec<_>>();
            msg_with_reasoning = msg_with_reasoning.with_reasoning_details(Some(payload));
        }

        if !msg_with_reasoning.content.as_text().is_empty()
            || msg_with_reasoning.reasoning.is_some()
            || msg_with_reasoning.reasoning_details.is_some()
        {
            push_assistant_message(self.working_history, msg_with_reasoning);
        }

        Ok(())
    }

    pub(crate) fn is_plan_mode(&self) -> bool {
        self.session_stats.is_plan_mode()
    }

    pub(crate) fn set_phase(
        &mut self,
        phase: crate::agent::runloop::unified::run_loop_context::TurnPhase,
    ) {
        self.harness_state.set_phase(phase);
    }

    pub(crate) fn restore_input_status(&mut self, left: Option<String>, right: Option<String>) {
        self.handle.set_input_status(left.clone(), right.clone());
        self.input_status_state.left = left;
        self.input_status_state.right = right;
    }

    pub(crate) fn push_system_message(&mut self, content: impl Into<String>) {
        self.working_history
            .push(uni::Message::system(content.into()));
    }

    pub(crate) fn reset_blocked_tool_call_streak(&mut self) {
        self.harness_state.reset_blocked_tool_call_streak();
    }

    pub(crate) fn record_blocked_tool_call(&mut self) -> usize {
        self.harness_state.record_blocked_tool_call()
    }

    pub(crate) fn blocked_tool_calls(&self) -> usize {
        self.harness_state.blocked_tool_calls
    }

    pub(crate) fn activate_recovery(&mut self, reason: impl Into<String>) {
        self.harness_state.activate_recovery(reason);
    }

    pub(crate) fn activate_recovery_with_mode(
        &mut self,
        reason: impl Into<String>,
        mode: RecoveryMode,
    ) {
        self.harness_state.activate_recovery_with_mode(reason, mode);
    }

    pub(crate) fn is_recovery_active(&self) -> bool {
        self.harness_state.is_recovery_active()
    }

    pub(crate) fn recovery_reason(&self) -> Option<&str> {
        self.harness_state.recovery_reason()
    }

    pub(crate) fn recovery_pass_used(&self) -> bool {
        self.harness_state.recovery_pass_used()
    }

    pub(crate) fn recovery_is_tool_free(&self) -> bool {
        self.harness_state.recovery_is_tool_free()
    }

    pub(crate) fn consume_recovery_pass(&mut self) -> bool {
        self.harness_state.consume_recovery_pass()
    }

    pub(crate) fn finish_recovery_pass(&mut self) -> bool {
        self.harness_state.finish_recovery_pass()
    }

    pub(crate) fn push_tool_response<S>(&mut self, tool_call_id: S, content: String)
    where
        S: AsRef<str> + Into<String>,
    {
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::push_tool_response(
            self.working_history,
            tool_call_id,
            content,
        );
    }

    pub(crate) async fn record_recovery_error(
        &self,
        scope: &str,
        error: &anyhow::Error,
        error_type: vtcode_core::core::agent::error_recovery::ErrorType,
    ) {
        let mut recovery = self.error_recovery.write().await;
        recovery.record_error(scope, format!("{:#}", error), error_type);
    }

    pub(crate) async fn turn_metadata(&mut self) -> anyhow::Result<Option<serde_json::Value>> {
        if let Some(cached) = self.turn_metadata_cache.as_ref() {
            return Ok(cached.clone());
        }

        let metadata = vtcode_core::turn_metadata::build_turn_metadata_value_with_timeout(
            &self.config.workspace,
            std::time::Duration::from_millis(250),
        )
        .await?;
        *self.turn_metadata_cache = Some(metadata.clone());
        Ok(metadata)
    }

    pub(crate) async fn handle_text_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        proposed_plan: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        let recovery_pass_response = self.is_recovery_active() && self.recovery_pass_used();
        let tool_free_recovery_pass = recovery_pass_response && self.recovery_is_tool_free();
        let recovery_progress_only = tool_free_recovery_pass && is_interim_progress_update(&text);
        let continuation_decision = if tool_free_recovery_pass {
            InterimTextContinuationDecision {
                should_continue: false,
                reason: "recovery_pass",
                is_interim_progress: recovery_progress_only,
                last_user_follow_up: false,
                recent_tool_activity: false,
                last_user_requested_progressive_work: false,
            }
        } else {
            evaluate_interim_text_continuation(
                self.full_auto,
                self.session_stats.is_plan_mode(),
                self.working_history,
                &text,
            )
        };
        self.handle_assistant_response(
            text,
            reasoning,
            reasoning_details,
            response_streamed,
            Some(uni::AssistantPhase::FinalAnswer),
        )?;

        if recovery_pass_response {
            self.finish_recovery_pass();
            if recovery_progress_only {
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(
                        "Recovery mode requested a final tool-free synthesis pass, but the model only described another next step."
                            .to_string(),
                    ),
                }));
            }
        }

        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "text_response_decision",
            run_id = %self.harness_state.run_id.0,
            turn_id = %self.harness_state.turn_id.0,
            should_continue = continuation_decision.should_continue,
            reason = continuation_decision.reason,
            is_interim_progress = continuation_decision.is_interim_progress,
            last_user_follow_up = continuation_decision.last_user_follow_up,
            recent_tool_activity = continuation_decision.recent_tool_activity,
            last_user_requested_progressive_work =
                continuation_decision.last_user_requested_progressive_work,
            recovery_pass_response,
            tool_free_recovery_pass,
            plan_mode = self.session_stats.is_plan_mode(),
            full_auto = self.full_auto,
            history_len = self.working_history.len(),
            "turn metric"
        );

        if continuation_decision.should_continue {
            push_system_directive_once(self.working_history, AUTONOMOUS_CONTINUE_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }

        if self.session_stats.is_plan_mode()
            && let Some(plan_text) = proposed_plan
        {
            self.emit_plan_events(&plan_text).await;
            persist_plan_draft(&self.tool_registry.plan_mode_state(), &plan_text).await?;
            self.tool_registry
                .plan_mode_state()
                .set_phase(PlanLifecyclePhase::DraftReady);
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }

    async fn emit_plan_events(&self, plan_text: &str) {
        let Some(emitter) = self.harness_emitter else {
            return;
        };

        let turn_id = self.harness_state.turn_id.0.clone();
        let thread_id = self.harness_state.run_id.0.clone();
        let item_id = format!("{turn_id}-plan");

        let start_item = ThreadItem {
            id: item_id.clone(),
            details: ThreadItemDetails::Plan(PlanItem {
                text: String::new(),
            }),
        };
        let _ = emitter.emit(ThreadEvent::ItemStarted(ItemStartedEvent {
            item: start_item,
        }));

        let _ = emitter.emit(ThreadEvent::PlanDelta(PlanDeltaEvent {
            thread_id,
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            delta: plan_text.to_string(),
        }));

        let completed_item = ThreadItem {
            id: item_id,
            details: ThreadItemDetails::Plan(PlanItem {
                text: plan_text.to_string(),
            }),
        };
        let _ = emitter.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: completed_item,
        }));
    }
}

fn reasoning_duplicates_content(reasoning: &str, content: &str) -> bool {
    let r = reasoning.trim();
    let c = content.trim();
    if r.is_empty() || c.is_empty() {
        return false;
    }
    r == c || r.contains(c) || c.contains(r)
}

fn build_combined_reasoning(
    reasoning: &[ReasoningSegment],
    detail_reasoning: Option<&str>,
) -> Option<String> {
    let capacity = reasoning
        .iter()
        .map(|segment| segment.text.len())
        .sum::<usize>()
        + reasoning.len().saturating_sub(1);
    let mut combined_reasoning = String::with_capacity(capacity);

    for segment in reasoning {
        if !combined_reasoning.is_empty() {
            combined_reasoning.push('\n');
        }
        combined_reasoning.push_str(&segment.text);
    }

    if combined_reasoning.trim().is_empty()
        && let Some(detail_reasoning) = detail_reasoning
    {
        return Some(detail_reasoning.to_string());
    }

    if combined_reasoning.is_empty() {
        None
    } else {
        Some(combined_reasoning)
    }
}

fn parse_reasoning_detail_value(detail: &str) -> serde_json::Value {
    let trimmed = detail.trim();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        return parsed;
    }
    serde_json::Value::String(detail.to_string())
}

fn push_assistant_message(history: &mut Vec<uni::Message>, msg: uni::Message) {
    if let Some(last) = history.last_mut()
        && last.role == uni::MessageRole::Assistant
        && last.tool_calls.is_none()
        && last.phase == msg.phase
    {
        last.content = msg.content;
        last.reasoning = msg.reasoning;
        last.reasoning_details = msg.reasoning_details;
    } else {
        history.push(msg);
    }
}

fn evaluate_interim_text_continuation(
    full_auto: bool,
    plan_mode: bool,
    history: &[uni::Message],
    text: &str,
) -> InterimTextContinuationDecision {
    let is_interim_progress = is_interim_progress_update(text);
    let last_user_follow_up = last_user_message_is_follow_up(history);
    let recent_tool_activity = has_recent_tool_activity(history);
    let last_user_requested_progressive_work = last_user_requested_progressive_work(history);

    if plan_mode {
        return InterimTextContinuationDecision {
            should_continue: false,
            reason: "plan_mode",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if !is_interim_progress {
        return InterimTextContinuationDecision {
            should_continue: false,
            reason: "non_interim_text",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if last_user_follow_up {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "follow_up_prompt",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if recent_tool_activity {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "recent_tool_activity",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    if last_user_requested_progressive_work {
        return InterimTextContinuationDecision {
            should_continue: true,
            reason: "progressive_request",
            is_interim_progress,
            last_user_follow_up,
            recent_tool_activity,
            last_user_requested_progressive_work,
        };
    }

    InterimTextContinuationDecision {
        should_continue: false,
        reason: if full_auto {
            "awaiting_model_action"
        } else {
            "interactive_mode"
        },
        is_interim_progress,
        last_user_follow_up,
        recent_tool_activity,
        last_user_requested_progressive_work,
    }
}

fn push_system_directive_once(history: &mut Vec<uni::Message>, directive: &str) {
    let already_present = history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().trim() == directive
    });
    if !already_present {
        history.push(uni::Message::system(directive.to_string()));
    }
}

fn last_user_message_is_follow_up(history: &[uni::Message]) -> bool {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .is_some_and(|message| {
            crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                message.content.as_text().as_ref(),
            )
        })
}

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool
            || message.tool_call_id.is_some()
            || message.tool_calls.is_some()
    })
}

fn last_user_requested_progressive_work(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    [
        "explore",
        "inspect",
        "look into",
        "investigate",
        "debug",
        "trace",
        "check",
        "review",
        "analy",
        "walk through",
        "run ",
        "execute",
        "format",
        "cargo fmt",
        "cargo check",
        "cargo test",
        "fix",
        "edit",
        "update",
        "change",
        "modify",
        "scan",
        "search",
        "grep",
        "ast-grep",
        "find ",
        "use vt code",
        "semantic code understanding",
        "show me how",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn is_interim_progress_update(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > 280 {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    let intent_prefixes = [
        "let me ",
        "i'll ",
        "i will ",
        "i need to ",
        "i am going to ",
        "i'm going to ",
        "now i need to ",
        "continuing ",
        "next i need to ",
        "next, i'll ",
        "now i'll ",
        "let us ",
    ];
    let starts_with_intent = intent_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        || starts_with_present_progress_update(&lower);
    if !starts_with_intent {
        return false;
    }

    let user_input_markers = [
        "could you",
        "can you",
        "please provide",
        "need your",
        "need you to",
        "which option",
    ];
    if trimmed.contains('?')
        || user_input_markers
            .iter()
            .any(|marker| lower.contains(marker))
    {
        return false;
    }

    let conclusive_markers = [
        "completed",
        "done",
        "fixed",
        "resolved",
        "summary",
        "final review",
        "final blocker",
        "next action",
        "what changed",
        "validation",
        "passed",
        "passes",
        "cannot proceed",
        "can't proceed",
        "blocked by",
        "all set",
    ];
    !conclusive_markers
        .iter()
        .any(|marker| lower.contains(marker))
}

fn starts_with_present_progress_update(lower: &str) -> bool {
    let present_progress_prefixes = [
        "running ",
        "checking ",
        "formatting ",
        "scanning ",
        "inspecting ",
        "searching ",
        "reading ",
        "reviewing ",
        "tracing ",
        "debugging ",
    ];
    let forward_markers = [
        " now",
        " then ",
        " next ",
        " follow-up",
        " to confirm",
        " to check",
        " to verify",
        " to inspect",
    ];

    present_progress_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
        && forward_markers.iter().any(|marker| lower.contains(marker))
}

fn should_suppress_redundant_diff_recap(history: &[uni::Message], assistant_text: &str) -> bool {
    if assistant_text.trim().is_empty() {
        return false;
    }
    if !is_redundant_diff_recap_text(assistant_text) {
        return false;
    }
    if !has_recent_git_diff_tool_output(history) {
        return false;
    }
    if !last_user_requested_diff_view(history) {
        return false;
    }
    if last_user_requested_diff_analysis(history) {
        return false;
    }
    true
}

fn is_redundant_diff_recap_text(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("diff for ")
        || lower.starts_with("the diff shows")
        || lower.starts_with("changes in ")
        || lower.starts_with("```diff")
        || lower.starts_with("diff preview changes")
        || lower.contains("\n**diff preview changes**")
        || (trimmed.contains("```") && is_diff_like_fenced_recap(trimmed))
}

fn is_diff_like_fenced_recap(text: &str) -> bool {
    let mut has_fence = false;
    let mut has_diff_marker = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            has_fence = true;
            continue;
        }
        if is_diff_header_line(trimmed)
            || is_diff_addition_line(trimmed)
            || is_diff_deletion_line(trimmed)
        {
            has_diff_marker = true;
        }
    }
    has_fence && has_diff_marker
}

fn has_recent_git_diff_tool_output(history: &[uni::Message]) -> bool {
    history
        .iter()
        .rev()
        .take(12)
        .any(message_is_git_diff_tool_output)
}

fn message_is_git_diff_tool_output(message: &uni::Message) -> bool {
    if message.role != uni::MessageRole::Tool {
        return false;
    }

    let content = message.content.as_text();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content.as_ref()) {
        if value
            .get("content_type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|content_type| content_type == "git_diff")
        {
            return true;
        }
        if value
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| command.trim_start().starts_with("git diff"))
        {
            return true;
        }
        if value
            .get("output")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|output| output.contains("diff --git "))
        {
            return true;
        }
    }

    content.contains("\"content_type\":\"git_diff\"") || content.contains("diff --git ")
}

fn last_user_message_text(history: &[uni::Message]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::User)
        .map(|message| message.content.as_text().to_ascii_lowercase())
}

fn last_user_requested_diff_view(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    [
        "show diff",
        "git diff",
        "view diff",
        "show changes",
        "what changed",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn last_user_requested_diff_analysis(history: &[uni::Message]) -> bool {
    let Some(text) = last_user_message_text(history) else {
        return false;
    };
    ["analy", "explain", "summar", "review", "why", "interpret"]
        .iter()
        .any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    #[test]
    fn follow_up_prompt_detection_accepts_continue_variants() {
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue."));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("go on"));
        assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("please continue"));
        assert!(
            crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                "Continue autonomously from the last stalled turn. Stall reason: x."
            )
        );
        assert!(
            !crate::agent::runloop::unified::state::is_follow_up_prompt_like(
                "run cargo clippy and fix"
            )
        );
    }

    #[test]
    fn interim_progress_detection_requires_non_conclusive_intent_text() {
        assert!(is_interim_progress_update(
            "Let me fix the second collapsible if statement:"
        ));
        assert!(is_interim_progress_update(
            "Let me fix the second collapsible if statement in the Anthropic provider:"
        ));
        assert!(is_interim_progress_update(
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        ));
        assert!(is_interim_progress_update(
            "I'll continue with the next fix."
        ));
        assert!(is_interim_progress_update(
            "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
        ));
        assert!(!is_interim_progress_update(
            "I need you to choose which option to apply."
        ));
        assert!(!is_interim_progress_update(
            "Running cargo fmt uses rustfmt to rewrite the source files."
        ));
        assert!(!is_interim_progress_update(
            "Completed. All requested fixes are done."
        ));
        assert!(!is_interim_progress_update(
            "Final review: two blockers remain with next action."
        ));
    }

    #[test]
    fn autonomous_continue_triggers_for_follow_up_and_interim_text() {
        let history = vec![uni::Message::user("continue".to_string())];
        assert!(
            evaluate_interim_text_continuation(true, false, &history, "Let me fix the next issue.")
                .should_continue
        );
        assert!(
            !evaluate_interim_text_continuation(true, true, &history, "Let me fix the next issue.")
                .should_continue
        );
        assert!(
            evaluate_interim_text_continuation(
                false,
                false,
                &history,
                "Let me fix the next issue."
            )
            .should_continue
        );
    }

    #[test]
    fn autonomous_continue_triggers_for_interim_text_after_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will run cargo clippy now.".to_string()).with_tool_calls(
                vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "unified_exec".to_string(),
                    "{}".to_string(),
                )],
            ),
            uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_triggers_for_execution_request_without_prior_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will start now.".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_triggers_for_exploration_request_without_full_auto() {
        let history = vec![
            uni::Message::user("explore about vtcode core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code."
        )
        .should_continue);
    }

    #[test]
    fn autonomous_continue_does_not_trigger_for_explanatory_request_without_full_auto() {
        let history = vec![
            uni::Message::user("tell me about core agent loop".to_string()),
            uni::Message::assistant("I can help.".to_string()),
        ];

        assert!(!evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code."
        )
        .should_continue);
    }

    #[tokio::test]
    async fn recovery_pass_progress_only_text_breaks_turn_instead_of_continuing() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "Let me try a narrower search next.".to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("recovery response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })
        ));
        assert!(!ctx.is_recovery_active());
    }

    #[tokio::test]
    async fn tool_enabled_recovery_pass_can_continue_after_interim_progress() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user(
            "run cargo fmt and follow up".to_string(),
        ));
        ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
        assert!(ctx.consume_recovery_pass());

        let outcome = ctx
            .handle_text_response(
                "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
                    .to_string(),
                Vec::new(),
                None,
                None,
                false,
            )
            .await
            .expect("tool-enabled recovery response should be handled");

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!ctx.is_recovery_active());
        assert!(ctx.recovery_pass_used());
    }

    #[test]
    fn suppresses_redundant_diff_recap_after_git_diff_view_request() {
        let history = vec![
            uni::Message::user("show diff src/main.rs".to_string()),
            uni::Message::tool_response(
                "call_1".to_string(),
                r#"{"content_type":"git_diff","command":"git diff -- src/main.rs","output":"diff --git a/src/main.rs b/src/main.rs"}"#.to_string(),
            ),
        ];

        assert!(should_suppress_redundant_diff_recap(
            &history,
            "Diff for src/main.rs:\n```diff\n@@ -1 +1 @@\n```"
        ));
    }

    #[test]
    fn does_not_suppress_diff_recap_when_user_asked_for_analysis() {
        let history = vec![
            uni::Message::user("analyze this diff and explain".to_string()),
            uni::Message::tool_response(
                "call_1".to_string(),
                r#"{"content_type":"git_diff","command":"git diff -- src/main.rs"}"#.to_string(),
            ),
        ];

        assert!(!should_suppress_redundant_diff_recap(
            &history,
            "The diff shows one behavior change."
        ));
    }

    #[test]
    fn suppresses_heading_style_diff_recap_after_view_request() {
        let history = vec![
            uni::Message::user("show diff on vtcode-tui/src/ui/markdown.rs".to_string()),
            uni::Message::tool_response(
                "call_1".to_string(),
                r#"{"content_type":"git_diff","command":"git diff -- vtcode-tui/src/ui/markdown.rs","output":"diff --git a/vtcode-tui/src/ui/markdown.rs b/vtcode-tui/src/ui/markdown.rs\n@@ -1 +1 @@\n- old\n+ new"}"#.to_string(),
            ),
        ];

        assert!(should_suppress_redundant_diff_recap(
            &history,
            "Implemented updated syntax highlighting for diff previews.\n\n**Diff preview changes**\n\n```\n@@\n- old\n+ new\n```\n"
        ));
    }

    #[test]
    fn parse_reasoning_detail_value_decodes_stringified_json_object() {
        let parsed =
            parse_reasoning_detail_value(r#"{"type":"reasoning.text","id":"r1","text":"hello"}"#);
        assert!(parsed.is_object());
        assert_eq!(parsed["type"], "reasoning.text");
    }

    #[test]
    fn build_combined_reasoning_falls_back_to_detail_text() {
        let combined = build_combined_reasoning(&[], Some("detail trace"));
        assert_eq!(combined.as_deref(), Some("detail trace"));
    }

    #[test]
    fn build_combined_reasoning_preserves_whitespace_only_segments_without_detail() {
        let combined = build_combined_reasoning(&[ReasoningSegment::new("  ", None)], None);
        assert_eq!(combined.as_deref(), Some("  "));
    }

    #[test]
    fn push_assistant_message_preserves_reasoning_details_when_merging() {
        let mut history = vec![uni::Message::assistant("old".to_string())];
        let new_msg =
            uni::Message::assistant("new".to_string()).with_reasoning_details(Some(vec![
                serde_json::json!({"type":"reasoning.text","text":"trace"}),
            ]));

        push_assistant_message(&mut history, new_msg);

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content.as_text(), "new");
        assert_eq!(
            history[0].reasoning_details,
            Some(vec![
                serde_json::json!({"type":"reasoning.text","text":"trace"})
            ])
        );
    }

    #[test]
    fn push_assistant_message_keeps_different_phases_separate() {
        let mut history = vec![
            uni::Message::assistant("working".to_string())
                .with_phase(Some(uni::AssistantPhase::Commentary)),
        ];
        let new_msg = uni::Message::assistant("done".to_string())
            .with_phase(Some(uni::AssistantPhase::FinalAnswer));

        push_assistant_message(&mut history, new_msg);

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].phase, Some(uni::AssistantPhase::Commentary));
        assert_eq!(history[1].phase, Some(uni::AssistantPhase::FinalAnswer));
    }
}
