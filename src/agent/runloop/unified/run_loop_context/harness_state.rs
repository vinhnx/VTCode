//! `HarnessTurnState` behavior: accounting, diagnosis, budget notices, recovery.

use super::*;

impl HarnessTurnState {
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub(crate) fn new(
        run_id: TurnRunId,
        turn_id: TurnId,
        max_tool_calls: usize,
        max_tool_wall_clock_secs: u64,
        max_tool_retries: u32,
    ) -> Self {
        Self {
            run_id,
            turn_id,
            phase: TurnPhase::Preparing,
            turn_started_at: Instant::now(),
            wait_started_at: None,
            excluded_wait_duration: Duration::ZERO,
            tool_calls: 0,
            requested_tool_calls: 0,
            admitted_tool_calls: 0,
            failed_tool_calls: 0,
            denied_tool_calls: 0,
            preflight_failures: 0,
            reused_results: 0,
            spooled_results: 0,
            raw_spooled_bytes: 0,
            model_visible_output_bytes: 0,
            model_visible_tool_preview_budget_exhausted: false,
            suppressed_tool_previews: 0,
            suppressed_tool_call_ids: HashSet::new(),
            recovery_activations: 0,
            blocked_tool_calls: 0,
            consecutive_blocked_tool_calls: 0,
            consecutive_preflight_failures: 0,
            consecutive_assistant_text_responses: 0,
            out_of_band_tool_progress: false,
            final_response_rendered: false,
            final_response_event_emitted: false,
            streamed_response_event_emitted: false,
            final_response_was_fallback: false,
            turn_refused: false,
            consecutive_spool_chunk_reads: 0,
            consecutive_same_shell_command_runs: 0,
            last_shell_command_signature: None,
            last_admitted_shell_command_signature: None,
            last_failed_shell_key: None,
            consecutive_same_file_read_family_calls: 0,
            last_file_read_family_signature: None,
            file_read_path_counts: HashMap::new(),
            claimed_patch_recovery_paths: HashSet::new(),
            seen_successful_readonly_signatures: HashSet::new(),
            streamed_tool_call_item_ids: HashMap::new(),
            failure_diagnosis_memo: HashMap::new(),
            failure_diagnosis_model_calls: 0,
            auto_permission_probe_model_calls: 0,
            stop_hook_active: false,
            seen_task_tracker_create_signatures: HashSet::new(),
            recently_written_files: HashSet::new(),
            tool_budget_warning_emitted: false,
            tool_budget_exhausted_emitted: false,
            wall_clock_exhausted_emitted: false,
            tool_budget_rejection_pending: false,
            wall_clock_directive_pending: false,
            tool_budget_directive_pending: false,
            session_limit_grant_directive_pending: false,
            session_limit_granted: false,
            pending_auto_permission_probe_warning: None,
            recovery_reason: None,
            recovery_prompt_reason: None,
            recovery_phase: RecoveryPhase::Inactive,
            recovery_mode: None,
            recovery_retry_count: 0,
            post_tool_compaction_pending: false,
            post_tool_context_capacity_failure: false,
            post_tool_context_compaction_failed: false,
            post_tool_tool_enabled_retry_used: false,
            post_tool_recovery_cycles: 0,
            recovery_rejected_synthesis: None,
            approved_plan_execution: false,
            approved_plan_recovery_retries: 0,
            interview_denial_recovery_pending: false,
            preflight_circuit_recovery_pending: false,
            blocked_tool_recovery_pending: false,
            blocked_tool_recovery_reason: None,
            blocked_tool_recovery_telemetry: None,
            max_tool_calls,
            max_tool_wall_clock: Duration::from_secs(max_tool_wall_clock_secs),
            max_tool_retries,
            consecutive_relaxed_continuations: 0,
            incomplete_tracker_items_cache: None,
        }
    }

    pub(crate) fn apply_tracker_probe(
        &mut self,
        probe: crate::agent::runloop::unified::turn::tool_outcomes::helpers::TrackerProbeOutcome,
    ) -> Option<&[String]> {
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::apply_tracker_probe_to_cache(
            &mut self.incomplete_tracker_items_cache,
            probe,
        )
    }

    pub(crate) fn has_tool_call_budget(&self) -> bool {
        self.max_tool_calls > 0
    }

    pub(crate) fn tool_budget_exhausted(&self) -> bool {
        self.has_tool_call_budget() && self.tool_calls >= self.max_tool_calls
    }

    pub(crate) fn tool_budget_exhaustion(&self) -> Option<ToolBudgetExhaustion> {
        self.tool_budget_exhausted().then_some(ToolBudgetExhaustion {
            used: self.tool_calls,
            max: self.max_tool_calls,
            remaining: self.remaining_tool_calls(),
        })
    }

    pub(crate) fn wall_clock_exhausted(&self) -> bool {
        self.effective_wall_clock_elapsed() >= self.max_tool_wall_clock
    }

    fn effective_wall_clock_elapsed(&self) -> Duration {
        let elapsed = self.turn_started_at.elapsed();
        let active_wait = self.wait_started_at.map(|started| started.elapsed()).unwrap_or(Duration::ZERO);
        elapsed.saturating_sub(self.excluded_wait_duration.saturating_add(active_wait))
    }

