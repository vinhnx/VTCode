use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use vtcode_exec_events::{
    CommandActivity, CommandExecutionStatus, HarnessEventKind, PatchApplyStatus, ThreadItem, ThreadItemDetails,
    ToolCallStatus, ToolOutcome,
};

#[derive(PartialEq, Eq)]
struct FileSignals {
    line: Option<u64>,
    reasons: Vec<(ReviewPriority, &'static str)>,
}

struct ItemState {
    item: ThreadItem,
    reference: EvidenceRef,
    position: u64,
    first_position: u64,
    started_position: Option<u64>,
    completed: bool,
    file_signals: BTreeMap<String, FileSignals>,
    diff_unavailable: bool,
}

#[derive(Hash, PartialEq, Eq)]
struct OutputIdentity<'a> {
    task_id: Option<&'a str>,
    actor_id: Option<&'a str>,
    call_id: &'a str,
}

fn output_identity<'a>(item: &'a ThreadItem, call_id: &'a str) -> OutputIdentity<'a> {
    OutputIdentity {
        task_id: item.context.as_ref().map(|c| c.task_id.as_str()),
        actor_id: item.context.as_ref().map(|c| c.actor_id.as_str()),
        call_id,
    }
}

pub(super) struct Reducer {
    model: ExplanationModel,
    items: HashMap<String, ItemState>,
    current_task: Option<String>,
    statuses: BTreeMap<Option<String>, String>,
    usages: BTreeMap<Option<String>, Usage>,
}

impl Reducer {
    pub fn new(session_id: String, scope: ExplanationScope) -> Self {
        Self {
            model: ExplanationModel {
                session_id,
                revision: String::new(),
                scope,
                task_id: None,
                status: "unavailable".into(),
                goals: vec![],
                actions: vec![],
                changes: vec![],
                edit_operations: 0,
                decisions: vec![],
                verification: vec![],
                token_breakdowns: vec![],
                failures: vec![],
                review_priorities: vec![],
                plan_evolution: vec![],
                graph: vec![],
                usage: None,
                cost_usd: None,
                completeness: EvidenceCompleteness::default(),
            },
            items: HashMap::new(),
            current_task: None,
            statuses: BTreeMap::new(),
            usages: BTreeMap::new(),
        }
    }

    pub fn malformed(&mut self) {
        self.model.completeness.malformed_records += 1;
    }

