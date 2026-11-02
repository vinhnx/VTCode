//! Bounded public command facts projected from the authoritative event stream.

use std::collections::{BTreeMap, BTreeSet};

use vtcode_core::exec::events::{CommandExecutionStatus, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus};
use vtcode_core::tools::{command_args, tool_intent};
use vtcode_webmcp::companion::{CommandEventReference, CommandResult, TaskSnapshot};

use super::public_text;

const MAX_RESULTS: usize = 32;
const MAX_BYTES: usize = 16 * 1024;
const MAX_COMMAND_BYTES: usize = 4096;
const MAX_ID_BYTES: usize = 256;

struct PendingCommand {
    command: Option<String>,
    command_truncated: bool,
    ambiguous: bool,
    tool_name: String,
    tool_call_id: Option<String>,
}

#[derive(Default)]
pub(super) struct CommandEvidence {
    thread_id: Option<String>,
    pending: BTreeMap<CommandEventReference, PendingCommand>,
    seen: BTreeSet<CommandEventReference>,
    bytes: usize,
}

impl CommandEvidence {
    pub(super) fn observe(&mut self, snapshot: &mut TaskSnapshot, event: &ThreadEvent) {
        match event {
            ThreadEvent::ThreadStarted(event) => {
                snapshot.evidence_truncated |= !self.pending.is_empty();
                self.pending.clear();
                self.thread_id = Some(event.thread_id.clone());
            }
            ThreadEvent::ThreadCompleted(_) => {
                snapshot.evidence_truncated |= !self.pending.is_empty();
                self.pending.clear();
            }
            ThreadEvent::ItemStarted(event) => self.invocation(snapshot, &event.item),
            ThreadEvent::ItemUpdated(event) => self.invocation(snapshot, &event.item),
            ThreadEvent::ItemCompleted(event) => {
                let item = &event.item;
                match &item.details {
                    ThreadItemDetails::ToolInvocation(_) => self.invocation(snapshot, item),
                    ThreadItemDetails::CommandExecution(command) => {
                        let Some(reference) = self.reference(snapshot, item, &item.id) else {
                            return;
                        };
                        let mut truncated = false;
                        let text = evidence_text(&command.command, MAX_COMMAND_BYTES, &mut truncated);
                        let status = match command.status {
                            CommandExecutionStatus::Completed => "completed",
                            CommandExecutionStatus::Failed => "failed",
                            CommandExecutionStatus::InProgress => "in_progress",
                        };
                        self.push(
                            snapshot,
                            CommandResult {
                                source: "command_execution".into(),
                                event_ref: reference,
                                tool_call_id: None,
                                tool_name: None,
                                command: Some(text),
                                command_truncated: truncated,
                                status: status.into(),
                                exit_code: command.exit_code,
                            },
                        );
                    }
                    ThreadItemDetails::ToolOutput(output) => {
                        let Some(key) = self.reference(snapshot, item, &output.call_id) else {
                            return;
                        };
                        let pending = self.pending.remove(&key);
                        // Unmatched exit-bearing outputs remain visible as incomplete
                        // observations. Unrelated non-command tool results are excluded.
                        if pending.is_none() && output.exit_code.is_none() {
                            return;
                        }
                        let Some(reference) = self.reference(snapshot, item, &item.id) else {
                            return;
                        };
                        if self.seen.contains(&reference) {
                            return;
                        }
                        let status = match output.status {
                            ToolCallStatus::Completed => "completed",
                            ToolCallStatus::Failed => "failed",
                            ToolCallStatus::InProgress => "in_progress",
                        };
                        let mut result = CommandResult {
                            source: "tool_output".into(),
                            event_ref: reference,
                            tool_call_id: output.tool_call_id.clone(),
                            tool_name: None,
                            command: None,
                            command_truncated: false,
                            status: status.into(),
                            exit_code: output.exit_code,
                        };
                        if let Some(pending) = pending {
                            let mismatched_call = pending
                                .tool_call_id
                                .as_ref()
                                .zip(output.tool_call_id.as_ref())
                                .is_some_and(|(a, b)| a != b);
                            if !mismatched_call {
                                result.command = pending.command;
                            }
                            result.command_truncated = pending.command_truncated;
                            result.tool_name = Some(pending.tool_name);
                        }
                        snapshot.evidence_truncated |= result.command.is_none();
                        self.push(snapshot, result);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn invocation(&mut self, snapshot: &mut TaskSnapshot, item: &ThreadItem) {
        let ThreadItemDetails::ToolInvocation(invocation) = &item.details else {
            return;
        };
        if !tool_intent::is_command_session_tool(&invocation.tool_name) {
            return;
        }
        let Some(key) = self.reference(snapshot, item, &item.id) else {
            return;
        };
        if invocation.tool_name.len() > MAX_ID_BYTES
            || invocation.tool_call_id.as_ref().is_some_and(|id| id.len() > MAX_ID_BYTES)
        {
            snapshot.evidence_truncated = true;
            return;
        }
        let mut truncated = false;
        let command = invocation
            .arguments
            .as_ref()
            .and_then(command_args::raw_command_text)
            .map(|command| evidence_text(&command, MAX_COMMAND_BYTES, &mut truncated));
        // The shared raw extractor deliberately excludes separately appended
        // argv. Withhold completeness credit when those arguments are present.
        truncated |= invocation
            .arguments
            .as_ref()
            .and_then(|args| args.get("args"))
            .is_some_and(|args| args.as_array().is_none_or(|args| !args.is_empty()));
        if let Some(pending) = self.pending.get_mut(&key) {
            // Started/updated/completed copies may fill initially absent args.
            // Conflicting identities never get guessed into a successful record.
            if pending.tool_name != invocation.tool_name
                || pending
                    .tool_call_id
                    .as_ref()
                    .zip(invocation.tool_call_id.as_ref())
                    .is_some_and(|(a, b)| a != b)
                || pending.command.as_ref().zip(command.as_ref()).is_some_and(|(a, b)| a != b)
            {
                pending.ambiguous = true;
                pending.command = None;
                snapshot.evidence_truncated = true;
            } else if !pending.ambiguous && command.is_some() {
                pending.command = command;
                pending.command_truncated |= truncated;
            }
            if pending.tool_call_id.is_none() {
                pending.tool_call_id = invocation.tool_call_id.clone();
            }
            return;
        }
        if self.pending.len() >= MAX_RESULTS {
            snapshot.evidence_truncated = true;
            return;
        }
        self.pending.insert(
            key,
            PendingCommand {
                command,
                command_truncated: truncated,
                ambiguous: false,
                tool_name: invocation.tool_name.clone(),
                tool_call_id: invocation.tool_call_id.clone(),
            },
        );
    }

    fn reference(&self, snapshot: &mut TaskSnapshot, item: &ThreadItem, id: &str) -> Option<CommandEventReference> {
        let context = item.context.as_deref();
        let reference = CommandEventReference {
            thread_id: self.thread_id.clone(),
            runtime_task_id: context.map(|c| c.task_id.clone()),
            actor_id: context.map(|c| c.actor_id.clone()),
            turn_id: context.map(|c| c.turn_id.clone()),
            item_id: id.into(),
        };
        if [
            &reference.thread_id,
            &reference.runtime_task_id,
            &reference.actor_id,
            &reference.turn_id,
        ]
        .into_iter()
        .flatten()
        .any(|value| value.len() > MAX_ID_BYTES)
            || id.len() > MAX_ID_BYTES
        {
            snapshot.evidence_truncated = true;
            return None;
        }
        Some(reference)
    }

    fn push(&mut self, snapshot: &mut TaskSnapshot, mut result: CommandResult) {
        if self.seen.contains(&result.event_ref) {
            return;
        }
        if snapshot.command_results.len() >= MAX_RESULTS {
            snapshot.evidence_truncated = true;
            return;
        }
        let raw_reference = result.event_ref.clone();
        // Runtime task IDs are generated as task-<UUID>. The generic secret
        // scrubber otherwise mistakes their embedded "sk-" for an API key.
        if let Some(id) = &mut result.event_ref.runtime_task_id {
            let canonical = id
                .strip_prefix("task-")
                .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok_and(|uuid| uuid.to_string() == suffix));
            if !canonical {
                *id = evidence_text(id, MAX_ID_BYTES, &mut snapshot.evidence_truncated);
            }
        }
        for text in [
            &mut result.event_ref.thread_id,
            &mut result.event_ref.actor_id,
            &mut result.event_ref.turn_id,
            &mut result.tool_call_id,
            &mut result.tool_name,
        ]
        .into_iter()
        .flatten()
        {
            *text = evidence_text(text, MAX_ID_BYTES, &mut snapshot.evidence_truncated);
        }
        result.event_ref.item_id =
            evidence_text(&result.event_ref.item_id, MAX_ID_BYTES, &mut snapshot.evidence_truncated);
        let Ok(encoded) = serde_json::to_vec(&result) else {
            snapshot.evidence_truncated = true;
            return;
        };
        let cost = encoded.len().saturating_add(1); // Array separator; reserve brackets too.
        if cost > MAX_BYTES.saturating_sub(self.bytes).saturating_sub(2) {
            snapshot.evidence_truncated = true;
            return;
        }
        snapshot.evidence_truncated |= result.command_truncated;
        self.bytes += cost;
        self.seen.insert(raw_reference);
        snapshot.command_results.push(result);
    }
}

/// Withheld command or reference data must not look like a complete proof.
fn evidence_text(text: &str, limit: usize, incomplete: &mut bool) -> String {
    let clean = public_text(text, limit, incomplete);
    *incomplete |= clean != text;
    clean
}