    /// Pause ordinary turn wall-clock accounting around an explicit external
    /// wait. This does not alter tool ceilings or cancellation behavior.
    pub(crate) fn begin_budget_excluded_wait(&mut self) {
        if self.wait_started_at.is_none() {
            self.wait_started_at = Some(Instant::now());
        }
    }

    pub(crate) fn end_budget_excluded_wait(&mut self) {
        if let Some(started) = self.wait_started_at.take() {
            self.excluded_wait_duration = self.excluded_wait_duration.saturating_add(started.elapsed());
        }
    }

    pub(crate) fn wall_clock_budget_exhaustion(&self) -> Option<ToolWallClockExhaustion> {
        self.wall_clock_exhausted()
            .then_some(ToolWallClockExhaustion { max_secs: self.max_tool_wall_clock.as_secs() })
    }

    pub(crate) fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    pub(crate) fn record_requested_tool_calls(&mut self, count: usize) {
        self.requested_tool_calls = self
            .requested_tool_calls
            .saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    pub(crate) fn record_admitted_tool_call(&mut self) {
        self.admitted_tool_calls = self.admitted_tool_calls.saturating_add(1);
    }

    pub(crate) fn admitted_tool_call_count(&self) -> u32 {
        self.admitted_tool_calls
    }

    pub(crate) fn record_failed_tool_call(&mut self) {
        self.failed_tool_calls = self.failed_tool_calls.saturating_add(1);
    }

    pub(crate) fn record_denied_tool_call(&mut self) {
        self.denied_tool_calls = self.denied_tool_calls.saturating_add(1);
    }

    pub(crate) fn record_tool_budget_rejection(&mut self) {
        self.tool_budget_rejection_pending = true;
    }

    pub(crate) fn take_tool_budget_rejection(&mut self) -> bool {
        std::mem::take(&mut self.tool_budget_rejection_pending)
    }

    pub(crate) fn record_tool_output_metrics(
        &mut self,
        reused: bool,
        spooled: bool,
        raw_spooled_bytes: u64,
        model_visible_output_bytes: usize,
    ) {
        if reused {
            self.reused_results = self.reused_results.saturating_add(1);
        }
        if spooled {
            self.spooled_results = self.spooled_results.saturating_add(1);
        }
        self.raw_spooled_bytes = self.raw_spooled_bytes.saturating_add(raw_spooled_bytes);
        self.record_model_visible_output_append(model_visible_output_bytes);
    }

    pub(crate) fn record_reused_result(&mut self) {
        self.reused_results = self.reused_results.saturating_add(1);
    }

    pub(crate) fn record_model_visible_output_append(&mut self, model_visible_output_bytes: usize) {
        self.model_visible_output_bytes = self
            .model_visible_output_bytes
            .saturating_add(u64::try_from(model_visible_output_bytes).unwrap_or(u64::MAX));
    }

    /// Return a memoized failure-diagnosis entry for `key`.
    pub(crate) fn failure_diagnosis_memo_get(&self, key: &DiagnosisMemoKey) -> Option<DiagnosisMemoEntry> {
        self.failure_diagnosis_memo.get(key).cloned()
    }

    /// Store a failure-diagnosis entry. Memo is bounded so a pathological
    /// failure storm cannot grow the turn state without limit.
    pub(crate) fn failure_diagnosis_memo_put(&mut self, key: DiagnosisMemoKey, entry: DiagnosisMemoEntry) {
        const MAX_MEMO: usize = 32;
        if self.failure_diagnosis_memo.len() >= MAX_MEMO {
            self.failure_diagnosis_memo.clear();
        }
        self.failure_diagnosis_memo.insert(key, entry);
    }

    /// Whether another model-backed diagnosis is allowed this turn.
    pub(crate) fn can_spend_failure_diagnosis_model_call(&self) -> bool {
        self.failure_diagnosis_model_calls < AUX_MODEL_CALL_BUDGET
    }

    /// Count a model-backed diagnosis attempt (success or failure).
    pub(crate) fn record_failure_diagnosis_model_call(&mut self) {
        self.failure_diagnosis_model_calls = self.failure_diagnosis_model_calls.saturating_add(1);
    }

    /// Whether another model-backed prompt-injection probe is allowed this turn.
    pub(crate) fn can_spend_auto_permission_probe_model_call(&self) -> bool {
        self.auto_permission_probe_model_calls < AUX_MODEL_CALL_BUDGET
    }

    /// Count a model-backed prompt-injection probe attempt (success or failure).
    pub(crate) fn record_auto_permission_probe_model_call(&mut self) {
        self.auto_permission_probe_model_calls = self.auto_permission_probe_model_calls.saturating_add(1);
    }

    /// Test-only execution-budget shorthand so exec-mode tests avoid
    /// repeating the `64 KiB` denominator on every call.
    #[cfg(test)]
    pub(crate) fn bound_model_visible_tool_preview(&mut self, tool_name: Option<&str>, content: String) -> String {
        self.bound_model_visible_tool_preview_with_budget(
            tool_name,
            content,
            vtcode_config::constants::output_limits::TURN_PREVIEW_BUDGET_BYTES,
        )
    }

    /// Per-result fallback for provider-history insertion. Registry outputs
    /// are already bounded; other producers still need a finite preview.
    #[cfg(test)]
    pub(crate) fn bound_model_visible_tool_preview_with_budget(
        &mut self,
        tool_name: Option<&str>,
        content: String,
        budget_bytes: usize,
    ) -> String {
        self.bound_model_visible_tool_preview_inner(None, tool_name, content, budget_bytes)
    }

    pub(crate) fn bound_model_visible_tool_preview_for_call_with_budget(
        &mut self,
        tool_call_id: &str,
        tool_name: Option<&str>,
        content: String,
        budget_bytes: usize,
    ) -> String {
        // Retain diagnostics for archived/legacy markers without letting
        // them hide new output or revoke the current tool catalog.
        self.observe_upstream_preview_budget_exhaustion(tool_call_id, &content, budget_bytes);
        self.bound_model_visible_tool_preview_inner(Some(tool_call_id), tool_name, content, budget_bytes)
    }

    fn bound_model_visible_tool_preview_inner(
        &mut self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: String,
        budget_bytes: usize,
    ) -> String {
        let limit = budget_bytes.max(1);
        if content.len() <= limit {
            return content;
        }
        self.record_suppressed_tool_preview(tool_call_id);
        let preview = bounded_tool_preview_metadata(tool_name, &content);
        if preview.len() <= limit {
            preview
        } else {
            // Pathological nested metadata must not produce a cut JSON object.
            // Keep a finite excerpt in a valid envelope even on this fallback.
            let fallback = serde_json::json!({
                "preview_truncated": true,
                "byte_count": content.len(),
                "preview": vtcode_commons::sanitizer::redact_secrets(
                    vtcode_commons::preview::condense_text_bytes(&content, limit / 16, limit / 16),
                ),
            })
            .to_string();
            if fallback.len() <= limit {
                fallback
            } else if limit >= 2 {
                "{}".to_owned()
            } else {
                "0".to_owned()
            }
        }
    }

    fn record_suppressed_tool_preview(&mut self, tool_call_id: Option<&str>) {
        let should_count = match tool_call_id {
            Some(tool_call_id) => self.suppressed_tool_call_ids.insert(tool_call_id.to_string()),
            None => true,
        };
        if should_count {
            self.suppressed_tool_previews = self.suppressed_tool_previews.saturating_add(1);
        }
    }

    /// Recognize archived exhaustion markers for diagnostic compatibility.
    /// These markers never gate calls, hide fresh results, or arm recovery.
    /// Call identity keeps response replacements from inflating diagnostics.
    pub(crate) fn observe_upstream_preview_budget_exhaustion(
        &mut self,
        tool_call_id: &str,
        content: &str,
        _budget_bytes: usize,
    ) -> bool {
        // Registry responses are already bounded. Refuse to parse an
        // oversized marker candidate here so arbitrary local/MCP output cannot
        // force a second unbounded JSON parse before the local limiter runs.
        let exhausted = content.len() <= TOOL_PREVIEW_METADATA_PARSE_LIMIT_BYTES
            && content.contains("\"preview_budget_exhausted\"")
            && serde_json::from_str::<PreviewBudgetExhaustedProbe>(content)
                .ok()
                .and_then(|probe| probe.preview_budget_exhausted)
                == Some(true);
        if !exhausted {
            return false;
        }

        self.model_visible_tool_preview_budget_exhausted = true;
        self.record_suppressed_tool_preview(Some(tool_call_id));
        true
    }

    /// Legacy exhaustion diagnostic, retained for replay compatibility only.
    /// This flag does not gate new tool calls or preview visibility.
    #[cfg(test)]
    pub(crate) fn model_visible_preview_budget_exhausted(&self) -> bool {
        self.model_visible_tool_preview_budget_exhausted
    }

    pub(crate) fn replace_model_visible_output_bytes(&mut self, previous_len: usize, new_len: usize) {
        let previous_len = u64::try_from(previous_len).unwrap_or(u64::MAX);
        self.model_visible_output_bytes = self.model_visible_output_bytes.saturating_sub(previous_len);
        self.record_model_visible_output_append(new_len);
    }

    pub(crate) fn snapshot_turn_diagnostics(
        &self,
        usage: vtcode_core::exec::events::Usage,
        low_signal_tool_calls: u32,
    ) -> vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics {
        vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics {
            usage,
            requested_tool_calls: self.requested_tool_calls,
            admitted_tool_calls: self.admitted_tool_calls,
            unadmitted_tool_calls: self.requested_tool_calls.saturating_sub(self.admitted_tool_calls),
            failed_tool_calls: self.failed_tool_calls,
            denied_tool_calls: self.denied_tool_calls,
            preflight_failures: self.preflight_failures,
            reused_results: self.reused_results,
            spooled_results: self.spooled_results,
            raw_spooled_bytes: self.raw_spooled_bytes,
            model_visible_output_bytes: self.model_visible_output_bytes,
            suppressed_tool_previews: self.suppressed_tool_previews,
            model_visible_tool_preview_budget_exhausted: self.model_visible_tool_preview_budget_exhausted,
            low_signal_tool_calls,
            recovery_activations: self.recovery_activations,
            ..Default::default()
        }
    }

    pub(crate) fn record_tool_call_with_warning(&mut self, threshold: f64) -> Option<ToolBudgetWarning> {
        self.record_tool_call();
        if !self.should_emit_tool_budget_warning(threshold) {
            return None;
        }

        let warning = ToolBudgetWarning {
            used: self.tool_calls,
            max: self.max_tool_calls,
            remaining: self.remaining_tool_calls(),
        };
        self.mark_tool_budget_warning_emitted();
        Some(warning)
    }

    pub(crate) fn record_tool_call_with_default_warning(&mut self) -> Option<ToolBudgetWarning> {
        self.record_tool_call_with_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    }

    /// Record that the agent emitted a text-only response in this turn.
    /// This state is authoritative and survives history compaction, including
    /// inline tool boundaries that are not represented in `working_history`.
    pub(crate) fn record_assistant_text_response(&mut self) -> u32 {
        self.consecutive_assistant_text_responses = self.consecutive_assistant_text_responses.saturating_add(1);
        self.consecutive_assistant_text_responses
    }

    /// Break the text-only response streak after a tool call passes admission.
    /// Blocked and malformed attempts are not progress and retain the streak;
    /// their dedicated safeguards remain responsible for those failure loops.
    pub(crate) fn reset_assistant_text_response_streak(&mut self) {
        self.consecutive_assistant_text_responses = 0;
    }

    /// Record productive tool execution that is not represented by a normal
    /// tool-result message, such as an inline Copilot runtime call.
    pub(crate) fn record_out_of_band_tool_progress(&mut self) {
        self.out_of_band_tool_progress = true;
        self.reset_assistant_text_response_streak();
    }

    pub(crate) fn record_out_of_band_tool_call(&mut self) {
        self.record_requested_tool_calls(1);
        self.record_admitted_tool_call();
        self.record_out_of_band_tool_progress();
    }

    pub(crate) fn has_out_of_band_tool_progress(&self) -> bool {
        self.out_of_band_tool_progress
    }

    pub(crate) fn record_tool_budget_exhaustion_notice(&mut self) -> Option<ToolBudgetExhaustionNotice> {
        let exhaustion = self.tool_budget_exhaustion()?;
        let first_notice = !self.tool_budget_exhausted_emitted;
        if first_notice {
            self.mark_tool_budget_exhausted_emitted();
            self.tool_budget_directive_pending = true;
        }
        Some(ToolBudgetExhaustionNotice { exhaustion, first_notice })
    }

    /// Consume the pending tool-call-budget synthesis-directive flag. Returns
    /// `true` exactly once per turn (after the batch where exhaustion first
    /// fired). Mirrors `take_wall_clock_directive_pending`.
    pub(crate) fn take_tool_budget_directive_pending(&mut self) -> bool {
        std::mem::take(&mut self.tool_budget_directive_pending)
    }

    /// Record a user-approved session limit increase for the current turn.
    pub(crate) fn record_session_limit_grant(&mut self) {
        self.session_limit_granted = true;
        self.session_limit_grant_directive_pending = true;
    }

    pub(crate) fn has_session_limit_grant(&self) -> bool {
        self.session_limit_granted
    }

    /// Consume the pending model-facing session-limit guidance after the tool
    /// batch has appended all of its responses.
    pub(crate) fn take_session_limit_grant_directive_pending(&mut self) -> bool {
        std::mem::take(&mut self.session_limit_grant_directive_pending)
    }

    /// Record a wall-clock-budget rejection for the current tool call.
    ///
    /// Returns `None` when the budget is not exhausted. On the first exhausted
    /// call it flags `first_notice` (so the full policy message is emitted once)
    /// and arms `wall_clock_directive_pending` so the handler pushes a single
    /// "synthesize now" system directive *after* the tool batch completes.
    pub(crate) fn record_wall_clock_exhaustion_notice(&mut self) -> Option<ToolWallClockExhaustionNotice> {
        let exhaustion = self.wall_clock_budget_exhaustion()?;
        let first_notice = !self.wall_clock_exhausted_emitted;
        if first_notice {
            self.wall_clock_exhausted_emitted = true;
            self.wall_clock_directive_pending = true;
        }
        Some(ToolWallClockExhaustionNotice { exhaustion, first_notice })
    }

    /// Consume the pending wall-clock synthesis-directive flag. Returns `true`
    /// exactly once per turn (after the batch where exhaustion first fired).
    pub(crate) fn take_wall_clock_directive_pending(&mut self) -> bool {
        std::mem::take(&mut self.wall_clock_directive_pending)
    }

    pub(crate) fn record_blocked_tool_call(&mut self) -> usize {
        self.blocked_tool_calls = self.blocked_tool_calls.saturating_add(1);
        self.record_blocked_tool_call_streak()
    }

    /// Path-local rejections must still stop uninterrupted retry loops.
    pub(crate) fn record_blocked_tool_call_streak(&mut self) -> usize {
        self.consecutive_blocked_tool_calls = self.consecutive_blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls
    }

    pub(crate) fn reset_blocked_tool_call_streak(&mut self) {
        self.consecutive_blocked_tool_calls = 0;
    }

    pub(crate) fn record_preflight_failure(&mut self) -> usize {
        self.consecutive_preflight_failures = self.consecutive_preflight_failures.saturating_add(1);
        self.preflight_failures = self.preflight_failures.saturating_add(1);
        self.consecutive_preflight_failures
    }

    pub(crate) fn reset_preflight_failure_streak(&mut self) {
        self.consecutive_preflight_failures = 0;
    }

    pub(crate) fn tool_budget_usage_ratio(&self) -> f64 {
        if !self.has_tool_call_budget() {
            0.0
        } else {
            self.tool_calls as f64 / self.max_tool_calls as f64
        }
    }

    pub(crate) fn remaining_tool_calls(&self) -> usize {
        self.max_tool_calls.saturating_sub(self.tool_calls)
    }

    pub(crate) fn should_emit_tool_budget_warning(&self, threshold: f64) -> bool {
        self.has_tool_call_budget() && !self.tool_budget_warning_emitted && self.tool_budget_usage_ratio() >= threshold
    }

    pub(crate) fn mark_tool_budget_warning_emitted(&mut self) {
        self.tool_budget_warning_emitted = true;
    }

    pub(crate) fn mark_tool_budget_exhausted_emitted(&mut self) {
        self.tool_budget_exhausted_emitted = true;
    }

    pub(crate) fn activate_recovery(&mut self, reason: impl Into<String>) -> bool {
        self.activate_recovery_with_mode(reason, RecoveryMode::ToolFreeSynthesis)
    }

    /// Arm a recovery pass. Returns `false` when a pass is already armed or in
    /// flight (`Pending`/`InPass`/`Completed`), so callers can skip the
    /// user-facing "scheduling" feedback instead of claiming a pass they did
    /// not schedule.
    pub(crate) fn activate_recovery_with_mode(&mut self, reason: impl Into<String>, mode: RecoveryMode) -> bool {
        if matches!(self.recovery_phase, RecoveryPhase::Inactive) {
            self.recovery_activations = self.recovery_activations.saturating_add(1);
            self.recovery_reason = Some(reason.into());
            self.recovery_prompt_reason = self.recovery_reason.clone();
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_mode = Some(mode);
            self.recovery_retry_count = 0;
            return true;
        }
        false
    }

    /// Arm the single tool-enabled retry used after a provider failure follows
    /// successful tool execution. The runloop consumes the
    /// compaction flag before consuming the recovery pass, so the retry sees
    /// the compacted prefix plus the current request and tool outputs.
    pub(crate) fn arm_post_tool_tool_enabled_retry(
        &mut self,
        reason: impl Into<String>,
        context_capacity_failure: bool,
    ) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::Inactive) {
            return false;
        }