    pub fn push(&mut self, event: ThreadEvent, reference: EvidenceRef) {
        match event {
            ThreadEvent::TurnStarted(e) => {
                self.current_task = e.context.as_ref().map(|c| public_identity(&c.task_id));
                self.model.task_id.clone_from(&self.current_task);
                self.statuses.insert(self.current_task.clone(), "running".into());
                if let Some(breakdown) = e.token_breakdown() {
                    let mut fact = self.fact("Recorded request-prefix tokens", "recorded", reference.clone());
                    if let Some(context) = &e.context {
                        fact.timestamp = Some(public_text(&context.timestamp, 128));
                        fact.actor_id = Some(public_identity(&context.actor_id));
                        fact.parent_actor_id = context.parent_actor_id.as_deref().map(public_identity);
                    }
                    self.model
                        .token_breakdowns
                        .push(TokenBreakdownEntry { fact, breakdown: *breakdown });
                }
                if let Some(c) = &e.context {
                    if !self.model.goals.iter().any(|g| g.task_id == self.current_task) {
                        let mut fact =
                            self.fact(c.goal.as_deref().unwrap_or("Goal unavailable"), "recorded", reference);
                        fact.timestamp = Some(public_text(&c.timestamp, 128));
                        fact.actor_id = Some(public_identity(&c.actor_id));
                        fact.parent_actor_id = c.parent_actor_id.as_deref().map(public_identity);
                        self.model.goals.push(fact);
                    }
                } else {
                    self.model.completeness.legacy_records += 1;
                }
            }
            ThreadEvent::TurnCompleted(e) => {
                let status = if e.in_progress_exec_sessions.is_empty() {
                    "turn completed"
                } else {
                    "commands pending"
                };
                self.statuses.insert(self.current_task.clone(), status.into());
                self.usages.entry(self.current_task.clone()).or_default().add(&e.usage);
            }
            ThreadEvent::TurnFailed(e) => {
                self.statuses.insert(self.current_task.clone(), "failed".into());
                let fact = self.fact(&e.message, "failed", reference);
                self.model.failures.push(fact);
                if let Some(usage) = e.usage {
                    self.usages.entry(self.current_task.clone()).or_default().add(&usage);
                }
            }
            ThreadEvent::TurnBlocked(e) => {
                self.statuses.insert(self.current_task.clone(), "blocked".into());
                let fact = self.fact(&e.message, "blocked", reference);
                self.model.failures.push(fact);
            }
            ThreadEvent::ThreadCompleted(e) => {
                if self.model.scope == ExplanationScope::Session {
                    self.model.cost_usd = e.total_cost_usd;
                    self.model.status = public_text(&e.outcome_code, 128);
                    self.model.usage = Some(e.usage);
                }
            }
            ThreadEvent::ItemStarted(e) => self.item(e.item, reference, false, true),
            ThreadEvent::ItemUpdated(e) => self.item(e.item, reference, false, false),
            ThreadEvent::ItemCompleted(e) => self.item(e.item, reference, true, false),
            ThreadEvent::PlanApprovalRequested(_) => {
                let fact = self.fact("Plan approval requested", "waiting", reference);
                self.model.plan_evolution.push(fact);
            }
            ThreadEvent::PlanApprovalResolved(e) => {
                let fact = self.fact(
                    &format!("Plan approval: {:?}{}", e.decision, if e.automatic { " (policy)" } else { " (user)" }),
                    "recorded",
                    reference,
                );
                self.model.plan_evolution.push(fact);
            }
            ThreadEvent::Interjected(e) => {
                let fact = self.fact(
                    &format!(
                        "Active-task correction: {}",
                        e.text.as_deref().map(String::as_str).unwrap_or("text unavailable")
                    ),
                    "recorded",
                    reference,
                );
                self.model.plan_evolution.push(fact);
            }
            ThreadEvent::Error(e) => {
                let fact = self.fact(&e.message, "failed", reference);
                self.model.failures.push(fact);
            }
            ThreadEvent::Unknown => self.model.completeness.unknown_records += 1,
            _ => {}
        }
    }

    fn fact(&self, label: &str, status: &str, evidence: EvidenceRef) -> ExplanationEntry {
        ExplanationEntry {
            label: public_text(label, 1000),
            status: status.into(),
            evidence,
            task_id: self.current_task.clone(),
            actor_id: None,
            parent_actor_id: None,
            timestamp: None,
            path: None,
            line: None,
        }
    }

