use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHandle;

#[derive(Default)]
pub(crate) struct PlanningWorkflowSessionState {
    interview_shown: bool,
    interview_pending: bool,
    turns: usize,
    interview_cycles_completed: usize,
    last_interview_cancelled: bool,
    entry_source: Option<PlanningEntrySource>,
    /// Set when the session budget is exhausted during planning. Prevents
    /// the interview from being re-forced on the next turn, which would
    /// loop forever because no further LLM calls are possible.
    budget_exhausted: bool,
    /// Set when the post-tool recovery cycle cap is reached during planning
    /// (repeated tool-free synthesis failures because the planning context is
    /// saturated). Prevents the interview from being re-forced on the next
    /// turn, which would re-research the still-huge context and fail again —
    /// looping forever across turns.
    recovery_exhausted: bool,
    /// Set when a `request_user_input` tool call is denied by a permanent
    /// capability/policy failure (e.g. the tool is not available in the
    /// current runtime) rather than the user cancelling the modal. Unlike
    /// cancellation, a policy denial will recur on every retry — this flag
    /// permanently stops the interview from being re-forced for the rest of
    /// the planning session, falling back to autonomous plan synthesis
    /// instead of looping (see checkpoint turn_655/turn_660).
    interview_denied: bool,
    /// Allows one bounded synthesis retry after an interview denial. The
    /// retry gives the model a direct instruction to emit a completed plan
    /// from the research already gathered instead of ending with an approval
    /// hint that has no draft behind it.
    plan_synthesis_retry_used: bool,
    /// Counts automatic validation-repair prompts issued during the current
    /// planning turn. The counter is deliberately turn-scoped so a failed
    /// draft cannot consume the repair budget for every later user turn.
    plan_validation_repair_reprompts: u8,
    /// Number of bounded planning retries waiting to be admitted through
    /// the turn loop. This is independent of the number of text responses
    /// already emitted: a retry may be scheduled after an interview denial
    /// or another ordinary planning response has used that budget.
    bounded_planning_follow_ups_pending: u8,
    /// Counts re-prompts issued after the model emitted pseudo-tool-call
    /// markup (XML-ish tool-call text no parser could execute) as a plan-mode
    /// text response. Bounded so a checkpoint that keeps emitting the same
    /// markup cannot loop the turn forever (turn_887/turn_888).
    pseudo_tool_call_reprompts: u32,
    /// Primary agent that was active before the planning workflow began.
    /// Used to restore execution to the prior mode when planning was entered
    /// by selecting the dedicated plan agent.
    previous_primary_agent: Option<String>,
    /// Configured execution agent to use when planning started from the
    /// dedicated `plan` agent without a previous execution agent.
    fallback_primary_agent: Option<String>,
    /// Telemetry identity for the latest unresolved plan approval request.
    pending_approval: Option<PendingPlanApproval>,
    /// Deferred full switch to the plan primary agent after a mid-turn
    /// `start_planning` entry. Must not end the current turn: research is
    /// supposed to continue in the entry turn. Always consumed at turn end
    /// (`take_plan_entry_agent_switch`); applied at the turn boundary unless
    /// a stronger handoff owns it. Applies on Blocked turns too — plan mode
    /// often blocks tools in the entry turn, and discarding would leave the
    /// execution agent selected while planning stays active.
    plan_entry_agent_switch_pending: bool,
}

/// Maximum number of pseudo-tool-call-markup re-prompts per planning session.
/// One re-prompt usually teaches the model to use the real tool-call channel;
/// two covers a repeat offense. Beyond that the turn ends with the cleaned
/// text so the user can steer.
pub(crate) const MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS: u32 = 2;

/// Maximum number of automatic validation-repair prompts after the initial
/// invalid plan candidate in one planning turn.
pub(crate) const MAX_PLAN_VALIDATION_REPAIR_REPROMPTS: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingPlanApproval {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
}

impl PlanningWorkflowSessionState {
    pub(crate) fn enter(&mut self, entry_source: PlanningEntrySource) {
        self.interview_shown = false;
        self.interview_pending = false;
        self.turns = 0;
        self.interview_cycles_completed = 0;
        self.last_interview_cancelled = false;
        self.entry_source = Some(entry_source);
        self.budget_exhausted = false;
        self.recovery_exhausted = false;
        self.interview_denied = false;
        self.plan_synthesis_retry_used = false;
        self.plan_validation_repair_reprompts = 0;
        self.bounded_planning_follow_ups_pending = 0;
        self.pseudo_tool_call_reprompts = 0;
        self.previous_primary_agent = None;
        self.fallback_primary_agent = None;
        self.pending_approval = None;
        self.plan_entry_agent_switch_pending = false;
    }

