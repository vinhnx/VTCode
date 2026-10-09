//! Runtime-owned task identity and item annotation shared by both runners.
use crate::exec::events::{
    CommandActivity, ExecutionContext, InputOrigin, ItemContext, ThreadEvent, ThreadItemDetails,
};
use crate::tools::tool_intent::{ShellActivity, classify_shell_activity};
use std::collections::VecDeque;

const MAX_TRACKED_EXEC_LAUNCHES: usize = 256;
const MAX_EXEC_ID_BYTES: usize = 256;

struct ExecSessionLaunch {
    session_id: Option<String>,
    call_item_id: String,
    tool_call_id: Option<String>,
    context: Box<ItemContext>,
}

/// Validate public rationale bounds at the tool boundary, independent of schemas.
pub fn validate_decision_input(args: serde_json::Value) -> anyhow::Result<crate::exec::events::DecisionItem> {
    use anyhow::{Context, bail};
    crate::tools::output_limits::max_output_tokens(&args)?;
    let args = crate::tools::output_limits::args_without_output_metadata(&args);
    if args.as_object().is_none_or(|o| {
        o.keys()
            .any(|k| !matches!(k.as_str(), "summary" | "rationale" | "alternatives" | "evidence_ids"))
    }) {
        bail!("invalid decision fields");
    }
    let d: crate::exec::events::DecisionItem = serde_json::from_value(args).context("invalid public decision")?;
    if d.summary.trim().is_empty()
        || d.summary.chars().count() > 240
        || d.rationale.trim().is_empty()
        || d.rationale.chars().count() > 1000
        || d.alternatives.len() > 3
        || d.evidence_ids.len() > 8
        || d.alternatives.iter().any(|a| a.chars().count() > 1000)
        || d.evidence_ids.iter().any(|id| id.is_empty() || id.chars().count() > 240)
    {
        bail!("public decision exceeds input bounds");
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn track_launch(tracker: &mut ExecutionContextTracker, item: &str, session: &str) {
        let args = json!({"cmd":"cargo check --locked"});
        let mut started = super::super::tool_started_event(item.into(), "exec_command", Some(&args), Some(item));
        tracker.annotate(&mut started);
        let pending = tracker
            .exec_session_output_event(item, "exec_command", &args, &json!({"session_id":session}))
            .unwrap();
        let ThreadEvent::ItemCompleted(completed) = pending else {
            panic!("canonical output expected")
        };
        let ThreadItemDetails::ToolOutput(output) = completed.item.details else {
            panic!("tool output expected")
        };
        assert_eq!(output.call_id, item);
        assert_eq!(output.status, crate::exec::events::ToolCallStatus::InProgress);
    }

    fn background(session: &str, task: &str, exit_code: Option<i32>) -> ThreadEvent {
        ThreadEvent::ItemCompleted(crate::exec::events::ItemCompletedEvent { item: crate::exec::events::ThreadItem {
            id: "background".into(), context: None,
            details: ThreadItemDetails::Harness(Box::new(serde_json::from_value(json!({
                "event":crate::exec::events::HarnessEventKind::BackgroundSubprocessCompleted,
                "task_id":task,"session_id":session,"exec_session_id":session,"exit_code":exit_code,"message":"observed exit",
            })).unwrap())),
        }})
    }

    #[test]
    fn native_delegation_status_is_captured_only_for_its_recorded_parent() {
        let mut tracker = ExecutionContextTracker::default();
        let current = tracker.begin("parent", "turn", "Delegate", InputOrigin::User);
        let output = json!({
            "id":"agent-1", "session_id":"child", "parent_thread_id":"parent", "agent_name":"reviewer", "display_label":"Review", "description":"Check storage", "source":"builtin", "status":"running", "background":true, "depth":1, "created_at":"2026-10-03T00:00:00Z", "updated_at":"2026-10-03T00:00:01Z",
        });
        let event = tracker.delegation_status_event("call", "agent", &output).unwrap();
        let ThreadEvent::ItemCompleted(completed) = event else {
            panic!("status event expected")
        };
        let context = completed.item.context.unwrap();
        assert_eq!(context.task_id, current.task_id);
        assert_eq!(context.actor_id, "child");
        assert_eq!(context.parent_actor_id.as_deref(), Some("parent"));
        let ThreadItemDetails::Harness(status) = completed.item.details else {
            panic!("harness status expected")
        };
        assert_eq!(status.status.as_deref(), Some("running"));
        assert!(tracker.delegation_status_event("call", "exec_command", &output).is_none());
        assert!(
            tracker
                .delegation_status_event("call", "agent", &json!({"session_id":"child"}))
                .is_none()
        );
        let mut unrelated = output.clone();
        unrelated["parent_thread_id"] = json!("unrelated");
        assert!(tracker.delegation_status_event("call", "agent", &unrelated).is_none());
        assert!(
            tracker
                .delegation_status_event("wait", "agent", &json!({"completed":true,"entry":output}))
                .is_some()
        );
    }

    #[test]
    fn exec_session_mapping_is_bounded_and_isolates_actors_and_unrelated_sessions() {
        let mut tracker = ExecutionContextTracker::default();
        let original = tracker.begin("root", "turn-1", "Verify", InputOrigin::User);
        for index in 0..=MAX_TRACKED_EXEC_LAUNCHES {
            track_launch(&mut tracker, &format!("launch-{index}"), &format!("session-{index}"));
        }
        assert_eq!(tracker.exec_launches.len(), MAX_TRACKED_EXEC_LAUNCHES);
        assert!(
            tracker
                .exec_session_output_event(
                    "poll",
                    "write_stdin",
                    &json!({"session_id":"session-0","chars":""}),
                    &json!({"exit_code":0})
                )
                .is_none()
        );
        assert!(
            tracker
                .exec_session_output_event(
                    "poll",
                    "write_stdin",
                    &json!({"session_id":"unknown","chars":""}),
                    &json!({"exit_code":0})
                )
                .is_none()
        );
        tracker.begin("other-actor", "turn-2", "Unrelated actor", InputOrigin::User);
        assert!(
            tracker
                .exec_session_output_event(
                    "poll",
                    "write_stdin",
                    &json!({"session_id":"session-1","chars":""}),
                    &json!({"exit_code":0})
                )
                .is_none()
        );
        tracker.begin("root", "turn-3", "Another task", InputOrigin::User);
        let completed = tracker
            .exec_session_output_event(
                "poll",
                "write_stdin",
                &json!({"session_id":"session-1","chars":""}),
                &json!({"exit_code":0}),
            )
            .unwrap();
        let ThreadEvent::ItemCompleted(completed) = completed else {
            panic!("completion expected")
        };
        let context = completed.item.context.unwrap();
        assert_eq!(context.task_id, original.task_id);
        assert_eq!(context.turn_id, "turn-1");
        assert_eq!(context.actor_id, "root");
        assert_eq!(tracker.exec_launches.len(), MAX_TRACKED_EXEC_LAUNCHES - 1);
    }

    #[test]
    fn raw_background_exit_correlates_to_the_original_verifier_only() {
        let mut tracker = ExecutionContextTracker::default();
        let original = tracker.begin("root", "turn", "Verify", InputOrigin::User);
        track_launch(&mut tracker, "original-call", "session");
        assert!(
            tracker
                .background_exec_output_event(&mut background("other", "exec:other", Some(0)))
                .is_none()
        );
        assert!(
            tracker
                .background_exec_output_event(&mut background("session", "managed-task", Some(0)))
                .is_none()
        );
        let pending = tracker
            .background_exec_output_event(&mut background("session", "exec:session", None))
            .unwrap();
        let ThreadEvent::ItemCompleted(pending) = pending else {
            panic!("pending output expected")
        };
        let ThreadItemDetails::ToolOutput(output) = pending.item.details else {
            panic!("tool output expected")
        };
        assert_eq!(output.status, crate::exec::events::ToolCallStatus::InProgress);
        tracker.begin("root", "new-turn", "Another task", InputOrigin::User);
        let mut raw = background("session", "exec:session", Some(7));
        tracker.annotate(&mut raw);
        let terminal = tracker.background_exec_output_event(&mut raw).unwrap();
        tracker.annotate(&mut raw);
        let ThreadEvent::ItemCompleted(raw) = raw else {
            panic!("raw completion expected")
        };
        assert_eq!(raw.item.context.unwrap().task_id, original.task_id);
        assert_eq!(tracker.current.as_ref().unwrap().turn_id, "new-turn");
        let ThreadEvent::ItemCompleted(terminal) = terminal else {
            panic!("terminal output expected")
        };
        assert_eq!(terminal.item.context.unwrap().task_id, original.task_id);
        let ThreadItemDetails::ToolOutput(output) = terminal.item.details else {
            panic!("tool output expected")
        };
        assert_eq!(output.call_id, "original-call");
        assert_eq!(output.status, crate::exec::events::ToolCallStatus::Failed);
        assert_eq!(output.exit_code, Some(7));
        assert!(
            tracker
                .background_exec_output_event(&mut background("session", "exec:session", Some(0)))
                .is_none()
        );
    }

    #[test]
    fn execution_task_identity_survives_corrections_and_handoffs() {
        let mut tracker = ExecutionContextTracker::default();
        let first = tracker.begin("root", "turn-1", "Fix login", InputOrigin::User);
        for (turn, origin) in [
            ("turn-2", InputOrigin::Correction),
            ("turn-3", InputOrigin::PlanApproval),
            ("turn-4", InputOrigin::Continuation),
            ("turn-5", InputOrigin::Retry),
        ] {
            let current = tracker.begin("root", turn, "changed input", origin);
            assert_eq!(current.task_id, first.task_id);
            assert_eq!(current.goal.as_deref(), Some("Fix login"));
            assert_eq!(current.turn_id, turn);
        }
        let next = tracker.begin("root", "turn-6", "Add logging", InputOrigin::User);
        assert_ne!(next.task_id, first.task_id);
        assert_eq!(next.goal.as_deref(), Some("Add logging"));
    }

    #[test]
    fn public_decision_bounds_are_validated_in_characters_and_fail_closed() {
        assert!(validate_decision_input(json!({"summary":"é".repeat(240),"rationale":"r".repeat(1000),"alternatives":["a","b","c"],"evidence_ids":vec!["id";8]})).is_ok());
        for invalid in [
            json!({"summary":"a".repeat(241),"rationale":"why"}),
            json!({"summary":"choice","rationale":"a".repeat(1001)}),
            json!({"summary":"choice","rationale":"why","alternatives":["a","b","c","d"]}),
            json!({"summary":"choice","rationale":"why","evidence_ids":[""]}),
            json!({"summary":"choice","rationale":"why","private_reasoning":"hidden"}),
            json!({"summary":" ","rationale":"why"}),
        ] {
            assert!(validate_decision_input(invalid).is_err());
        }
    }

    #[test]
    fn public_decision_accepts_valid_common_output_metadata() {
        let args = json!({"summary":"Reuse parser","rationale":"Preserve validation","max_output_tokens":100});
        let decision = validate_decision_input(args.clone()).unwrap();
        assert_eq!(decision.summary, "Reuse parser");
        let event = super::super::tool_invocation_completed_event(
            "decision-call".into(),
            "record_decision",
            Some(&args),
            None,
            crate::exec::events::ToolCallStatus::Completed,
            crate::exec::events::ToolOutcome::Success,
        );
        assert!(decision_completed_event(&event).is_some());
        for invalid in [json!(0), json!(50_001), json!("100"), json!(1.5)] {
            let mut args = args.clone();
            args["max_output_tokens"] = invalid;
            assert!(validate_decision_input(args).is_err());
        }
    }
}

/// Convert a successful decision call into a canonical public decision item.
pub fn decision_completed_event(event: &ThreadEvent) -> Option<ThreadEvent> {
    let ThreadEvent::ItemCompleted(e) = event else {
        return None;
    };
    let ThreadItemDetails::ToolInvocation(t) = &e.item.details else {
        return None;
    };
    if t.tool_name != crate::config::constants::tools::RECORD_DECISION
        || t.status != crate::exec::events::ToolCallStatus::Completed
        || t.outcome != Some(crate::exec::events::ToolOutcome::Success)
    {
        return None;
    }
    let decision = validate_decision_input(t.arguments.clone()?).ok()?;
    Some(ThreadEvent::ItemCompleted(crate::exec::events::ItemCompletedEvent {
        item: crate::exec::events::ThreadItem {
            id: format!("{}:decision", e.item.id),
            context: e.item.context.clone(),
            details: ThreadItemDetails::Decision(Box::new(decision)),
        },
    }))
}

#[derive(Default)]
pub struct ExecutionContextTracker {
    current: Option<ExecutionContext>,
    exec_launches: VecDeque<ExecSessionLaunch>,
}

impl ExecutionContextTracker {
    /// Preserve native delegation metadata before tool-output compaction.
    pub fn delegation_status_event(
        &self,
        call_item_id: &str,
        tool_name: &str,
        output: &serde_json::Value,
    ) -> Option<ThreadEvent> {
        use crate::config::constants::tools;
        use crate::exec::events::{HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ThreadItem};
        if !matches!(
            tool_name,
            tools::AGENT
                | tools::SPAWN_AGENT
                | tools::WAIT_AGENT
                | tools::RESUME_AGENT
                | tools::CLOSE_AGENT
                | tools::SEND_INPUT
        ) {
            return None;
        }
        let value = output.get("entry").unwrap_or(output);
        let entry: crate::subagents::SubagentStatusEntry = serde_json::from_value(value.clone()).ok()?;
        let current = self.current.as_ref()?;
        if entry.session_id.is_empty()
            || entry.session_id.len() > MAX_EXEC_ID_BYTES
            || entry.parent_thread_id != current.actor_id
        {
            return None;
        }
        let item: HarnessEventItem = serde_json::from_value(serde_json::json!({
            "event": HarnessEventKind::DelegatedAgentStatus,
            "message": format!("Delegated {}: {}", entry.display_label, entry.description),
            "task_id": entry.id,
            "session_id": entry.session_id,
            "status": entry.status.as_str(),
        }))
        .ok()?;
        Some(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: format!("{call_item_id}:delegated-agent"),
                context: Some(Box::new(ItemContext {
                    task_id: current.task_id.clone(),
                    turn_id: current.turn_id.clone(),
                    actor_id: entry.session_id,
                    parent_actor_id: Some(entry.parent_thread_id),
                    timestamp: entry.updated_at.to_rfc3339(),
                    activity: None,
                })),
                details: ThreadItemDetails::Harness(Box::new(item)),
            },
        }))
    }

    /// A new idle user request starts a task; other origins retain its root goal.
    pub fn begin(&mut self, actor: &str, turn: &str, input: &str, origin: InputOrigin) -> ExecutionContext {
        let previous = self.current.as_ref().filter(|_| origin != InputOrigin::User);
        let context = ExecutionContext {
            task_id: previous.map_or_else(|| format!("task-{}", uuid::Uuid::new_v4()), |c| c.task_id.clone()),
            turn_id: turn.to_owned(),
            actor_id: actor.to_owned(),
            parent_actor_id: None,
            origin,
            timestamp: chrono::Utc::now().to_rfc3339(),
            goal: previous.and_then(|c| c.goal.clone()).or_else(|| Some(input.to_owned())),
        };
        self.current = Some(context.clone());
        context
    }

    /// Annotate before persistence and exporter fanout so all consumers agree.
    pub fn annotate(&mut self, event: &mut ThreadEvent) {
        let now = chrono::Utc::now().to_rfc3339();
        match event {
            ThreadEvent::TurnStarted(e) => {
                if let Some(context) = e.context.as_deref() {
                    self.current = Some(context.clone());
                } else {
                    e.context = self.current.clone().map(Box::new);
                }
            }
            ThreadEvent::TurnCompleted(e) => {
                e.completed_at.get_or_insert_with(|| Box::new(now));
            }
            ThreadEvent::TurnFailed(e) => {
                e.completed_at.get_or_insert_with(|| Box::new(now));
            }
            ThreadEvent::ThreadCompleted(e) => {
                e.completed_at.get_or_insert(now);
            }
            ThreadEvent::TurnBlocked(e) => {
                e.completed_at.get_or_insert(now);
            }
            ThreadEvent::ItemStarted(e) => self.annotate_item(&mut e.item, now),
            ThreadEvent::ItemUpdated(e) => self.annotate_item(&mut e.item, now),
            ThreadEvent::ItemCompleted(e) => self.annotate_item(&mut e.item, now),
            _ => {}
        }
    }

    fn annotate_item(&mut self, item: &mut crate::exec::events::ThreadItem, timestamp: String) {
        let Some(c) = &self.current else {
            return;
        };
        let activity = match &item.details {
            ThreadItemDetails::ToolInvocation(t) => t
                .arguments
                .as_ref()
                .filter(|_| crate::tools::tool_intent::is_command_session_tool(&t.tool_name))
                .map(|args| classify_shell_activity(&t.tool_name, args)),
            ThreadItemDetails::CommandExecution(t) => {
                Some(classify_shell_activity("exec_command", &serde_json::json!({"cmd": t.command})))
            }
            _ => None,
        }
        .map(|a| match a {
            ShellActivity::Inspection => CommandActivity::Inspection,
            ShellActivity::Verification => CommandActivity::Verification,
            ShellActivity::Mutation => CommandActivity::Mutation,
        });
        item.context.get_or_insert_with(|| {
            Box::new(ItemContext {
                task_id: c.task_id.clone(),
                turn_id: c.turn_id.clone(),
                actor_id: c.actor_id.clone(),
                parent_actor_id: c.parent_actor_id.clone(),
                timestamp,
                activity,
            })
        });
        let ThreadItemDetails::ToolInvocation(invocation) = &item.details else {
            return;
        };
        let Some(args) = invocation.arguments.as_ref() else {
            return;
        };
        let Some(context) = item.context.as_ref() else { return };
        if invocation.status == crate::exec::events::ToolCallStatus::Failed {
            self.exec_launches.retain(|launch| {
                launch.call_item_id != item.id
                    || launch.context.task_id != context.task_id
                    || launch.context.actor_id != context.actor_id
            });
            return;
        }
        if context.activity != Some(CommandActivity::Verification)
            || !crate::tools::tool_intent::is_command_run_tool_call(&invocation.tool_name, args)
            || item.id.len() > MAX_EXEC_ID_BYTES
            || invocation.tool_call_id.as_ref().is_some_and(|id| id.len() > MAX_EXEC_ID_BYTES)
            || [
                &context.task_id,
                &context.turn_id,
                &context.actor_id,
                &context.timestamp,
            ]
            .into_iter()
            .any(|id| id.len() > MAX_EXEC_ID_BYTES)
            || context.parent_actor_id.as_ref().is_some_and(|id| id.len() > MAX_EXEC_ID_BYTES)
            || self.exec_launches.iter().any(|launch| {
                launch.call_item_id == item.id
                    && launch.context.task_id == context.task_id
                    && launch.context.actor_id == context.actor_id
            })
        {
            return;
        }
        if self.exec_launches.len() == MAX_TRACKED_EXEC_LAUNCHES {
            self.exec_launches.pop_front();
        }
        self.exec_launches.push_back(ExecSessionLaunch {
            session_id: None,
            call_item_id: item.id.clone(),
            tool_call_id: invocation.tool_call_id.clone(),
            context: context.clone(),
        });
    }

    /// Link authoritative exec results to their original verifier invocation.
    /// Session identities, never repeated command text, own this association.
    pub fn exec_session_output_event(
        &mut self,
        call_item_id: &str,
        tool_name: &str,
        args: &serde_json::Value,
        output: &serde_json::Value,
    ) -> Option<ThreadEvent> {
        crate::tools::tool_intent::canonical_command_session_tool_name(tool_name)?;
        let current = self.current.as_ref()?;
        let is_launch = crate::tools::tool_intent::is_command_run_tool_call(tool_name, args);
        let response_session = crate::tools::command_args::session_id_text(output);
        let requested_session = crate::tools::command_args::session_id_text(args);
        if response_session.is_some_and(|id| id.len() > MAX_EXEC_ID_BYTES)
            || (!is_launch && response_session.is_some() && response_session != requested_session)
        {
            return None;
        }
        let position = self.exec_launches.iter().position(|launch| {
            launch.context.actor_id == current.actor_id
                && if is_launch {
                    launch.context.task_id == current.task_id && launch.call_item_id == call_item_id
                } else {
                    requested_session.is_some() && launch.session_id.as_deref() == requested_session
                }
        })?;
        let exit_code = output
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .and_then(|code| i32::try_from(code).ok());
        if exit_code == Some(0)
            && (output.get("success").and_then(serde_json::Value::as_bool) == Some(false)
                || output.get("error").is_some_and(|error| !error.is_null()))
        {
            return None;
        }
        if is_launch {
            if exit_code.is_some() || response_session.is_none() {
                self.exec_launches.remove(position);
                return None;
            }
            self.exec_launches.get_mut(position)?.session_id = response_session.map(str::to_owned);
        }
        let payload = super::tool_output_payload_from_value(output);
        self.exec_session_completion_event(
            position,
            exit_code,
            payload.spool_path.as_deref(),
            payload.aggregated_output,
        )
    }

    /// Resolve raw subprocess completion facts through the same launch mapping.
    pub fn background_exec_output_event(&mut self, event: &mut ThreadEvent) -> Option<ThreadEvent> {
        let ThreadEvent::ItemCompleted(completed) = event else {
            return None;
        };
        let ThreadItemDetails::Harness(harness) = &completed.item.details else {
            return None;
        };
        if harness.event != crate::exec::events::HarnessEventKind::BackgroundSubprocessCompleted {
            return None;
        }
        let session_id = harness.exec_session_id.as_deref()?;
        // Raw exec completions carry this exact runtime-owned identity. Managed
        // subagents use their own task/session identities and cannot alias it.
        if harness.task_id.as_deref() != Some(format!("exec:{session_id}").as_str())
            || harness.session_id.as_deref() != Some(session_id)
        {
            return None;
        }
        let current = self.current.as_ref()?;
        let position = self.exec_launches.iter().position(|launch| {
            launch.context.actor_id == current.actor_id && launch.session_id.as_deref() == Some(session_id)
        })?;
        let output = self.exec_session_completion_event(
            position,
            harness.exit_code,
            None,
            harness.message.clone().unwrap_or_default(),
        )?;
        if let ThreadEvent::ItemCompleted(output) = &output {
            completed.item.context = output.item.context.clone();
        }
        Some(output)
    }

    fn exec_session_completion_event(
        &mut self,
        position: usize,
        exit_code: Option<i32>,
        spool_path: Option<&str>,
        output: String,
    ) -> Option<ThreadEvent> {
        let launch = self.exec_launches.get(position)?;
        let status = match exit_code {
            Some(0) => crate::exec::events::ToolCallStatus::Completed,
            Some(_) => crate::exec::events::ToolCallStatus::Failed,
            None => crate::exec::events::ToolCallStatus::InProgress,
        };
        let mut event = super::tool_output_completed_event(
            launch.call_item_id.clone(),
            launch.tool_call_id.as_deref(),
            status,
            exit_code,
            spool_path,
            output,
        );
        if let ThreadEvent::ItemCompleted(completed) = &mut event {
            completed.item.id = format!("{}:exec-session-output", launch.call_item_id);
            let mut context = launch.context.clone();
            context.timestamp = chrono::Utc::now().to_rfc3339();
            completed.item.context = Some(context);
        }
        if exit_code.is_some() {
            self.exec_launches.remove(position);
        }
        Some(event)
    }
}