    fn item(&mut self, mut item: ThreadItem, mut reference: EvidenceRef, completed: bool, started: bool) {
        item.id = public_identity(&item.id);
        if let Some(context) = &mut item.context {
            context.task_id = public_identity(&context.task_id);
            context.turn_id = public_identity(&context.turn_id);
            context.actor_id = public_identity(&context.actor_id);
            context.parent_actor_id = context.parent_actor_id.as_deref().map(public_identity);
            context.timestamp = public_text(&context.timestamp, 128);
        } else {
            self.model.completeness.legacy_records += 1;
        }
        let mut file_signals = BTreeMap::new();
        let mut diff_unavailable = false;
        match &mut item.details {
            ThreadItemDetails::ToolOutput(o) => {
                o.output = String::new();
                o.spool_path = None;
                o.call_id = public_identity(&o.call_id);
                o.tool_call_id = o.tool_call_id.as_deref().map(public_identity);
            }
            ThreadItemDetails::CommandExecution(c) => {
                c.aggregated_output = String::new();
                c.arguments = None;
                c.command = public_text(&c.command, 1000);
            }
            ThreadItemDetails::ToolInvocation(t) => {
                let label = command_label(&t.tool_name, t.arguments.as_ref());
                t.arguments = Some(serde_json::json!({"cmd":public_text(&label, 1000)}));
                t.tool_name = public_text(&t.tool_name, 256);
                t.tool_call_id = t.tool_call_id.as_deref().map(public_identity);
            }
            ThreadItemDetails::FileChange(c) => {
                diff_unavailable = c.diff_incomplete == Some(true);
                let single_file = c.changes.len() == 1;
                for change in &mut c.changes {
                    let file_diff = c.unified_diff.as_deref().and_then(|d| diff_for_file(d, &change.path));
                    diff_unavailable |=
                        file_diff.is_none() || file_diff.as_deref().is_some_and(|d| d.contains("[truncated"));
                    let line = file_diff.as_deref().and_then(|d| hunk_line(d, &change.path));
                    let reasons = review_reasons(
                        &change.path,
                        file_diff.as_deref(),
                        (single_file && file_diff.is_none())
                            .then(|| c.additions.zip(c.deletions).map(|(a, d)| a.saturating_add(d)))
                            .flatten(),
                    );
                    change.path = public_path(&change.path);
                    file_signals.insert(change.path.clone(), FileSignals { line, reasons });
                }
                c.unified_diff = None;
            }
            ThreadItemDetails::AgentMessage(_)
            | ThreadItemDetails::Reasoning(_)
            | ThreadItemDetails::WebSearch(_)
            | ThreadItemDetails::McpToolCall(_) => return,
            ThreadItemDetails::Plan(p) => p.text = public_text(&p.text, 1000),
            ThreadItemDetails::Harness(h) => {
                h.message = h.message.as_ref().map(|s| public_text(s, 1000));
                h.command = None;
                h.path = None;
                h.archive_path = None;
                h.transcript_path = None;
                h.error_category = h.error_category.as_deref().map(|s| public_text(s, 128));
                h.task_id = h.task_id.as_deref().map(public_identity);
                h.session_id = h.session_id.as_deref().map(public_identity);
                h.exec_session_id = h.exec_session_id.as_deref().map(public_identity);
                h.status = h.status.as_deref().map(|s| public_text(s, 128));
            }
            ThreadItemDetails::Error(e) => e.message = public_text(&e.message, 1000),
            ThreadItemDetails::Decision(d) => {
                d.summary = public_text(&d.summary, 240);
                d.rationale = public_text(&d.rationale, 1000);
                d.alternatives = d.alternatives.iter().take(3).map(|a| public_text(a, 1000)).collect();
                d.evidence_ids.truncate(8);
                for id in &mut d.evidence_ids {
                    *id = public_identity(id);
                }
            }
        }
        reference.item_id = Some(item.id.clone());
        let task = item
            .context
            .as_ref()
            .map(|c| c.task_id.as_str())
            .or(self.current_task.as_deref())
            .unwrap_or("legacy");
        let actor = item.context.as_ref().map_or("legacy", |c| c.actor_id.as_str());
        let key = format!("{task}\0{actor}\0{}", item.id);
        if let Some(old) = self.items.get(&key) {
            // A late nonterminal alias cannot regress a terminal lifecycle.
            if old.completed && item_is_terminal(&old.item.details) && (!completed || !item_is_terminal(&item.details))
            {
                return;
            }
            // Replayed terminal aliases retain the original evidence and order.
            if old.completed
                && completed
                && old.item.details == item.details
                && old.item.context.as_ref().and_then(|c| c.activity) == item.context.as_ref().and_then(|c| c.activity)
                && old.file_signals == file_signals
            {
                return;
            }
        }
        let first_position = self.items.get(&key).map_or(reference.offset, |old| old.first_position);
        let started_position = self
            .items
            .get(&key)
            .and_then(|old| old.started_position)
            .or(started.then_some(reference.offset));
        self.items.insert(
            key,
            ItemState {
                position: reference.offset,
                first_position,
                started_position,
                item,
                reference,
                completed,
                file_signals,
                diff_unavailable,
            },
        );
    }