    pub(crate) fn exit(&mut self) {
        self.entry_source = None;
        self.budget_exhausted = false;
        self.recovery_exhausted = false;
        self.interview_denied = false;
        self.plan_synthesis_retry_used = false;
        self.plan_validation_repair_reprompts = 0;
        self.bounded_planning_follow_ups_pending = 0;
        self.pseudo_tool_call_reprompts = 0;
        self.previous_primary_agent = None;
        self.fallback_primary_agent = None;
        self.pending_approval = None;
        self.plan_entry_agent_switch_pending = false;
    }

    /// Leave Planning after the artifact/tracker handoff while retaining the
    /// approval identity until the caller emits its resolved event.
    pub(crate) fn exit_preserving_pending_approval(&mut self) {
        let pending_approval = self.pending_approval.take();
        self.exit();
        self.pending_approval = pending_approval;
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    #[cfg(test)]
    pub(crate) fn interview_shown(&self) -> bool {
        self.interview_shown
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn mark_interview_shown(&mut self) {
        self.interview_shown = true;
        self.interview_pending = false;
    }

    pub(crate) fn turns(&self) -> usize {
        self.turns
    }

    pub(crate) fn increment_turns(&mut self) {
        self.turns = self.turns.saturating_add(1);
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn interview_pending(&self) -> bool {
        self.interview_pending
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn mark_interview_pending(&mut self) {
        self.interview_pending = true;
    }

    pub(crate) fn clear_interview_pending(&mut self) {
        self.interview_pending = false;
    }

    pub(crate) fn record_interview_result(&mut self, answered_questions: usize, cancelled: bool) {
        let answered_questions = answered_questions.min(3);
        self.last_interview_cancelled = cancelled || answered_questions == 0;
        self.interview_pending = false;

        if !self.last_interview_cancelled {
            self.interview_cycles_completed = self.interview_cycles_completed.saturating_add(1);
            self.interview_shown = true;
        } else {
            self.interview_shown = false;
        }
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn interview_cycles_completed(&self) -> usize {
        self.interview_cycles_completed
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub(crate) fn last_interview_cancelled(&self) -> bool {
        self.last_interview_cancelled
    }

    pub(crate) fn mark_budget_exhausted(&mut self) {
        self.budget_exhausted = true;
    }

    pub(crate) fn is_budget_exhausted(&self) -> bool {
        self.budget_exhausted
    }

    pub(crate) fn mark_recovery_exhausted(&mut self) {
        self.recovery_exhausted = true;
    }

    pub(crate) fn is_recovery_exhausted(&self) -> bool {
        self.recovery_exhausted
    }

    /// Record that `request_user_input` was denied by a permanent
    /// capability/policy failure this session. Once set, the interview must
    /// never be re-forced — see the field doc comment for why this differs
    /// from `record_interview_result(0, cancelled=true)`.
    pub(crate) fn mark_interview_denied(&mut self) {
        self.interview_denied = true;
        self.interview_pending = false;
    }

    pub(crate) fn is_interview_denied(&self) -> bool {
        self.interview_denied
    }

    pub(crate) fn plan_synthesis_retry_allowed(&self) -> bool {
        self.interview_denied && !self.plan_synthesis_retry_used && !self.budget_exhausted && !self.recovery_exhausted
    }

    pub(crate) fn mark_plan_synthesis_retry_used(&mut self) {
        self.plan_synthesis_retry_used = true;
        self.interview_pending = false;
    }

    /// Reset the automatic validation-repair budget for a fresh planning turn.
    pub(crate) fn start_turn(&mut self) {
        self.plan_validation_repair_reprompts = 0;
        self.bounded_planning_follow_ups_pending = 0;
    }

    pub(crate) fn plan_validation_repair_allowed(&self) -> bool {
        self.plan_validation_repair_reprompts < MAX_PLAN_VALIDATION_REPAIR_REPROMPTS
    }

    /// Generic bounded-planning-retry allowance shared by validation repair,
    /// denied-interview synthesis retries, and pseudo-tool-call reprompts.
    /// Each queues exactly one extra request through the text-response cap;
    /// the underlying retry budget (validation max 2, synthesis once,
    /// pseudo max 2) is enforced separately by the caller.
    pub(crate) fn bounded_planning_follow_up_allowed(&self) -> bool {
        self.bounded_planning_follow_ups_pending > 0
    }

    pub(crate) fn consume_bounded_planning_follow_up(&mut self) {
        self.bounded_planning_follow_ups_pending = self.bounded_planning_follow_ups_pending.saturating_sub(1);
    }

    /// Queue one cap-bypassing follow-up without consuming validation budget.
    /// Use when a bounded non-validation retry (denied interview, pseudo
    /// tool-call markup) schedules a `Continue`; the retry's own
    /// used/allowed counters remain responsible for bounding the loop.
    pub(crate) fn queue_bounded_planning_follow_up(&mut self) {
        self.bounded_planning_follow_ups_pending = self.bounded_planning_follow_ups_pending.saturating_add(1);
    }

    pub(crate) fn mark_plan_validation_repair_used(&mut self) {
        self.plan_validation_repair_reprompts = self.plan_validation_repair_reprompts.saturating_add(1);
        self.bounded_planning_follow_ups_pending = self.bounded_planning_follow_ups_pending.saturating_add(1);
    }

    pub(crate) fn plan_pseudo_tool_call_reprompt_allowed(&self) -> bool {
        self.pseudo_tool_call_reprompts < MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS
    }

    pub(crate) fn mark_plan_pseudo_tool_call_reprompt_used(&mut self) {
        self.pseudo_tool_call_reprompts = self.pseudo_tool_call_reprompts.saturating_add(1);
    }

    pub(crate) fn interview_forcing_allowed(&self) -> bool {
        !self.is_budget_exhausted() && !self.is_recovery_exhausted() && !self.is_interview_denied()
    }

    /// Queue a full switch to the plan primary agent for after the current
    /// turn completes. Mid-turn entry must keep the turn alive so research can
    /// continue; `ToolPipelineOutcome::pending_primary_agent` would become
    /// `SwitchPrimaryAgent` and break the turn before any research runs.
    pub(crate) fn queue_plan_entry_agent_switch(&mut self) {
        self.plan_entry_agent_switch_pending = true;
    }

    /// Take the deferred plan-entry agent switch. Returns true once per
    /// queued entry so the turn-loop can attach it to `TurnLoopOutcome`
    /// after final-response validation.
    pub(crate) fn take_plan_entry_agent_switch(&mut self) -> bool {
        std::mem::take(&mut self.plan_entry_agent_switch_pending)
    }

    pub(crate) fn set_previous_primary_agent(&mut self, agent: Option<String>) {
        self.previous_primary_agent = agent.filter(|name| !name.trim().is_empty());
    }

    pub(crate) fn previous_primary_agent(&self) -> Option<&str> {
        self.previous_primary_agent.as_deref()
    }

    pub(crate) fn set_fallback_primary_agent(&mut self, agent: Option<String>) {
        self.fallback_primary_agent = agent.filter(|name| !name.trim().is_empty());
    }

    pub(crate) fn fallback_primary_agent(&self) -> Option<&str> {
        self.fallback_primary_agent.as_deref()
    }

    /// Execution agent to restore when planning is cancelled without an
    /// approved-plan handoff. Prefers the agent that was active before
    /// planning began, then the configured default. The plan agent itself is
    /// never a restore target: `/plan off` must land on an execution agent,
    /// not hand back the read-only planner (reachable when planning was
    /// toggled while the plan agent was already active).
    pub(crate) fn restore_agent_after_planning(&self) -> Option<&str> {
        let not_plan = |name: &str| !name.eq_ignore_ascii_case(PLAN_PRIMARY_AGENT_NAME);
        self.previous_primary_agent()
            .filter(|name| not_plan(name))
            .or_else(|| self.fallback_primary_agent().filter(|name| not_plan(name)))
    }

    pub(crate) fn mark_plan_approval_pending(&mut self, thread_id: String, turn_id: String) {
        self.pending_approval = Some(PendingPlanApproval { thread_id, turn_id });
    }

    pub(crate) fn take_pending_plan_approval(&mut self) -> Option<PendingPlanApproval> {
        self.pending_approval.take()
    }
}

pub(crate) const PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT: &str = "Planning workflow is active. Continue refining the plan; approval controls appear only after a validated draft is persisted.";
pub(crate) const PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT: &str = "Planning workflow: type `implement` (or `yes`/`continue`/`go`/`start`) to execute, or say `keep planning` to revise.";
pub(crate) const PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT: &str =
    "Planning workflow remains active: no approval-ready plan was produced. Keep planning and describe what to revise.";

/// Confirmation verb line for planning stop paths (2-line diagnostic contract).
pub(crate) fn short_confirmation_hint() -> &'static str {
    PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT
}

pub(crate) fn render_planning_workflow_next_step_hint(renderer: &mut AnsiRenderer) -> Result<()> {
    // Strict 2-line cap: status + one action (R1).
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT)?;
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT)?;
    Ok(())
}

/// Promote to the Planning stage without emitting a transcript row.
///
/// Mid-turn `start_planning` entry uses this: the turn-loop Build→Plan
/// auto-switch detector owns the single researching row (rendered after the
/// `• Start planning` summary). Emitting here would duplicate it (❋, •, ❋).
/// If the turn breaks before the detector runs, the footer shows `Planning…`
/// with no row; the next planning turn re-renders at turn start (self-heals).
pub(crate) fn promote_planning_stage(handle: &InlineHandle) {
    handle.set_activity_state(ActivityState::Planning);
    handle.force_redraw();
}

/// Promote to the Planning stage and render the researching transcript row.
///
/// Called once per planning turn when work is live (turn start, mid-turn
/// planning entry via the turn-loop detector). Mode entry alone stays `Idle`
/// so the footer never shows `Planning...` before the user has typed a request.
pub(crate) fn mark_planning_turn_started(renderer: &mut AnsiRenderer, handle: &InlineHandle) {
    promote_planning_stage(handle);
    if let Err(err) = crate::agent::runloop::unified::tool_summary::render_planning_progress_indicator(
        renderer,
        crate::agent::runloop::unified::tool_summary::PLANNING_RESEARCHING_INDICATOR,
    ) {
        tracing::warn!("failed to render planning progress indicator: {}", err);
    }
}

/// Canonical plan-agent display identity used when planning entry cannot
/// mutate `ActivePrimaryAgentState` yet (mid-turn `start_planning`). Matches
/// the built-in plan primary agent name and color so the header badge agrees
/// with the post-turn modes handoff.
pub(crate) const PLAN_PRIMARY_AGENT_NAME: &str = "plan";

/// Refresh the session header badge to the plan agent so the user sees Plan
/// mode as soon as planning is confirmed, not only after the turn ends.
pub(crate) fn apply_plan_agent_header(handle: &InlineHandle) {
    let color = vtcode_config::constants::ui::AGENT_COLOR_PLAN.to_string();
    handle.set_primary_agent(Some(PLAN_PRIMARY_AGENT_NAME.to_string()), Some(color));
}

/// Refresh the session header badge to an arbitrary primary agent. Used for
/// execution restore after planning and for selected plan agents that carry a
/// custom display name.
pub(crate) fn apply_agent_header(handle: &InlineHandle, name: &str, color: Option<String>) {
    let color = color.filter(|c| !c.trim().is_empty());
    handle.set_primary_agent(Some(name.to_string()), color);
}

/// Header display name to apply when planning ends. Prefer the restore target
/// when present; otherwise fall back to the active agent so a Plan badge cannot
/// stick after a no-op restore.
pub(crate) fn plan_exit_header_name<'a>(restore: Option<&'a str>, active_display: &'a str) -> &'a str {
    restore.map(str::trim).filter(|name| !name.is_empty()).unwrap_or(active_display)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanningFinishReason {
    Approved,
    Cancelled,
}

pub(crate) async fn transition_to_planning_workflow(
    tool_registry: &ToolRegistry,
    session_stats: &mut SessionStats,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    entry_source: PlanningEntrySource,
    previous_primary_agent: Option<String>,
    fallback_primary_agent: Option<String>,
    reset_plan_file: bool,
    reset_plan_baseline: bool,
) {
    tool_registry.enable_planning();
    tool_registry.apply_planning_mode_policy_overrides().await;
    let plan_state = tool_registry.planning_workflow_state();
    // `enable_planning()` above already sets the active flag on
    // `PlanningWorkflowState` (the single source of truth), so we do not call
    // `plan_state.enable()` again here.
    if reset_plan_file {
        plan_state.set_plan_file(None).await;
    }
    if reset_plan_baseline {
        plan_state.set_plan_baseline(None).await;
    }

    session_stats.reset_for_planning_workflow_entry();
    plan_session.enter(entry_source);
    plan_session.set_previous_primary_agent(previous_primary_agent);
    plan_session.set_fallback_primary_agent(fallback_primary_agent);
    // Stay Idle until the first planning turn actually starts. Setting
    // Planning here would show "Planning..." in the footer before the user
    // has typed anything, and the researching transcript row below would
    // claim research started with no request. `run_turn_loop` promotes to
    // the Planning stage and renders the researching indicator once per
    // planning turn; mid-turn entry (`start_planning`) promotes explicitly
    // because its turn is already running.
    handle.set_activity_state(ActivityState::Idle);
    handle.force_redraw();
}

pub(crate) async fn finish_planning_workflow(
    tool_registry: &ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    reason: PlanningFinishReason,
) -> Result<Option<crate::agent::runloop::unified::planning_workflow::TaskTrackerHandoff>> {
    let tracker = if reason == PlanningFinishReason::Approved {
        Some(
            crate::agent::runloop::unified::planning_workflow::create_task_tracker_from_active_plan(
                tool_registry,
                handle,
            )
            .await?,
        )
    } else {
        None
    };
    tool_registry.disable_planning();
    tool_registry.restore_post_planning_policies().await;
    let plan_state = tool_registry.planning_workflow_state();
    plan_state.disable();
    if reason == PlanningFinishReason::Cancelled {
        plan_state.set_plan_file(None).await;
    }

    if reason == PlanningFinishReason::Approved {
        plan_session.exit_preserving_pending_approval();
    } else {
        plan_session.exit();
    }
    handle.set_activity_state(ActivityState::Idle);
    handle.force_redraw();
    Ok(tracker)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS, MAX_PLAN_VALIDATION_REPAIR_REPROMPTS, PlanningWorkflowSessionState,
    };
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    #[test]
    fn interview_result_updates_cycle_metrics() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.record_interview_result(2, false);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(!state.last_interview_cancelled());

        state.record_interview_result(0, true);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(state.last_interview_cancelled());
    }

    #[test]
    fn entering_resets_interview_cycle_metrics() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.record_interview_result(1, false);
        assert_eq!(state.interview_cycles_completed(), 1);

        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert_eq!(state.interview_cycles_completed(), 0);
        assert!(!state.last_interview_cancelled());
    }

    #[test]
    fn restore_agent_after_planning_prefers_previous_then_fallback() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::AgentSuggestion);
        state.set_previous_primary_agent(Some("build".to_string()));
        state.set_fallback_primary_agent(Some("auto".to_string()));
        assert_eq!(state.restore_agent_after_planning(), Some("build"));

