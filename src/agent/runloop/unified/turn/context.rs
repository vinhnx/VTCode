use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::plan_confirmation::{
    PlanConfirmationOutcome, execute_plan_confirmation,
};
use crate::agent::runloop::unified::plan_mode_state::transition_to_edit_mode;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::exec::events::{
    ItemCompletedEvent, ItemStartedEvent, PlanDeltaEvent, PlanItem, ThreadEvent, ThreadItem,
    ThreadItemDetails,
};
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::providers::ReasoningSegment;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::InlineHandle;
use vtcode_tui::PlanContent;

use crate::agent::runloop::unified::state::CtrlCState;

const AUTONOMOUS_CONTINUE_DIRECTIVE: &str = "Do not stop with intent-only updates (for example: 'let me check...'). Execute the next concrete action now, then provide a completion summary or explicit blocker with next action.";

pub enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Exit,
    Blocked { reason: Option<String> },
}

/// Result of processing a single turn
pub(crate) enum TurnProcessingResult {
    /// Turn resulted in tool calls that need to be executed
    ToolCalls {
        tool_calls: Vec<uni::ToolCall>,
        assistant_text: String,
        reasoning: Vec<ReasoningSegment>,
    },
    /// Turn resulted in a text response
    TextResponse {
        text: String,
        reasoning: Vec<ReasoningSegment>,
        proposed_plan: Option<String>,
    },
    /// Turn resulted in no actionable output
    Empty,
    /// Turn was completed successfully (used in match exhaustiveness)
    #[allow(dead_code)]
    Completed,
    /// Turn was cancelled by user (used in match exhaustiveness)
    #[allow(dead_code)]
    Cancelled,
    /// Turn was aborted due to error (used in match exhaustiveness)
    #[allow(dead_code)]
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
    pub turn_elapsed: Duration,
    pub show_turn_timer: bool,
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
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
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
            tool_catalog: self.tool_catalog,
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
            steering_receiver: self.steering_receiver,
        }
    }

    /// Creates a RunLoopContext directly from this TurnProcessingContext,
    /// skipping the intermediate TurnLoopContext conversion.
    pub fn as_run_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
        crate::agent::runloop::unified::run_loop_context::RunLoopContext {
            renderer: self.renderer,
            handle: self.handle,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_result_cache: self.tool_result_cache,
            tool_permission_cache: self.tool_permission_cache,
            decision_ledger: self.decision_ledger,
            session_stats: self.session_stats,
            mcp_panel_state: self.mcp_panel_state,
            approval_recorder: self.approval_recorder,
            session: self.session,
            safety_validator: Some(self.safety_validator),
            traj: self.traj,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
        }
    }

    pub fn handle_assistant_response(
        &mut self,
        text: String,
        reasoning: Vec<ReasoningSegment>,
        response_streamed: bool,
    ) -> anyhow::Result<()> {
        if !response_streamed {
            use vtcode_core::utils::ansi::MessageStyle;
            if !text.trim().is_empty() {
                self.renderer.line(MessageStyle::Response, &text)?;
            }

            for segment in &reasoning {
                if let Some(stage) = &segment.stage {
                    self.handle.set_reasoning_stage(Some(stage.clone()));
                }

                let reasoning_text = &segment.text;
                if !reasoning_text.trim().is_empty() {
                    let duplicates_content = !text.trim().is_empty()
                        && reasoning_duplicates_content(reasoning_text, &text);
                    if !duplicates_content {
                        let cleaned_for_display =
                            vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
                        self.renderer
                            .line(MessageStyle::Reasoning, &cleaned_for_display)?;
                    }
                }
            }
            // Clear reasoning stage after rendering
            self.handle.set_reasoning_stage(None);
        }

        let combined_reasoning = reasoning
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let msg = uni::Message::assistant(text.clone());
        let msg_with_reasoning = if !combined_reasoning.is_empty() {
            if reasoning_duplicates_content(&combined_reasoning, &text) {
                msg
            } else {
                msg.with_reasoning(Some(combined_reasoning))
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
        reasoning: Vec<ReasoningSegment>,
        proposed_plan: Option<String>,
        response_streamed: bool,
    ) -> anyhow::Result<TurnHandlerOutcome> {
        let should_force_continue = should_continue_autonomously_after_interim_text(
            self.full_auto,
            self.session_stats.is_plan_mode(),
            self.working_history,
            &text,
        );
        self.handle_assistant_response(text, reasoning, response_streamed)?;

        if should_force_continue {
            push_system_directive_once(self.working_history, AUTONOMOUS_CONTINUE_DIRECTIVE);
            return Ok(TurnHandlerOutcome::Continue);
        }

        if self.session_stats.is_plan_mode()
            && let Some(plan_text) = proposed_plan
        {
            self.emit_plan_events(&plan_text).await;
            self.maybe_prompt_plan_implementation(plan_text).await?;
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

    async fn maybe_prompt_plan_implementation(&mut self, plan_text: String) -> anyhow::Result<()> {
        let require_confirmation = self
            .vt_cfg
            .map(|cfg| cfg.agent.require_plan_confirmation)
            .unwrap_or(true);

        if !require_confirmation {
            transition_to_edit_mode(self.tool_registry, self.session_stats, self.handle, true)
                .await;
            return Ok(());
        }

        let plan = PlanContent::from_markdown(
            "Implementation Plan".to_string(),
            &plan_text,
            None::<String>,
        );

        let confirmation = execute_plan_confirmation(
            self.handle,
            self.session,
            plan,
            self.ctrl_c_state,
            self.ctrl_c_notify,
        )
        .await?;

        if matches!(
            confirmation,
            PlanConfirmationOutcome::Execute
                | PlanConfirmationOutcome::AutoAccept
                | PlanConfirmationOutcome::ClearContextAutoAccept
        ) {
            self.handle.set_skip_confirmations(matches!(
                confirmation,
                PlanConfirmationOutcome::AutoAccept
                    | PlanConfirmationOutcome::ClearContextAutoAccept
            ));
            if matches!(
                confirmation,
                PlanConfirmationOutcome::ClearContextAutoAccept
            ) {
                self.session_stats.request_context_clear();
            }
            transition_to_edit_mode(self.tool_registry, self.session_stats, self.handle, true)
                .await;
        }

        Ok(())
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

fn should_continue_autonomously_after_interim_text(
    full_auto: bool,
    plan_mode: bool,
    history: &[uni::Message],
    text: &str,
) -> bool {
    if !full_auto || plan_mode {
        return false;
    }

    if !is_interim_progress_update(text) {
        return false;
    }

    if last_user_message_is_follow_up(history) {
        return true;
    }

    has_recent_tool_activity(history)
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
        .is_some_and(|message| is_follow_up_prompt_like(message.content.as_text().as_ref()))
}

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool
            || message.tool_call_id.is_some()
            || message.tool_calls.is_some()
    })
}

fn is_follow_up_prompt_like(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
        .to_ascii_lowercase();
    if normalized.starts_with("continue autonomously from the last stalled turn") {
        return true;
    }
    let words: Vec<&str> = normalized.split_whitespace().collect();
    matches!(
        words.as_slice(),
        ["continue"]
            | ["retry"]
            | ["proceed"]
            | ["go", "on"]
            | ["go", "ahead"]
            | ["keep", "going"]
            | ["please", "continue"]
            | ["continue", "please"]
            | ["please", "retry"]
            | ["retry", "please"]
            | ["continue", "with", "recommendation"]
            | ["continue", "with", "your", "recommendation"]
    )
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
        .any(|prefix| lower.starts_with(prefix));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_up_prompt_detection_accepts_continue_variants() {
        assert!(is_follow_up_prompt_like("continue"));
        assert!(is_follow_up_prompt_like("continue."));
        assert!(is_follow_up_prompt_like("go on"));
        assert!(is_follow_up_prompt_like("please continue"));
        assert!(is_follow_up_prompt_like(
            "Continue autonomously from the last stalled turn. Stall reason: x."
        ));
        assert!(!is_follow_up_prompt_like("run cargo clippy and fix"));
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
        assert!(!is_interim_progress_update(
            "I need you to choose which option to apply."
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
        assert!(should_continue_autonomously_after_interim_text(
            true,
            false,
            &history,
            "Let me fix the next issue."
        ));
        assert!(!should_continue_autonomously_after_interim_text(
            true,
            true,
            &history,
            "Let me fix the next issue."
        ));
        assert!(!should_continue_autonomously_after_interim_text(
            false,
            false,
            &history,
            "Let me fix the next issue."
        ));
    }

    #[test]
    fn autonomous_continue_triggers_for_interim_text_after_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will run cargo clippy now.".to_string()).with_tool_calls(
                vec![uni::ToolCall::function(
                    "call_1".to_string(),
                    "run_pty_cmd".to_string(),
                    "{}".to_string(),
                )],
            ),
            uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
        ];

        assert!(should_continue_autonomously_after_interim_text(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        ));
    }

    #[test]
    fn autonomous_continue_does_not_trigger_without_follow_up_or_tool_activity() {
        let history = vec![
            uni::Message::user("run cargo clippy and fix".to_string()),
            uni::Message::assistant("I will start now.".to_string()),
        ];

        assert!(!should_continue_autonomously_after_interim_text(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
        ));
    }
}
