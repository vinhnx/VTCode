use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::state::SessionStats;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;
use tokio::sync::Notify;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::unified::state::CtrlCState;

#[allow(dead_code)]
pub enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Exit,
    Blocked { reason: Option<String> },
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

pub(crate) enum TurnHandlerOutcome {
    Continue,
    Break(TurnLoopResult),
}

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

/// Context for turn processing operations
#[allow(dead_code)]
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
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse (HP-3 optimization)
    pub cached_tools: &'a Option<Arc<Vec<uni::ToolDefinition>>>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub session: &'a mut vtcode_core::ui::tui::InlineSession,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
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
}

impl<'a> TurnProcessingContext<'a> {
    /// Creates a TurnLoopContext from this TurnProcessingContext.
    /// This is used when calling handle_tool_execution_result which requires TurnLoopContext.
    pub fn as_turn_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_> {
        crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            session_stats: self.session_stats,
            auto_exit_plan_mode_attempted: self.auto_exit_plan_mode_attempted,
            mcp_panel_state: self.mcp_panel_state,
            tool_result_cache: self.tool_result_cache,
            approval_recorder: self.approval_recorder,
            decision_ledger: self.decision_ledger,
            tool_registry: self.tool_registry,
            tools: self.tools,
            cached_tools: self.cached_tools,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            context_manager: self.context_manager,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state: self.input_status_state,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            tool_permission_cache: self.tool_permission_cache,
            safety_validator: self.safety_validator,
            circuit_breaker: self.circuit_breaker,
            tool_health_tracker: self.tool_health_tracker,
            rate_limiter: self.rate_limiter,
            telemetry: self.telemetry,
            autonomous_executor: self.autonomous_executor,
            error_recovery: self.error_recovery,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
            config: self.config,
            vt_cfg: self.vt_cfg,
            provider_client: self.provider_client,
            traj: self.traj,
            full_auto: self.full_auto,
        }
    }

    pub fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<()> {
        if !response_streamed {
            use vtcode_core::utils::ansi::MessageStyle;
            if !text.trim().is_empty() {
                self.renderer.line(MessageStyle::Response, &text)?;
            }
            if let Some(reasoning_text) = reasoning.as_ref()
                && !reasoning_text.trim().is_empty()
            {
                let duplicates_content = !text.trim().is_empty()
                    && reasoning_duplicates_content(reasoning_text, &text);
                if !reasoning_text.trim().is_empty() && !duplicates_content {
                    let cleaned_for_display =
                        vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                    self.renderer
                        .line(MessageStyle::Reasoning, &cleaned_for_display)?;
                }
            }
        }

        let msg = uni::Message::assistant(text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            if reasoning_duplicates_content(&reasoning_text, &text) {
                msg
            } else {
                msg.with_reasoning(Some(reasoning_text))
            }
        } else {
            msg
        };

        if !text.is_empty() || msg_with_reasoning.reasoning.is_some() {
            push_assistant_message(self.working_history, msg_with_reasoning);
        }

        Ok(())
    }

    pub async fn handle_text_response(
        &mut self,
        text: String,
        reasoning: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        self.handle_assistant_response(text, reasoning, response_streamed)?;
        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
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

fn push_assistant_message(history: &mut Vec<uni::Message>, msg: uni::Message) {
    if let Some(last) = history.last_mut()
        && last.role == uni::MessageRole::Assistant
        && last.tool_calls.is_none()
    {
        last.content = msg.content;
        last.reasoning = msg.reasoning;
    } else {
        history.push(msg);
    }
}