        state.set_previous_primary_agent(None);
        assert_eq!(state.restore_agent_after_planning(), Some("auto"));

        state.set_fallback_primary_agent(None);
        assert_eq!(state.restore_agent_after_planning(), None);
    }

    #[test]
    fn restore_agent_after_planning_never_returns_plan_agent() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        // `/plan on` while the plan agent was already active records "plan" as
        // previous; restoring must fall through to the execution fallback
        // instead of re-selecting the read-only planner.
        state.set_previous_primary_agent(Some("plan".to_string()));
        state.set_fallback_primary_agent(Some("build".to_string()));
        assert_eq!(state.restore_agent_after_planning(), Some("build"));

        state.set_fallback_primary_agent(Some("plan".to_string()));
        assert_eq!(state.restore_agent_after_planning(), None);
    }

    #[test]
    fn exit_clears_restore_agent_targets() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.set_previous_primary_agent(Some("build".to_string()));
        state.set_fallback_primary_agent(Some("auto".to_string()));

        state.exit();
        assert_eq!(state.restore_agent_after_planning(), None);
    }

    #[test]
    fn plan_exit_header_name_falls_back_to_active_display() {
        use super::plan_exit_header_name;

        assert_eq!(plan_exit_header_name(Some("build"), "auto"), "build");
        assert_eq!(plan_exit_header_name(Some("  "), "auto"), "auto");
        assert_eq!(plan_exit_header_name(None, "build"), "build");
    }

    #[test]
    fn plan_entry_agent_switch_is_deferred_and_consumed_once() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::AgentSuggestion);
        assert!(!state.take_plan_entry_agent_switch());

        // Mid-turn start_planning queues the switch without ending the turn.
        state.queue_plan_entry_agent_switch();
        assert!(state.take_plan_entry_agent_switch());
        // Consumed once: a second take must not re-trigger SwitchPrimaryAgent.
        assert!(!state.take_plan_entry_agent_switch());
    }

    #[test]
    fn plan_entry_agent_switch_cleared_on_enter_and_exit() {
        let mut state = PlanningWorkflowSessionState::default();
        state.queue_plan_entry_agent_switch();
        state.enter(PlanningEntrySource::AgentSuggestion);
        assert!(!state.take_plan_entry_agent_switch(), "enter must clear a stale deferred switch");

        state.queue_plan_entry_agent_switch();
        state.exit();
        assert!(!state.take_plan_entry_agent_switch(), "exit must clear the deferred plan-agent switch");
    }

    #[test]
    fn take_plan_entry_agent_switch_always_consumes_even_when_not_applied() {
        // Stronger handoffs discard the deferred switch at the turn boundary,
        // but the take itself must still clear the flag so a later turn cannot
        // fire a stale plan-agent switch mid-implementation.
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::AgentSuggestion);
        state.queue_plan_entry_agent_switch();
        assert!(state.take_plan_entry_agent_switch());
        assert!(!state.take_plan_entry_agent_switch());
    }

    #[test]
    fn mark_interview_denied_is_permanent_until_reset() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_interview_denied());

        state.mark_interview_pending();
        state.mark_interview_denied();
        assert!(state.is_interview_denied());
        // A denial also clears any pending interview request — re-forcing it
        // would just repeat the same policy failure.
        assert!(!state.interview_pending());

        // Re-entering the planning workflow (a fresh session) clears the flag.
        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_interview_denied());
    }

    #[test]
    fn interview_denial_allows_one_plan_synthesis_retry() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.mark_interview_denied();

        assert!(state.plan_synthesis_retry_allowed());
        state.mark_plan_synthesis_retry_used();
        assert!(!state.plan_synthesis_retry_allowed());

        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_interview_denied());
        assert!(!state.plan_synthesis_retry_allowed());
    }

    #[test]
    fn pseudo_tool_call_reprompts_are_bounded_and_reset_by_enter() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        for attempt in 0..MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
            assert!(
                state.plan_pseudo_tool_call_reprompt_allowed(),
                "reprompt attempt {attempt} should be allowed before the bound is reached"
            );
            state.mark_plan_pseudo_tool_call_reprompt_used();
        }
        assert!(
            !state.plan_pseudo_tool_call_reprompt_allowed(),
            "reprompts must stop once the bound is exhausted so a checkpoint that keeps emitting markup cannot loop the turn"
        );

        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(state.plan_pseudo_tool_call_reprompt_allowed());
    }

    #[test]
    fn plan_validation_repair_is_bounded_and_reset_at_turn_and_reentry() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        for attempt in 0..MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
            assert!(
                state.plan_validation_repair_allowed(),
                "repair attempt {attempt} should be allowed before the bound is reached"
            );
            state.mark_plan_validation_repair_used();
        }
        assert!(
            !state.plan_validation_repair_allowed(),
            "validation repairs must stop after the bounded automatic passes"
        );
        assert!(state.bounded_planning_follow_up_allowed());
        state.consume_bounded_planning_follow_up();
        assert!(state.bounded_planning_follow_up_allowed());
        state.consume_bounded_planning_follow_up();
        assert!(!state.bounded_planning_follow_up_allowed());

        state.start_turn();
        assert!(state.plan_validation_repair_allowed(), "a fresh planning turn gets a fresh repair budget");
        assert!(!state.bounded_planning_follow_up_allowed(), "a fresh turn has no stale repair request");
        state.mark_plan_validation_repair_used();
        assert!(
            state.bounded_planning_follow_up_allowed(),
            "a repair remains pending regardless of earlier text responses"
        );

        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(state.plan_validation_repair_allowed());
        assert!(!state.bounded_planning_follow_up_allowed(), "re-entry clears pending repair requests");
    }

    #[test]
    fn budget_and_recovery_exhaustion_cleared_by_enter_and_exit() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.mark_budget_exhausted();
        state.mark_recovery_exhausted();
        assert!(state.is_budget_exhausted());
        assert!(state.is_recovery_exhausted());

        // exit() clears both exhaustion flags.
        state.exit();
        assert!(!state.is_budget_exhausted());
        assert!(!state.is_recovery_exhausted());

        // Re-apply and verify enter() also clears them.
        state.mark_budget_exhausted();
        state.mark_recovery_exhausted();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_budget_exhausted());
        assert!(!state.is_recovery_exhausted());
    }

    #[test]
    fn record_interview_result_treats_zero_answered_as_cancelled() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        // answered_questions=0 with cancelled=false should still count as cancelled.
        state.record_interview_result(0, false);
        assert!(state.last_interview_cancelled());
        assert_eq!(state.interview_cycles_completed(), 0);
    }

    #[test]
    fn record_interview_result_clamps_answered_questions_to_three() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.record_interview_result(10, false);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(!state.last_interview_cancelled());
    }

    #[test]
    fn pending_approval_identity_is_consumed_once() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.mark_plan_approval_pending("thread-1".to_string(), "turn-2".to_string());

        assert_eq!(
            state.take_pending_plan_approval(),
            Some(super::PendingPlanApproval {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-2".to_string(),
            })
        );
        assert_eq!(state.take_pending_plan_approval(), None);
    }

    #[test]
    fn approved_exit_preserves_pending_identity_until_resolution() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.mark_plan_approval_pending("thread-1".to_string(), "turn-2".to_string());

        state.exit_preserving_pending_approval();

        assert_eq!(
            state.take_pending_plan_approval(),
            Some(super::PendingPlanApproval {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-2".to_string(),
            })
        );
        assert_eq!(state.take_pending_plan_approval(), None);
    }

    #[test]
    fn bounded_retry_queue_bypasses_cap_without_consuming_validation_budget() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.bounded_planning_follow_up_allowed());

        // A denied-interview or pseudo-tool-call retry queues one allowance
        // without touching the validation-repair budget.
        state.queue_bounded_planning_follow_up();
        assert!(state.bounded_planning_follow_up_allowed());
        assert!(state.plan_validation_repair_allowed());

        state.consume_bounded_planning_follow_up();
        assert!(!state.bounded_planning_follow_up_allowed());
        assert!(state.plan_validation_repair_allowed());
    }

    #[test]
    fn bounded_retry_allowance_survives_prior_text_budget_use() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        // Validation repair and generic retries share the pending queue so a
        // retry scheduled after an ordinary planning response still admits
        // exactly the queued requests.
        state.mark_plan_validation_repair_used();
        state.queue_bounded_planning_follow_up();
        assert!(state.bounded_planning_follow_up_allowed());
        state.consume_bounded_planning_follow_up();
        assert!(state.bounded_planning_follow_up_allowed());
        state.consume_bounded_planning_follow_up();
        assert!(!state.bounded_planning_follow_up_allowed());

        state.start_turn();
        assert!(!state.bounded_planning_follow_up_allowed());

        state.queue_bounded_planning_follow_up();
        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.bounded_planning_follow_up_allowed());
    }

    fn drain_transcript_text(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_ui::tui::app::InlineCommand>,
    ) -> String {
        use vtcode_ui::tui::app::InlineCommand;
        std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendLine { segments, .. } => {
                    Some(segments.into_iter().map(|s| s.text).collect::<String>())
                }
                InlineCommand::Inline { segment, .. } => Some(segment.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn promote_planning_stage_emits_no_researching_row() {
        // Mid-turn `start_planning` entry must not render the researching row:
        // the turn-loop detector owns it (rendered after the `• Start planning`
        // summary). Emitting here duplicates it (❋, •, ❋).
        use vtcode_core::utils::ansi::AnsiRenderer;
        use vtcode_ui::tui::app::InlineHandle;

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let _renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        super::promote_planning_stage(&handle);

        let text = drain_transcript_text(&mut receiver);
        assert!(!text.contains("Drafting plan"), "stage promotion must not emit a transcript row, got: {text:?}");
    }

    #[test]
    fn mark_planning_turn_started_emits_single_researching_row() {
        // Turn start / detector path owns the single researching row.
        use vtcode_core::utils::ansi::AnsiRenderer;
        use vtcode_ui::tui::app::InlineHandle;

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        super::mark_planning_turn_started(&mut renderer, &handle);

        let text = drain_transcript_text(&mut receiver);
        assert_eq!(
            text.matches("Drafting plan").count(),
            1,
            "exactly one researching row per planning turn, got: {text:?}"
        );
    }
}