    pub fn finish(mut self, revision: String, evicted_turns: u64) -> ExplanationModel {
        self.model.revision = revision;
        self.model.completeness.evicted_turns = evicted_turns;
        let selected = self.model.task_id.clone();
        let in_scope = |task: &Option<String>| self.model.scope == ExplanationScope::Session || *task == selected;
        self.model.goals.retain(|e| in_scope(&e.task_id));
        self.model.plan_evolution.retain(|e| in_scope(&e.task_id));
        self.model.failures.retain(|e| in_scope(&e.task_id));
        self.model.token_breakdowns.retain(|entry| in_scope(&entry.fact.task_id));
        if self.model.status == "unavailable" {
            self.model.status = self.statuses.get(&selected).cloned().unwrap_or_else(|| "unavailable".into());
        }
        if self.model.usage.is_none() {
            for (task, usage) in &self.usages {
                if in_scope(task) {
                    self.model.usage.get_or_insert_default().add(usage);
                }
            }
        }
        let mut items: Vec<_> = self
            .items
            .into_values()
            .filter(|state| {
                let task = state.item.context.as_ref().map(|c| c.task_id.clone());
                self.model.scope == ExplanationScope::Session || task == selected
            })
            .collect();
        items.sort_by_key(|s| s.position);
        let outputs: HashMap<_, _> = items
            .iter()
            .filter_map(|s| {
                if let ThreadItemDetails::ToolOutput(o) = &s.item.details {
                    Some((output_identity(&s.item, &o.call_id), s))
                } else {
                    None
                }
            })
            .collect();
        let mut output_started_offsets = HashMap::new();
        for state in &items {
            if let ThreadItemDetails::ToolOutput(output) = &state.item.details
                && let Some(started) = state.started_position
            {
                output_started_offsets
                    .entry(output_identity(&state.item, &output.call_id))
                    .and_modify(|old: &mut u64| *old = (*old).min(started))
                    .or_insert(started);
            }
        }
        let mut last_mutation: Option<u64> = None;
        let mut mutation_fact = None;
        let mut last_action: Option<String> = None;
        let mut files = BTreeMap::new();
        let mut unavailable_diffs = 0usize;
        let mut verification_start_offsets = Vec::new();
        for state in &items {
            let mut fact = ExplanationEntry {
                label: String::new(),
                status: if state.completed { "completed" } else { "pending" }.into(),
                evidence: state.reference.clone(),
                task_id: state.item.context.as_ref().map(|c| c.task_id.clone()),
                actor_id: state.item.context.as_ref().map(|c| c.actor_id.clone()),
                parent_actor_id: state.item.context.as_ref().and_then(|c| c.parent_actor_id.clone()),
                timestamp: state.item.context.as_ref().map(|c| c.timestamp.clone()),
                path: None,
                line: None,
            };
            let mut node_id = graph_node_id(&fact);
            match &state.item.details {
                ThreadItemDetails::ToolInvocation(t) => {
                    fact.label = public_text(&command_label(&t.tool_name, t.arguments.as_ref()), 1000);
                    let identity = output_identity(&state.item, &state.item.id);
                    let output = outputs.get(&identity);
                    let payload = output.and_then(|s| {
                        if let ThreadItemDetails::ToolOutput(o) = &s.item.details {
                            Some(o)
                        } else {
                            None
                        }
                    });
                    let successful = state.completed
                        && t.status == ToolCallStatus::Completed
                        && t.outcome.unwrap_or(ToolOutcome::Success) == ToolOutcome::Success
                        && payload.is_none_or(|o| o.status == ToolCallStatus::Completed)
                        && output.is_none_or(|o| o.completed);
                    let pending = !state.completed
                        || t.status == ToolCallStatus::InProgress
                        || t.outcome == Some(ToolOutcome::Followup)
                        || payload.is_some_and(|o| o.status == ToolCallStatus::InProgress)
                        || output.is_some_and(|o| !o.completed);
                    fact.status = if pending {
                        "pending"
                    } else if t.outcome == Some(ToolOutcome::Cancelled) {
                        "cancelled"
                    } else if !successful {
                        "failed or denied"
                    } else {
                        "completed"
                    }
                    .into();
                    let activity = state.item.context.as_ref().and_then(|c| c.activity);
                    if activity == Some(CommandActivity::Verification) {
                        let exit_code = payload.and_then(|o| o.exit_code);
                        let verified = successful
                            && output.is_some_and(|o| o.completed)
                            && payload.is_some_and(|o| o.status == ToolCallStatus::Completed)
                            && exit_code == Some(0);
                        fact.status = if verified {
                            "passed"
                        } else if pending {
                            "pending"
                        } else if exit_code.is_none() && successful {
                            "unconfirmed"
                        } else {
                            "failed or denied"
                        }
                        .into();
                        if let Some(source) = output {
                            fact.evidence = source.reference.clone();
                            node_id = graph_node_id(&fact);
                        }
                        self.model
                            .verification
                            .push(VerificationEntry { fact: fact.clone(), exit_code, fresh: false });
                        verification_start_offsets
                            .push(output_started_offsets.get(&identity).copied().unwrap_or(state.first_position));
                    }
                    if activity == Some(CommandActivity::Mutation)
                        && matches!(
                            t.outcome,
                            None | Some(ToolOutcome::Success | ToolOutcome::Error | ToolOutcome::Cancelled)
                        )
                    {
                        let position = output.map_or(state.position, |o| o.position).max(state.position);
                        last_mutation = Some(last_mutation.map_or(position, |old| old.max(position)));
                        mutation_fact = Some(fact.clone());
                    }
                    if fact.status == "failed or denied" {
                        self.model.failures.push(fact.clone());
                    }
                    self.model.actions.push(fact);
                }
                ThreadItemDetails::CommandExecution(c) => {
                    fact.label = public_text(&c.command, 1000);
                    let passed =
                        state.completed && c.status == CommandExecutionStatus::Completed && c.exit_code == Some(0);
                    fact.status = if passed {
                        "passed"
                    } else if c.status == CommandExecutionStatus::InProgress || !state.completed {
                        "pending"
                    } else if c.status == CommandExecutionStatus::Failed {
                        "failed"
                    } else if c.exit_code.is_none() {
                        "unconfirmed"
                    } else {
                        "failed"
                    }
                    .into();
                    let activity = state.item.context.as_ref().and_then(|c| c.activity);
                    if activity == Some(CommandActivity::Verification) {
                        self.model.verification.push(VerificationEntry {
                            fact: fact.clone(),
                            exit_code: c.exit_code,
                            fresh: false,
                        });
                        verification_start_offsets.push(state.first_position);
                    }
                    if activity == Some(CommandActivity::Mutation) {
                        last_mutation = Some(last_mutation.map_or(state.position, |old| old.max(state.position)));
                        mutation_fact = Some(fact.clone());
                    }
                    if fact.status == "failed" {
                        self.model.failures.push(fact.clone());
                    }
                    self.model.actions.push(fact);
                }
                ThreadItemDetails::FileChange(change) => {
                    last_mutation = Some(last_mutation.map_or(state.position, |old| old.max(state.position)));
                    mutation_fact = Some(fact.clone());
                    if !state.completed || change.status != PatchApplyStatus::Completed {
                        fact.label = "File change did not complete".into();
                        if state.completed && change.status == PatchApplyStatus::Failed {
                            fact.status = "failed".into();
                            self.model.failures.push(fact.clone());
                        } else {
                            fact.status = "pending".into();
                        }
                        self.model.actions.push(fact);
                        continue;
                    }
                    self.model.edit_operations += 1;
                    unavailable_diffs += usize::from(state.diff_unavailable);
                    for file in &change.changes {
                        let mut f = fact.clone();
                        f.path = Some(public_text(&file.path, 1000));
                        f.label = public_text(&file.path, 1000);
                        f.status = format!("{:?}", file.kind).to_lowercase();
                        f.line = state.file_signals.get(&file.path).and_then(|s| s.line);
                        self.model.graph.push(GraphEdge {
                            from: node_id.clone(),
                            to: f.label.clone(),
                            relation: "changed file".into(),
                        });
                        for (priority, reason) in state
                            .file_signals
                            .get(&file.path)
                            .map(|s| s.reasons.clone())
                            .unwrap_or_default()
                        {
                            self.model.review_priorities.push(ReviewSignal {
                                priority,
                                reason: reason.into(),
                                fact: f.clone(),
                            });
                        }
                        files.insert(file.path.clone(), f);
                    }
                }
                ThreadItemDetails::Decision(d) => {
                    fact.label = public_text(&d.summary, 240);
                    fact.status = "agent-reported".into();
                    self.model.decisions.push(DecisionEntry {
                        fact,
                        rationale: public_text(&d.rationale, 1000),
                        alternatives: d.alternatives.iter().take(3).map(|a| public_text(a, 1000)).collect(),
                        evidence_ids: d.evidence_ids.iter().take(8).cloned().collect(),
                    });
                }
                ThreadItemDetails::Plan(plan) => {
                    fact.label = public_text(&plan.text, 1000);
                    self.model.plan_evolution.push(fact);
                }
                ThreadItemDetails::Harness(h) => {
                    fact.label = public_text(h.message.as_deref().unwrap_or("Harness event"), 1000);
                    if h.event == HarnessEventKind::DelegatedAgentStatus {
                        fact.status = h.status.clone().unwrap_or_else(|| "unconfirmed".into());
                        if let (Some(parent), Some(actor)) = (&fact.parent_actor_id, &fact.actor_id) {
                            self.model.graph.push(GraphEdge {
                                from: parent.clone(),
                                to: actor.clone(),
                                relation: "delegated agent".into(),
                            });
                        }
                        if fact.status == "failed" {
                            self.model.failures.push(fact.clone());
                        }
                    }
                    self.model.actions.push(fact);
                }
                ThreadItemDetails::Error(e) => {
                    fact.label = public_text(&e.message, 1000);
                    fact.status = "failed".into();
                    self.model.failures.push(fact.clone());
                    self.model.actions.push(fact);
                }
                _ => continue,
            }
            if let Some(previous) = last_action.replace(node_id.clone()) {
                self.model.graph.push(GraphEdge {
                    from: previous,
                    to: node_id,
                    relation: "recorded next".into(),
                });
            }
        }
        self.model.changes = files.into_values().collect();
        let unconfirmed = self.model.actions.iter().filter(|fact| fact.status == "unconfirmed").count();
        if unconfirmed > 0 {
            self.model.completeness.warnings.push(format!(
                "Exit evidence unavailable for {unconfirmed} recorded operations; verification remains unconfirmed"
            ));
        }
        if unavailable_diffs > 0 {
            self.model.completeness.warnings.push(format!(
                "Captured diff unavailable or truncated for {unavailable_diffs} file-change operations; API and size review signals may be incomplete"
            ));
        }
        for (verifier, started) in self.model.verification.iter_mut().zip(verification_start_offsets) {
            verifier.fresh =
                verifier.fact.status == "passed" && last_mutation.is_none_or(|position| started > position);
        }
        if !self.model.verification.iter().any(|v| v.fresh) {
            if let Some(fact) = self.model.changes.last().or(mutation_fact.as_ref()) {
                self.model.review_priorities.push(ReviewSignal {
                    priority: ReviewPriority::Medium,
                    reason: "No recorded successful verification after the last mutation".into(),
                    fact: fact.clone(),
                });
            }
        }
        if self.model.failures.len() >= 2 {
            if let Some(fact) = self.model.failures.last() {
                self.model.review_priorities.push(ReviewSignal {
                    priority: ReviewPriority::Medium,
                    reason: "Repeated recorded failures; inspect recovery evidence".into(),
                    fact: fact.clone(),
                });
            }
        }
        self.model.review_priorities.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.fact.path.cmp(&b.fact.path))
                .then_with(|| a.fact.line.cmp(&b.fact.line))
                .then_with(|| a.fact.evidence.offset.cmp(&b.fact.evidence.offset))
                .then_with(|| a.reason.cmp(&b.reason))
        });
        let mut seen_review_signals = BTreeSet::new();
        self.model.review_priorities.retain(|signal| {
            seen_review_signals.insert((signal.priority, signal.fact.path.clone(), signal.reason.clone()))
        });
        if self.model.goals.is_empty() {
            self.model
                .completeness
                .warnings
                .push("Task goal and identity unavailable in retained history".into());
        }
        if self.model.decisions.is_empty() {
            self.model
                .completeness
                .warnings
                .push("Public rationale unavailable: no recorded decisions".into());
        }
        self.model
    }
}

