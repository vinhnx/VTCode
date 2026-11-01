use super::*;

#[derive(Debug, Clone)]
pub struct ActiveCommandHandle {
    id: String,
    command: String,
}

#[derive(Debug, Clone)]
pub struct ActiveToolHandle {
    pub(super) id: String,
    tool_name: String,
    arguments: Option<Value>,
    tool_call_id: Option<String>,
}

impl ActiveToolHandle {
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.id
    }
}

/// Helper responsible for recording execution events and relaying them to optional sinks.
#[derive(Default)]
pub struct ExecEventRecorder {
    context: ExecutionContextTracker,
    thread_id: String,
    pub(super) events: Vec<ThreadEvent>,
    event_sink: Option<EventSink>,
    thread_handle: Option<ThreadRuntimeHandle>,
    active_submission_id: Option<SubmissionId>,
    active_turn_id: Option<String>,
    lifecycle: SharedLifecycleEmitter,
}

impl ExecEventRecorder {
    pub fn new(
        thread_id: impl Into<String>,
        event_sink: Option<EventSink>,
        thread_handle: Option<ThreadRuntimeHandle>,
    ) -> Self {
        let thread_id = thread_id.into();
        let mut recorder = Self {
            context: ExecutionContextTracker::default(),
            thread_id: thread_id.clone(),
            events: Vec::new(),
            event_sink,
            thread_handle,
            active_submission_id: None,
            active_turn_id: None,
            lifecycle: SharedLifecycleEmitter::default(),
        };
        recorder.record_with_context(None, None, ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id }));
        recorder
    }

    fn record(&mut self, event: ThreadEvent) {
        self.record_with_context(self.active_submission_id.clone(), self.active_turn_id.clone(), event);
    }

    fn record_with_context(
        &mut self,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        mut event: ThreadEvent,
    ) {
        self.context.annotate(&mut event);
        let decision = decision_completed_event(&event);
        let exec_completion = self.context.background_exec_output_event(&mut event);
        if let Some(sink) = &self.event_sink {
            let mut callback = sink.lock();
            callback(&event);
        }
        if let Some(handle) = &self.thread_handle {
            handle.record_event(submission_id, turn_id, event.clone());
        }
        self.events.push(event);
        if let Some(decision) = decision {
            self.record(decision);
        }
        if let Some(completion) = exec_completion {
            self.record(completion);
        }
    }

    /// Attach a session result to its original verifier's canonical output.
    pub fn record_exec_session_output(&mut self, call_item_id: &str, tool_name: &str, args: &Value, output: &Value) {
        if let Some(event) = self.context.exec_session_output_event(call_item_id, tool_name, args, output) {
            self.record(event);
        }
        if let Some(event) = self.context.delegation_status_event(call_item_id, tool_name, output) {
            self.record(event);
        }
        if let Some(event) = self.context.session_message_event(call_item_id, tool_name, output) {
            self.record(event);
        }
    }

    pub fn record_thread_event(&mut self, event: ThreadEvent) {
        self.record(event);
    }

    pub fn record_thread_events<I>(&mut self, events: I)
    where
        I: IntoIterator<Item = ThreadEvent>,
    {
        for event in events {
            self.record(event);
        }
    }

    fn record_pending_lifecycle_events(&mut self) {
        for event in self.lifecycle.drain_events() {
            self.record(event);
        }
    }

    fn next_item_id(&mut self) -> String {
        self.lifecycle.next_item_id()
    }

    pub fn turn_started(&mut self) {
        let turn_id = format!("turn-{}", Uuid::new_v4());
        self.context
            .begin(&self.thread_id, &turn_id, "", crate::exec::events::InputOrigin::Continuation);
        self.active_turn_id = Some(turn_id);
        self.begin_turn_submission();
    }

    fn begin_turn_submission(&mut self) {
        if let Some(handle) = &self.thread_handle {
            match handle.begin_turn() {
                Ok(submission_id) => self.active_submission_id = Some(submission_id),
                Err(err) => {
                    // A failed begin_turn means a previous turn was never
                    // finished (or a concurrent turn is in flight). Surface
                    // it instead of silently dropping the submission id:
                    // without it, every event of this turn loses its
                    // submission context on retried attempts.
                    tracing::warn!(error = %err, "failed to begin turn; submission id unavailable");
                    self.active_submission_id = None;
                }
            }
        }
        self.record(ThreadEvent::TurnStarted(TurnStartedEvent::default()));
    }

    /// Start a task with its original public request.
    pub fn task_started(&mut self, goal: &str) -> String {
        let context = self.context.begin(
            &self.thread_id,
            &format!("turn-{}", Uuid::new_v4()),
            goal,
            crate::exec::events::InputOrigin::User,
        );
        self.active_turn_id = Some(context.turn_id);
        self.begin_turn_submission();
        context.task_id
    }

    pub fn turn_completed(&mut self, usage: Usage) {
        self.record(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage,
            in_progress_exec_sessions: Vec::new(),
        }));
        self.finish_turn();
    }

    pub fn turn_failed(&mut self, message: &str, usage: Option<Usage>) {
        self.record(ThreadEvent::TurnFailed(TurnFailedEvent {
            completed_at: None,
            message: message.to_string(),
            usage,
        }));
        self.finish_turn();
    }

    pub fn turn_blocked(&mut self, event: TurnBlockedEvent) {
        self.record(ThreadEvent::TurnBlocked(Box::new(event)));
    }

    pub fn thread_completed(
        &mut self,
        session_id: &str,
        subtype: ThreadCompletionSubtype,
        outcome_code: &str,
        result: Option<&str>,
        stop_reason: Option<&str>,
        usage: Usage,
        total_cost_usd: Option<serde_json::Number>,
        num_turns: usize,
    ) {
        self.record(ThreadEvent::ThreadCompleted(Box::new(ThreadCompletedEvent {
            completed_at: None,
            thread_id: self.thread_id.clone(),
            session_id: session_id.to_string(),
            subtype,
            outcome_code: outcome_code.to_string(),
            result: result.map(str::to_string),
            stop_reason: stop_reason.map(str::to_string),
            usage,
            total_cost_usd,
            num_turns,
        })));
    }

    /// Record the terminal lifecycle events for an execution failure.
    pub fn thread_failed(&mut self, session_id: &str, message: &str, num_turns: usize) {
        self.turn_failed(message, None);
        self.thread_completed(
            session_id,
            ThreadCompletionSubtype::ErrorDuringExecution,
            "error",
            None,
            Some(message),
            Usage::default(),
            None,
            num_turns,
        );
    }

    pub fn compact_boundary(
        &mut self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
        previous_envelope: Option<&crate::core::agent::request_envelope::SessionRequestEnvelope>,
        new_envelope: Option<&crate::core::agent::request_envelope::SessionRequestEnvelope>,
    ) {
        let previous_segment_id = previous_envelope.map(|envelope| envelope.segment_id().to_owned());
        let new_segment_id = new_envelope.map(|envelope| envelope.segment_id().to_owned());
        let previous_prefix_hash = previous_envelope.map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let new_prefix_hash = new_envelope.map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let previous_catalog_hash = previous_envelope
            .and_then(crate::core::agent::request_envelope::SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        let new_catalog_hash = new_envelope
            .and_then(crate::core::agent::request_envelope::SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        self.record(ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
            thread_id: self.thread_id.clone(),
            trigger,
            mode,
            original_message_count,
            compacted_message_count,
            history_artifact_path: history_artifact_path.map(str::to_string),
            previous_segment_id,
            new_segment_id,
            previous_prefix_hash,
            new_prefix_hash,
            previous_catalog_hash,
            new_catalog_hash,
        })));
    }

    fn finish_turn(&mut self) {
        if let Some(handle) = &self.thread_handle {
            handle.finish_turn();
        }
        self.active_submission_id = None;
        self.active_turn_id = None;
    }

    pub fn agent_message(&mut self, text: &str) {
        self.lifecycle.emit_completed_agent_message(text);
        self.record_pending_lifecycle_events();
    }

    pub fn agent_message_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_assistant_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_assistant_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn agent_message_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_assistant_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning(&mut self, text: &str) {
        self.lifecycle.emit_completed_reasoning(text);
        self.record_pending_lifecycle_events();
    }

    pub fn set_reasoning_stage(&mut self, stage: &str) {
        if !self.lifecycle.set_reasoning_stage(Some(stage.to_string())) {
            return;
        }
        let _ = self.lifecycle.emit_reasoning_stage_update();
        self.record_pending_lifecycle_events();
    }

    pub fn reasoning_stream_update(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || !self.lifecycle.replace_reasoning_text(text) {
            return false;
        }
        let emitted = self.lifecycle.emit_reasoning_snapshot(None);
        self.record_pending_lifecycle_events();
        emitted
    }

    pub fn reasoning_stream_complete(&mut self) {
        let _ = self.lifecycle.complete_reasoning_stream();
        self.record_pending_lifecycle_events();
    }

    pub fn tool_started(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
    ) -> ActiveToolHandle {
        let handle = ActiveToolHandle {
            id: self.next_item_id(),
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
        };
        self.record(tool_started_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
        ));
        handle
    }

    pub fn tool_finished(
        &mut self,
        handle: &ActiveToolHandle,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
        app_previews: Vec<AppPreviewItem>,
    ) {
        let outcome = tool_outcome_from_status(&status);
        self.record(tool_invocation_completed_event(
            handle.id.clone(),
            &handle.tool_name,
            handle.arguments.as_ref(),
            handle.tool_call_id.as_deref(),
            status.clone(),
            outcome,
        ));
        self.record(tool_output_completed_event(
            handle.id.clone(),
            handle.tool_call_id.as_deref(),
            status,
            exit_code,
            spool_path,
            aggregated_output,
            app_previews,
        ));
    }

    pub fn tool_output_started(&mut self, call_item_id: &str, tool_call_id: Option<&str>) {
        self.record(tool_output_started_event(call_item_id.to_string(), tool_call_id));
    }

    pub fn tool_output_updated(&mut self, call_item_id: &str, tool_call_id: Option<&str>, output: &str) {
        self.record(tool_output_updated_event(call_item_id.to_string(), tool_call_id, output));
    }

    pub fn tool_output_finished(
        &mut self,
        call_item_id: &str,
        tool_call_id: Option<&str>,
        status: crate::exec::events::ToolCallStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
        spool_path: Option<&str>,
        app_previews: Vec<AppPreviewItem>,
    ) {
        self.record(tool_output_completed_event(
            call_item_id.to_string(),
            tool_call_id,
            status,
            exit_code,
            spool_path,
            aggregated_output,
            app_previews,
        ));
    }

    pub fn tool_rejected(
        &mut self,
        tool_name: &str,
        arguments: Option<&Value>,
        tool_call_id: Option<&str>,
        detail: &str,
    ) {
        let handle = self.tool_started(tool_name, arguments, tool_call_id);
        let call_item_id = handle.id.clone();
        self.record(tool_invocation_completed_event(
            call_item_id.clone(),
            tool_name,
            arguments,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            ToolOutcome::HookDenied,
        ));
        self.record(tool_output_started_event(call_item_id.clone(), tool_call_id));
        self.record(tool_output_completed_event(
            call_item_id,
            tool_call_id,
            crate::exec::events::ToolCallStatus::Failed,
            None,
            None,
            detail,
            Vec::new(),
        ));
        let error_item_id = self.next_item_id();
        self.record(error_item_completed_event(error_item_id, detail.to_string()));
    }

    pub fn permission_requested(&mut self, tool_name: &str) {
        self.record(ThreadEvent::PermissionRequested(crate::exec::events::PermissionRequestedEvent {
            tool_name: tool_name.to_string(),
            tool_call_id: None,
        }));
    }

    pub fn permission_resolved(
        &mut self,
        tool_name: &str,
        decision: crate::exec::events::PermissionDecision,
        wait_ms: u64,
    ) {
        self.record(ThreadEvent::PermissionResolved(crate::exec::events::PermissionResolvedEvent {
            tool_name: tool_name.to_string(),
            tool_call_id: None,
            decision,
            wait_ms,
        }));
    }

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle {
        let id = self.next_item_id();
        let item = ThreadItem {
            context: None,
            id: id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: command.to_string(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: None,
                status: CommandExecutionStatus::InProgress,
            })),
        };
        self.record(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        ActiveCommandHandle { id, command: command.to_string() }
    }

    pub fn command_finished(
        &mut self,
        handle: &ActiveCommandHandle,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        aggregated_output: &str,
    ) {
        let item = ThreadItem {
            context: None,
            id: handle.id.clone(),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: handle.command.clone(),
                arguments: None,
                aggregated_output: aggregated_output.to_string(),
                exit_code,
                status,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Record an operational incident update using the canonical event contract.
    pub fn diagnostic(&mut self, diagnostic: &vtcode_commons::diagnostics::Diagnostic) {
        let diagnostic = vtcode_commons::diagnostics::redact_diagnostic(diagnostic);
        let previous = self.events.iter().rev().find_map(|event| {
            let item = match event {
                ThreadEvent::ItemStarted(event) => &event.item,
                ThreadEvent::ItemUpdated(event) => &event.item,
                ThreadEvent::ItemCompleted(event) => &event.item,
                _ => return None,
            };
            let ThreadItemDetails::Reasoning(reasoning) = &item.details else {
                return None;
            };
            reasoning
                .diagnostic
                .as_deref()
                .filter(|previous| previous.correlation_id == diagnostic.correlation_id)
        });
        if previous == Some(&diagnostic) {
            return;
        }
        self.record(ThreadEvent::ItemUpdated(crate::exec::events::ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: diagnostic.correlation_id.clone(),
                details: ThreadItemDetails::Reasoning(Box::new(crate::exec::events::ReasoningItem {
                    text: diagnostic.summary.clone(),
                    stage: Some("diagnosis".into()),
                    diagnostic: Some(Box::new(diagnostic)),
                })),
            },
        }));
    }

    #[track_caller]
    pub fn warning(&mut self, message: &str) {
        self.diagnostic(&vtcode_commons::diagnostics::legacy_ui_diagnostic(
            vtcode_commons::diagnostics::DiagnosticSeverity::Warning,
            message,
        ));
    }

    pub fn harness_event(
        &mut self,
        event: HarnessEventKind,
        message: Option<String>,
        command: Option<String>,
        path: Option<String>,
        exit_code: Option<i32>,
        attempt: Option<u32>,
        error_category: Option<String>,
    ) {
        let item = ThreadItem {
            context: None,
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event,
                message,
                command,
                path,
                exit_code,
                attempt,
                error_category,
                duration_ms: None,
                task_id: None,
                session_id: None,
                correlation_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit a tool latency harness event with recorded duration.
    pub fn record_tool_latency(&mut self, tool_name: &str, duration_ms: u64) {
        self.record_tool_outcome(tool_name, 1, duration_ms, None);
    }

    /// Emit the single terminal harness observation for one invocation.
    pub fn record_tool_outcome(
        &mut self,
        tool_name: &str,
        attempts: u32,
        duration_ms: u64,
        error_category: Option<&str>,
    ) {
        let item = ThreadItem {
            context: None,
            id: self.next_item_id(),
            details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                event: HarnessEventKind::ToolLatencyRecorded,
                message: Some(format!("{tool_name} completed in {duration_ms}ms")),
                command: None,
                path: None,
                exit_code: None,
                attempt: Some(attempts),
                error_category: error_category.map(str::to_string),
                duration_ms: Some(duration_ms),
                task_id: None,
                session_id: None,
                correlation_id: None,
                exec_session_id: None,
                status: None,
                transcript_path: None,
                archive_path: None,
            })),
        };
        self.record(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
    }

    /// Emit an `ErrorRecovered` harness event, recording that the agent
    /// successfully recovered from a transient error after retries.
    pub fn error_recovered(&mut self, tool_name: &str, attempt: u32, error_category: &str) {
        self.harness_event(
            HarnessEventKind::ErrorRecovered,
            Some(format!("{tool_name} recovered after {attempt} retries")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    /// Emit a `ToolRetryAttempted` harness event, recording that a transient
    /// tool failure triggered an automatic retry.
    pub fn tool_retry_attempted(&mut self, tool_name: &str, attempt: u32, error_category: &str, delay_ms: u64) {
        self.harness_event(
            HarnessEventKind::ToolRetryAttempted,
            Some(format!("{tool_name}: retry {attempt} after {delay_ms}ms")),
            None,
            None,
            None,
            Some(attempt),
            Some(error_category.to_string()),
        );
    }

    /// Drain the collected events while keeping the recorder available for a
    /// terminal failure path that may run after the normal result assembly.
    pub fn take_events(&mut self) -> Vec<ThreadEvent> {
        self.lifecycle.complete_open_items();
        self.record_pending_lifecycle_events();
        std::mem::take(&mut self.events)
    }

    pub fn into_events(mut self) -> Vec<ThreadEvent> {
        self.take_events()
    }
}