        self.recovery_activations = self.recovery_activations.saturating_add(1);
        self.recovery_reason = Some(reason.into());
        self.recovery_prompt_reason = self.recovery_reason.clone();
        self.recovery_phase = RecoveryPhase::Pending;
        self.recovery_mode = Some(RecoveryMode::ToolEnabledRetry);
        self.recovery_retry_count = 0;
        self.post_tool_compaction_pending = true;
        self.post_tool_context_capacity_failure = context_capacity_failure;
        self.post_tool_tool_enabled_retry_used = true;
        true
    }

    pub(crate) fn is_recovery_active(&self) -> bool {
        matches!(self.recovery_phase, RecoveryPhase::Pending | RecoveryPhase::InPass)
    }

    #[cfg(test)]
    pub(crate) fn recovery_reason(&self) -> Option<&str> {
        self.recovery_reason.as_deref()
    }

    /// Reason frozen into the `[Recovery Mode]` prompt block. Stable for the
    /// whole recovery activation so consecutive recovery turns keep an
    /// identical system-prompt prefix.
    pub(crate) fn recovery_prompt_reason(&self) -> Option<&str> {
        self.recovery_prompt_reason.as_deref().or(self.recovery_reason.as_deref())
    }

    pub(crate) fn recovery_pass_used(&self) -> bool {
        matches!(self.recovery_phase, RecoveryPhase::InPass | RecoveryPhase::Completed)
    }

    #[cfg(test)]
    pub(super) fn recovery_mode(&self) -> Option<RecoveryMode> {
        self.recovery_mode
    }

    /// Switch to tool-free synthesis mode and reset the recovery phase back to
    /// `Pending` so the next loop iteration can consume it.
    ///
    /// Unlike `activate_recovery_with_mode` (which is a guarded no-op once a
    /// pass is in flight), this unconditionally forces the phase to `Pending`,
    /// covering `Inactive`, `InPass`, and `Completed`. This is required
    /// because the post-tool follow-up failure path runs from a *non-recovery*
    /// turn (phase == `Inactive`): `activate_recovery_with_mode` would set the
    /// reason and mode but leave the phase as `Inactive`, so
    /// `consume_recovery_pass()` would return `false`, `tool_free_recovery`
    /// would evaluate to `false`, and tools would never be disabled at the API
    /// level.
    ///
    /// When transitioning from `Inactive`, this also resets the retry counter
    /// and seeds a default `recovery_reason` (mirroring
    /// `activate_recovery_with_mode`) so the `[Recovery Mode]` request block
    /// reports why recovery was engaged.
    ///
    /// Returns `true` when the phase actually changed.
    pub(crate) fn switch_to_tool_free_recovery(&mut self) -> bool {
        let was_inactive = matches!(self.recovery_phase, RecoveryPhase::Inactive);
        self.recovery_mode = Some(RecoveryMode::ToolFreeSynthesis);
        let changed = !matches!(self.recovery_phase, RecoveryPhase::Pending);
        self.recovery_phase = RecoveryPhase::Pending;
        if was_inactive {
            self.recovery_activations = self.recovery_activations.saturating_add(1);
            self.recovery_retry_count = 0;
            if self.recovery_reason.is_none() {
                self.recovery_reason = Some("post-tool follow-up failure".to_string());
            }
            self.recovery_prompt_reason = self.recovery_reason.clone();
        } else if self.recovery_prompt_reason.is_none() {
            self.recovery_prompt_reason = self.recovery_reason.clone();
        }
        changed
    }

    /// Arm the bounded tool-free plan synthesis fallback used when
    /// `request_user_input` is permanently unavailable in the current
    /// runtime. The directive is flushed after the current tool batch so
    /// provider message ordering remains valid.
    pub(crate) fn arm_interview_denial_recovery(&mut self) {
        self.interview_denial_recovery_pending = true;
    }

    pub(crate) fn take_interview_denial_recovery(&mut self) -> bool {
        std::mem::take(&mut self.interview_denial_recovery_pending)
    }

    pub(crate) fn interview_denial_recovery_pending(&self) -> bool {
        self.interview_denial_recovery_pending
    }

    /// Arm the preflight circuit-breaker recovery so the tool batch can flush
    /// its synthesis directive after all tool responses land.
    pub(crate) fn arm_preflight_circuit_recovery(&mut self) {
        self.preflight_circuit_recovery_pending = true;
    }

    pub(crate) fn take_preflight_circuit_recovery(&mut self) -> bool {
        std::mem::take(&mut self.preflight_circuit_recovery_pending)
    }

    /// Arm the bounded tool-free recovery used after repeated blocked calls.
    /// The response batch consumes this flag after appending every required
    /// tool response, preserving provider message ordering. The telemetry
    /// snapshot is kept for the `TurnBlockedEvent` emitted at turn finalize.
    pub(crate) fn arm_blocked_tool_recovery(
        &mut self,
        reason: impl Into<String>,
        telemetry: BlockedToolRecoveryTelemetry,
    ) {
        self.blocked_tool_recovery_pending = true;
        self.blocked_tool_recovery_reason = Some(reason.into());
        self.blocked_tool_recovery_telemetry = Some(telemetry);
    }

    /// Record blocked-call telemetry without arming recovery. Used when the
    /// fuse hard-breaks the turn in recovery mode: no recovery pass is
    /// scheduled, but `finalize_turn` still needs the values for
    /// `TurnBlockedEvent`.
    pub(crate) fn record_blocked_tool_recovery_telemetry(&mut self, telemetry: BlockedToolRecoveryTelemetry) {
        self.blocked_tool_recovery_telemetry = Some(telemetry);
    }

    /// One-shot accessor for the blocked-call telemetry captured at fuse-trip
    /// time; consumed by `finalize_turn`.
    pub(crate) fn take_blocked_tool_recovery_telemetry(&mut self) -> Option<BlockedToolRecoveryTelemetry> {
        self.blocked_tool_recovery_telemetry.take()
    }

    pub(crate) fn take_blocked_tool_recovery(&mut self) -> bool {
        std::mem::take(&mut self.blocked_tool_recovery_pending)
    }

    pub(crate) fn blocked_tool_recovery_pending(&self) -> bool {
        self.blocked_tool_recovery_pending
    }

    pub(crate) fn take_blocked_tool_recovery_reason(&mut self) -> Option<String> {
        self.blocked_tool_recovery_reason.take()
    }

    pub(crate) fn recovery_is_tool_free(&self) -> bool {
        matches!(self.recovery_mode, Some(RecoveryMode::ToolFreeSynthesis))
    }

    #[cfg(test)]
    pub(crate) fn post_tool_compaction_pending(&self) -> bool {
        self.post_tool_compaction_pending
    }

    pub(crate) fn take_post_tool_compaction_pending(&mut self) -> bool {
        std::mem::take(&mut self.post_tool_compaction_pending)
    }

    pub(crate) fn post_tool_context_capacity_failure(&self) -> bool {
        self.post_tool_context_capacity_failure
    }

    pub(crate) fn mark_post_tool_context_compaction_failed(&mut self) {
        self.post_tool_context_compaction_failed = true;
    }

    pub(crate) fn post_tool_context_compaction_failed(&self) -> bool {
        self.post_tool_context_compaction_failed
    }

    pub(crate) fn post_tool_tool_enabled_retry_used(&self) -> bool {
        self.post_tool_tool_enabled_retry_used
    }

    pub(crate) fn set_approved_plan_execution(&mut self, active: bool) {
        self.approved_plan_execution = active;
        self.approved_plan_recovery_retries = 0;
    }

    pub(crate) fn queue_auto_permission_probe_warning(&mut self, warning: String) -> bool {
        if self.pending_auto_permission_probe_warning.is_some() {
            return false;
        }
        self.pending_auto_permission_probe_warning = Some(warning);
        true
    }

    pub(crate) fn take_auto_permission_probe_warning(&mut self) -> Option<String> {
        self.pending_auto_permission_probe_warning.take()
    }

    pub(crate) fn final_response_rendered(&self) -> bool {
        self.final_response_rendered
    }

    pub(crate) fn final_response_event_emitted(&self) -> bool {
        self.final_response_event_emitted
    }

    pub(crate) fn mark_final_response_rendered(&mut self) {
        self.final_response_rendered = true;
    }

    pub(crate) fn mark_final_response_event_emitted(&mut self) {
        self.final_response_event_emitted = true;
    }

    pub(crate) fn mark_streamed_response_event_emitted(&mut self) {
        self.streamed_response_event_emitted = true;
    }

    pub(crate) fn reset_streamed_response_event_emitted(&mut self) {
        self.streamed_response_event_emitted = false;
    }

    pub(crate) fn streamed_response_event_emitted(&self) -> bool {
        self.streamed_response_event_emitted
    }

    pub(crate) fn mark_final_response_fallback(&mut self) {
        self.final_response_was_fallback = true;
    }

    pub(crate) fn final_response_was_fallback(&self) -> bool {
        self.final_response_was_fallback
    }

    pub(crate) fn mark_turn_refused(&mut self) {
        self.turn_refused = true;
    }

    pub(crate) fn turn_refused(&self) -> bool {
        self.turn_refused
    }

    pub(crate) fn is_approved_plan_execution(&self) -> bool {
        self.approved_plan_execution
    }

    pub(crate) fn approved_plan_recovery_retries(&self) -> u8 {
        self.approved_plan_recovery_retries
    }

    pub(crate) fn record_approved_plan_recovery_retry(&mut self) {
        self.approved_plan_recovery_retries = self.approved_plan_recovery_retries.saturating_add(1);
    }

    pub(crate) fn consume_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::Pending) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::InPass;
        true
    }

    /// Resume tools only after admitted navigation recovery or explicit missing-fact clarification.
    /// The caller retains request allowances, hard budgets, and permission boundaries.
    pub(crate) fn resume_after_navigation_synthesis(&mut self) {
        self.recovery_phase = RecoveryPhase::Inactive;
        self.recovery_mode = None;
        self.recovery_reason = None;
        self.recovery_prompt_reason = None;
    }

    pub(crate) fn finish_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::InPass) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::Completed;
        true
    }

    /// Retry the recovery pass by resetting the phase back to `Pending`
    /// so the next loop iteration re-enters tool-free recovery mode.
    /// Increments the retry counter; the caller is responsible for checking
    /// `recovery_retry_count()` against its own limit.
    /// Only works if a recovery pass has been consumed (phase is InPass or Completed).
    pub(crate) fn retry_recovery_pass(&mut self) -> bool {
        if matches!(self.recovery_phase, RecoveryPhase::InPass | RecoveryPhase::Completed) {
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_retry_count += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn recovery_retry_count(&self) -> u8 {
        self.recovery_retry_count
    }

    /// Record best-effort prose salvaged from a rejected recovery synthesis
    /// response. Later rejections overwrite earlier ones (the latest attempt
    /// is the most complete).
    pub(crate) fn record_recovery_rejected_synthesis(&mut self, text: String) {
        if !text.trim().is_empty() {
            self.recovery_rejected_synthesis = Some(text);
        }
    }

    pub(crate) fn take_recovery_rejected_synthesis(&mut self) -> Option<String> {
        self.recovery_rejected_synthesis.take()
    }

    pub(crate) fn post_tool_recovery_cycles(&self) -> u8 {
        self.post_tool_recovery_cycles
    }

    /// Increment the tool-free post-tool recovery cycle counter. Returns the new value.
    pub(crate) fn increment_post_tool_recovery_cycle(&mut self) -> u8 {
        self.post_tool_recovery_cycles = self.post_tool_recovery_cycles.saturating_add(1);
        self.post_tool_recovery_cycles
    }

    pub(crate) fn record_spool_chunk_read(&mut self) -> usize {
        self.consecutive_spool_chunk_reads = self.consecutive_spool_chunk_reads.saturating_add(1);
        self.consecutive_spool_chunk_reads
    }

    pub(crate) fn reset_spool_chunk_read_streak(&mut self) {
        self.consecutive_spool_chunk_reads = 0;
    }

    pub(crate) fn record_shell_command_run(&mut self, signature: String) -> usize {
        if self.last_shell_command_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_shell_command_runs = self.consecutive_same_shell_command_runs.saturating_add(1);
        } else {
            self.last_shell_command_signature = Some(signature);
            self.consecutive_same_shell_command_runs = 1;
        }

        self.consecutive_same_shell_command_runs
    }

    pub(crate) fn reset_shell_command_run_streak(&mut self) {
        self.last_shell_command_signature = None;
        self.consecutive_same_shell_command_runs = 0;
    }

    pub(crate) fn record_admitted_shell_command(&mut self, signature: String) {
        self.last_admitted_shell_command_signature = Some(signature);
    }

    /// Remember this turn's failed shell execution, keyed by command plus
    /// first-line error text. The cross-turn tracker compares the key across
    /// turns; only byte-identical command/error pairs extend the streak, so
    /// retries after a genuine fix (changed error or success) start over.
    /// The last failure wins: one representative per turn is enough because
    /// the streak requires the *same* key in consecutive turns.
    pub(crate) fn record_failed_shell_command(&mut self, signature: String, error_signature: String) {
        self.last_failed_shell_key = Some(format!("{signature}::err::{error_signature}"));
    }

    pub(crate) fn last_failed_shell_key(&self) -> Option<&str> {
        self.last_failed_shell_key.as_deref()
    }

    pub(crate) fn record_file_read_family_call(&mut self, signature: String) -> usize {
        if self.last_file_read_family_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_file_read_family_calls =
                self.consecutive_same_file_read_family_calls.saturating_add(1);
        } else {
            self.last_file_read_family_signature = Some(signature);
            self.consecutive_same_file_read_family_calls = 1;
        }

        self.consecutive_same_file_read_family_calls
    }

    pub(crate) fn reset_file_read_family_streak(&mut self) {
        self.last_file_read_family_signature = None;
        self.consecutive_same_file_read_family_calls = 0;
    }

    /// Reserve the one path-cap exception before a batch starts executing.
    pub(crate) fn claim_patch_recovery_path(&mut self, path: std::path::PathBuf) -> bool {
        self.claimed_patch_recovery_paths.insert(path)
    }

    /// Record a read of `path` and return the total count of reads for that
    /// path this turn. Independent of slice (offset/limit/raw) — catches
    /// paginated reads of the same file that the slice-aware family key lets
    /// through.
    pub(crate) fn record_file_read_path_call(&mut self, path: String) -> usize {
        let count = self.file_read_path_counts.entry(path).or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

    #[cfg(test)]
    pub(super) fn reset_file_read_path_counts(&mut self) {
        self.file_read_path_counts.clear();
    }

    pub(crate) fn record_written_file(&mut self, path: &str) {
        self.recently_written_files.insert(path.to_string());
    }

    pub(crate) fn was_recently_written(&self, path: &str) -> bool {
        self.recently_written_files.contains(path)
    }

    pub(crate) fn record_task_tracker_create_signature(&mut self, signature: String) -> bool {
        self.seen_task_tracker_create_signatures.insert(signature)
    }

    pub(crate) fn clear_task_tracker_create_signatures(&mut self) {
        self.seen_task_tracker_create_signatures.clear();
    }

    pub(crate) fn record_successful_readonly_signature(&mut self, signature: String) -> bool {
        self.seen_successful_readonly_signatures.insert(signature)
    }

    pub(crate) fn has_successful_readonly_signature(&self, signature: &str) -> bool {
        self.seen_successful_readonly_signatures.contains(signature)
    }

    pub(crate) fn remember_streamed_tool_call_items<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = (String, StreamedToolCallItem)>,
    {
        self.streamed_tool_call_item_ids.extend(items);
    }

    pub(crate) fn take_streamed_tool_call_item_id(&mut self, tool_call_id: &str) -> Option<StreamedToolCallItem> {
        self.streamed_tool_call_item_ids.remove(tool_call_id)
    }

    /// Drain every streamed tool-call item still registered. Turn teardown
    /// uses this to close items the LLM runtime started but whose calls never
    /// reached the pipeline (rejected, dropped, or interrupted mid-batch), so
    /// they do not dangle as `item.started` forever.
    pub(crate) fn take_all_streamed_tool_call_item_ids(&mut self) -> Vec<(String, StreamedToolCallItem)> {
        self.streamed_tool_call_item_ids.drain().collect()
    }

    pub(crate) fn set_phase(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }

    pub(crate) fn execution_snapshot(&self) -> TurnExecutionSnapshot {
        TurnExecutionSnapshot {
            run_id: self.run_id.0.clone(),
            turn_id: self.turn_id.0.clone(),
            phase: self.phase,
            max_tool_calls: self.max_tool_calls,
            max_tool_wall_clock_secs: self.max_tool_wall_clock.as_secs(),
            max_tool_retries: self.max_tool_retries,
        }
    }
}