pub(super) fn graph_node_id(fact: &ExplanationEntry) -> String {
    format!(
        "event {}:{} [{} / {} / {}]",
        fact.evidence.offset,
        fact.evidence.digest,
        fact.task_id.as_deref().unwrap_or("legacy"),
        fact.actor_id.as_deref().unwrap_or("legacy"),
        fact.evidence.item_id.as_deref().unwrap_or("event")
    )
}

fn item_is_terminal(details: &ThreadItemDetails) -> bool {
    match details {
        ThreadItemDetails::ToolInvocation(tool) => {
            tool.status != ToolCallStatus::InProgress && tool.outcome.is_none_or(|outcome| outcome.is_terminal())
        }
        ThreadItemDetails::ToolOutput(output) => output.status != ToolCallStatus::InProgress,
        ThreadItemDetails::CommandExecution(command) => command.status != CommandExecutionStatus::InProgress,
        _ => true,
    }
}

fn command_label(tool: &str, args: Option<&serde_json::Value>) -> String {
    args.and_then(|a| a.get("cmd").or_else(|| a.get("command")))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(tool)
        .to_owned()
}

fn hunk_line(diff: &str, path: &str) -> Option<u64> {
    let mut matching = false;
    for line in diff.lines() {
        if let Some(file) = line.strip_prefix("+++ ") {
            matching = file.strip_prefix("b/").unwrap_or(file) == path;
        }
        if matching && line.starts_with("@@ ") {
            return line.split_whitespace().find_map(|s| {
                s.strip_prefix('+')
                    .and_then(|n| n.split(',').next())
                    .and_then(|n| n.parse().ok())
            });
        }
    }
    None
}

