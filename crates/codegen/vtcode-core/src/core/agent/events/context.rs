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

/// Parse one raw checklist item into snapshot state.
///
/// Mirrors `compact_task_tree_view_from_items` filtering: items without a
/// non-empty description/text are skipped, unparseable statuses are skipped,
/// and a missing status defaults to pending. Index paths accept dotted
/// `index_path` values and numeric `index` values, falling back to the
/// 1-based position so every row keeps a stable identity.
fn tracker_item_state(
    item: &serde_json::Value,
    position: usize,
) -> Option<crate::exec::events::tracker::TrackerItemState> {
    use crate::exec::events::tracker::{TrackerItemState, TrackerItemStatus};
    use std::str::FromStr;

    let index_path = item
        .get("index_path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            item.get("index")
                .and_then(serde_json::Value::as_u64)
                .map(|index| index.to_string())
        })
        .unwrap_or_else(|| (position + 1).to_string());
    let status = match item.get("status").and_then(serde_json::Value::as_str) {
        Some(raw) => TrackerItemStatus::from_str(raw).ok()?,
        None => TrackerItemStatus::Pending,
    };
    let description = item
        .get("description")
        .and_then(serde_json::Value::as_str)
        .or_else(|| item.get("text").and_then(serde_json::Value::as_str))
        .filter(|description| !description.trim().is_empty())?;
    let short = crate::tools::handlers::task_tracking::short_task_description(description);
    let description = if short.trim().is_empty() {
        description.trim().to_string()
    } else {
        short
    };
    Some(TrackerItemState { index_path, description, status })
}

/// Resolve snapshot progress counts.
///
/// Explicit `completed`/`total` win; otherwise counts derive from the parsed
/// items. Returns `None` when neither source can describe progress, mirroring
/// the CLI progress-line contract.
fn tracker_snapshot_counts(
    checklist: &serde_json::Map<String, serde_json::Value>,
    items: &[crate::exec::events::tracker::TrackerItemState],
) -> Option<(usize, usize)> {
    use crate::exec::events::tracker::TrackerItemStatus;

    if let (Some(completed), Some(total)) = (
        checklist.get("completed").and_then(serde_json::Value::as_u64),
        checklist.get("total").and_then(serde_json::Value::as_u64),
    ) {
        return Some((usize::try_from(completed).unwrap_or(usize::MAX), usize::try_from(total).unwrap_or(usize::MAX)));
    }
    if items.is_empty() {
        return None;
    }
    let completed = items.iter().filter(|item| item.status == TrackerItemStatus::Completed).count();
    Some((completed, items.len()))
}

#[cfg(test)]
mod tests;

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

    /// Preserve peer-messaging sends as harness status before tool-output compaction.
    ///
    /// Mirrors [`Self::delegation_status_event`]: a successful `ask_session` or
    /// `answer_session` call fans out a `SessionMessageSent` event carrying the
    /// peer session and the mailbox message id as `correlation_id`, so the
    /// later `SessionMessageReceived` drain event can be linked to this send.
    pub fn session_message_event(
        &self,
        call_item_id: &str,
        tool_name: &str,
        output: &serde_json::Value,
    ) -> Option<ThreadEvent> {
        use crate::config::constants::tools;
        use crate::exec::events::{HarnessEventItem, HarnessEventKind, ItemCompletedEvent, ThreadItem};
        if !matches!(tool_name, tools::ASK_SESSION | tools::ANSWER_SESSION) {
            return None;
        }
        let current = self.current.as_ref()?;
        let message_id = output.get("message_id")?.as_str()?;
        let to_session = output.get("to_session")?.as_str()?;
        if message_id.is_empty() || to_session.is_empty() || to_session.len() > MAX_EXEC_ID_BYTES {
            return None;
        }
        let item: HarnessEventItem = serde_json::from_value(serde_json::json!({
            "event": HarnessEventKind::SessionMessageSent,
            "message": format!("Message to session {to_session}"),
            "task_id": current.task_id,
            "session_id": to_session,
            "correlation_id": message_id,
        }))
        .ok()?;
        Some(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: format!("{call_item_id}:session-message"),
                context: Some(Box::new(ItemContext {
                    task_id: current.task_id.clone(),
                    turn_id: current.turn_id.clone(),
                    actor_id: current.actor_id.clone(),
                    parent_actor_id: None,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    activity: None,
                })),
                details: ThreadItemDetails::Harness(Box::new(item)),
            },
        }))
    }

    /// Publish a typed checklist snapshot after a `task_tracker` mutation.
    ///
    /// Mirrors [`Self::delegation_status_event`]: parses the structured tool
    /// output (still a [`serde_json::Value`] here, before output compaction
    /// renders it into display text) into a [`ThreadEvent::TrackerUpdated`]
    /// snapshot so desktop and archive consumers replay typed state instead
    /// of re-parsing rendered output. Returns `None` for other tools,
    /// missing checklists, and checklists with neither renderable items nor
    /// explicit counts.
    pub fn tracker_status_event(&self, tool_name: &str, output: &serde_json::Value) -> Option<ThreadEvent> {
        use crate::config::constants::tools;
        use crate::exec::events::tracker::TrackerSnapshot;

        if tool_name != tools::TASK_TRACKER {
            return None;
        }
        let checklist = output.get("checklist")?.as_object()?;
        let mut items = Vec::new();
        if let Some(raw_items) = checklist.get("items").and_then(serde_json::Value::as_array) {
            for (position, item) in raw_items.iter().enumerate() {
                if let Some(state) = tracker_item_state(item, position) {
                    items.push(state);
                }
            }
        }
        let (completed, total) = tracker_snapshot_counts(checklist, &items)?;
        let title = checklist
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Some(ThreadEvent::TrackerUpdated(Box::new(TrackerSnapshot { title, completed, total, items })))
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
            Vec::new(),
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