fn diff_for_file(diff: &str, path: &str) -> Option<String> {
    let mut section = String::new();
    let mut matching = false;
    let mut lines = diff.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("diff --git ") {
            matching = false;
        }
        if let Some(old_file) = line.strip_prefix("--- ")
            && let Some(new_file) = lines.peek().and_then(|line| line.strip_prefix("+++ "))
        {
            let file = if new_file == "/dev/null" {
                old_file.strip_prefix("a/").unwrap_or(old_file)
            } else {
                new_file.strip_prefix("b/").unwrap_or(new_file)
            };
            matching = file == path;
            if matching {
                section.push_str(line);
                section.push('\n');
                section.push_str(lines.next().unwrap_or_default());
                section.push('\n');
            } else {
                lines.next();
            }
            continue;
        }
        if matching {
            section.push_str(line);
            section.push('\n');
        }
    }
    (!section.is_empty()).then_some(section)
}

fn review_reasons(path: &str, diff: Option<&str>, changed_lines: Option<u64>) -> Vec<(ReviewPriority, &'static str)> {
    let lower = path.to_ascii_lowercase();
    let components: Vec<_> = lower.split(['/', '.', '-', '_']).collect();
    let mut reasons = vec![];
    if components.iter().any(|c| {
        matches!(
            *c,
            "security"
                | "safety"
                | "sandbox"
                | "permission"
                | "permissions"
                | "auth"
                | "authentication"
                | "credential"
                | "credentials"
                | "exec"
                | "execution"
                | "persistence"
                | "storage"
                | "schema"
                | "migration"
                | "migrations"
        )
    }) || lower.contains("event_log")
    {
        reasons.push((ReviewPriority::High, "Security, execution, persistence, or schema boundary changed"));
    }
    if matches!(
        lower.rsplit('/').next(),
        Some("cargo.toml" | "cargo.lock" | "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock")
    ) {
        reasons.push((ReviewPriority::High, "Dependency manifest or lockfile changed"));
    }
    let changed: Vec<_> = diff
        .unwrap_or_default()
        .lines()
        .skip_while(|line| !line.starts_with("@@ "))
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .collect();
    if changed.iter().any(|l| {
        l.contains("pub fn ")
            || l.contains("pub struct ")
            || l.contains("pub enum ")
            || l.contains("pub async fn ")
            || l.contains("export function ")
            || l.contains("export interface ")
    }) {
        reasons.push((ReviewPriority::Medium, "Public API-like diff signal; review compatibility"));
    }
    if changed_lines.unwrap_or(changed.len() as u64) >= 200 {
        reasons.push((ReviewPriority::Medium, "At least 200 changed lines recorded"));
    }
    reasons
}

#[cfg(test)]
mod payload_tests {
    use super::*;
    use vtcode_exec_events::{CommandExecutionItem, ItemCompletedEvent, ToolOutputItem};

    #[test]
    fn projection_drops_full_output_allocations_while_retaining_lifecycle_metadata() {
        let mut reducer = Reducer::new("session".into(), ExplanationScope::Session);
        let details = [
            ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "call".into(),
                tool_call_id: None,
                spool_path: None,
                output: "x".repeat(1024 * 1024),
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
            })),
            ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: "check".into(),
                arguments: None,
                aggregated_output: "x".repeat(1024 * 1024),
                exit_code: Some(0),
                status: CommandExecutionStatus::Completed,
            })),
        ];
        for (i, details) in details.into_iter().enumerate() {
            reducer.push(
                ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem { context: None, id: format!("i{i}"), details },
                }),
                EvidenceRef {
                    session_id: "session".into(),
                    offset: i as u64,
                    length: 1,
                    digest: "digest".into(),
                    item_id: None,
                },
            );
        }
        for state in reducer.items.values() {
            match &state.item.details {
                ThreadItemDetails::ToolOutput(output) => {
                    assert_eq!(output.output.capacity(), 0);
                    assert_eq!(output.exit_code, Some(0));
                }
                ThreadItemDetails::CommandExecution(command) => {
                    assert_eq!(command.aggregated_output.capacity(), 0);
                    assert_eq!(command.command, "check");
                }
                _ => panic!("unexpected fixture type"),
            }
        }
    }
}
