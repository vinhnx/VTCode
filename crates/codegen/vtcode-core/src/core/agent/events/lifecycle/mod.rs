use crate::exec::events::{
    AgentMessageItem, AppPreviewItem, AppUiPosture, ErrorItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent,
    ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem, ToolOutcome,
    ToolOutputItem, tool_outcome_from_status,
};
use crate::mcp::{MCP_APP_DISCLOSURE_KEY, MCP_APP_SUPPRESSED_KEY, McpAppDisclosure};
use crate::tools::file_ops::{canonical_diff_previews, diff_preview_user_message};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide source of `ThreadItem` handles.
///
/// `ThreadItem.id` is the handle every consumer keys on (transcript rows,
/// canonical memory projections, replay), so it must be unique across all
/// emitters in a session. A per-emitter counter is not enough: emitters are
/// created per recorder, per runtime and per streaming step, so each one
/// restarted at `item_0` and rebound an existing handle to a different item —
/// a dangling handle that silently overwrote an earlier row. One monotonic
/// counter keeps handles unique across emitters, turns and retries.
static NEXT_ITEM_HANDLE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default)]
struct StreamingTextState {
    item_id: Option<String>,
    text: String,
    started: bool,
}

#[derive(Debug, Clone)]
struct ToolCallStreamState {
    item_id: String,
    name: Option<String>,
    arguments: String,
    started: bool,
    /// Bytes of `arguments` included in the last emitted `item.updated` event.
    /// Used to throttle per-token delta emissions — see `MIN_TOOL_ARG_UPDATE_BYTES`.
    last_emitted_args_len: usize,
    /// Number of intermediate `item.updated` events emitted for this tool call.
    /// Capped at `MAX_TOOL_ARG_UPDATE_EVENTS` to bound log growth for large arguments.
    update_events: usize,
}

/// Minimum accumulated bytes of tool-call arguments between two `item.updated`
/// events. Without this, every streaming argument delta (one per token) emits a
/// full-arguments update, producing ~30+ events per tool call and bloating the
/// session log (observed 3069 `item.updated` events for 98 tool outputs in a
/// single 3-turn session). The final `complete_tool_call` always emits the full
/// arguments, so intermediate updates are progress hints only.
const MIN_TOOL_ARG_UPDATE_BYTES: usize = 512;

/// Maximum intermediate `item.updated` events per tool call. Once reached, no
/// further streaming updates are emitted until the tool call completes.
const MAX_TOOL_ARG_UPDATE_EVENTS: usize = 8;

mod summary;
pub use summary::tool_output_payload_from_value;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Aggregated output from a tool execution, containing either inline text or a
/// spool file reference.
pub struct ToolOutputPayload {
    /// Combined output text from the tool execution.
    pub aggregated_output: String,
    /// Optional path to a spool file containing the full output.
    pub spool_path: Option<String>,
    /// MCP Apps referenced by the result, mapped from execution-time disclosure.
    pub app_previews: Vec<AppPreviewItem>,
    /// App URIs refused by the `off` tier, mapped from the suppression marker.
    /// The runner emits a sandbox-denied event for these; empty otherwise.
    pub suppressed_apps: Vec<String>,
}

/// Attribute only a successful file tool's recorded result, never current Git state.
pub fn file_change_completed_event(
    call_id: &str,
    tool_name: &str,
    args: &Value,
    output: &Value,
) -> Option<ThreadEvent> {
    use crate::exec::events::{FileChangeItem, FileUpdateChange, PatchApplyStatus, PatchChangeKind};
    let file_tool =
        matches!(tool_name, "apply_patch" | "write_file" | "edit_file" | "create_file" | "delete_file" | "file")
            || tool_name == crate::config::constants::tools::UNIFIED_FILE;
    if !file_tool
        || !crate::tools::tool_intent::classify_tool_intent(tool_name, args).mutating
        || output.get("success").and_then(Value::as_bool) == Some(false)
        || output.get("skipped").and_then(Value::as_bool) == Some(true)
        || output.get("conflict").and_then(Value::as_bool) == Some(true)
        || crate::tools::file_ops::diff_output_has_effective_change(output) == Some(false)
    {
        return None;
    }
    let previews = canonical_diff_previews(output);
    let mut changes = Vec::new();
    let mut diffs = Vec::new();
    let mut additions = 0u64;
    let mut deletions = 0u64;
    let mut counts_available = true;
    let mut diff_incomplete = previews.is_empty();
    for preview in &previews {
        if preview.get("is_empty").and_then(Value::as_bool) == Some(true)
            && !matches!(preview.get("operation").and_then(Value::as_str), Some("created" | "deleted"))
        {
            continue;
        }
        let Some(path) = preview.get("path").and_then(Value::as_str) else {
            continue;
        };
        let kind = match preview.get("operation").and_then(Value::as_str) {
            Some("created" | "added") => PatchChangeKind::Add,
            Some("deleted" | "removed") => PatchChangeKind::Delete,
            _ => PatchChangeKind::Update,
        };
        changes.push(FileUpdateChange {
            path: path.to_owned(),
            kind,
            terminators_normalized: preview.get("normalized_terminators").and_then(Value::as_bool),
        });
        diff_incomplete |= preview.get("truncated").and_then(Value::as_bool) == Some(true)
            || preview.get("skipped").and_then(Value::as_bool) == Some(true)
            || preview.get("content").and_then(Value::as_str).is_none();
        if let Some(content) = preview.get("content").and_then(Value::as_str) {
            diffs.push(content.to_owned());
        }
        if let (Some(a), Some(d)) = (
            preview
                .get("additions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("additions")))
                .and_then(Value::as_u64),
            preview
                .get("deletions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("deletions")))
                .and_then(Value::as_u64),
        ) {
            additions = additions.saturating_add(a);
            deletions = deletions.saturating_add(d);
        } else {
            counts_available = false;
        }
    }
    if changes.is_empty()
        && let Some(paths) = output.get("modified_files").and_then(Value::as_array)
    {
        counts_available = false;
        for path in paths.iter().filter_map(Value::as_str) {
            changes.push(FileUpdateChange {
                path: path.to_owned(),
                kind: PatchChangeKind::Update,
                terminators_normalized: None,
            });
        }
    }
    if changes.is_empty() {
        return None;
    }
    Some(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: format!("{call_id}:changes"),
            context: None,
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                diff_incomplete: diff_incomplete.then_some(true),
                changes,
                status: PatchApplyStatus::Completed,
                unified_diff: (!diffs.is_empty()).then(|| diffs.join("\n")),
                additions: counts_available.then_some(additions),
                deletions: counts_available.then_some(deletions),
            })),
        },
    }))
}

/// Shared lifecycle state for assistant text, reasoning, and model-emitted tool calls.
#[derive(Debug, Default)]
pub struct SharedLifecycleEmitter {
    assistant: StreamingTextState,
    reasoning: StreamingTextState,
    reasoning_stage: Option<String>,
    tool_calls: HashMap<String, ToolCallStreamState>,
    pending_events: Vec<ThreadEvent>,
}

impl SharedLifecycleEmitter {
    /// Generate the next unique item ID for lifecycle events.
    ///
    /// Takes `&self` because the counter is process-wide: uniqueness must not
    /// depend on which emitter instance mints the handle.
    #[must_use]
    pub fn next_item_id(&self) -> String {
        format!("item_{}", NEXT_ITEM_HANDLE.fetch_add(1, Ordering::Relaxed))
    }

    /// Emit a completed agent message event with the full text.
    pub fn emit_completed_agent_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
            },
        }));
    }

    /// Replace the current assistant streaming text. Returns `true` if the text changed.
    pub fn replace_assistant_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.assistant, text)
    }

    /// Whether the assistant text stream has been started.
    #[must_use]
    pub fn assistant_started(&self) -> bool {
        self.assistant.started
    }

    /// Length of the accumulated assistant text in bytes.
    #[must_use]
    pub fn assistant_len(&self) -> usize {
        self.assistant.text.len()
    }

    /// Append a delta to the assistant text stream. Returns `true` if content was added.
    pub fn append_assistant_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.assistant, delta)
    }

    /// Emit a snapshot of the current assistant text as an item event.
    pub fn emit_assistant_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        emit_text_snapshot(&mut self.pending_events, &mut self.assistant, item_id, |text| {
            ThreadItemDetails::AgentMessage(AgentMessageItem { text })
        })
    }

    /// Complete the assistant text stream, emitting a final completed event.
    pub fn complete_assistant_stream(&mut self) -> bool {
        complete_text_stream(&mut self.pending_events, &mut self.assistant, |text| {
            ThreadItemDetails::AgentMessage(AgentMessageItem { text })
        })
    }

    /// Emit a completed reasoning event with the full text.
    pub fn emit_completed_reasoning(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    diagnostic: None,
                    text: text.to_string(),
                    stage: self.reasoning_stage.clone(),
                })),
            },
        }));
    }

    /// Replace the current reasoning streaming text. Returns `true` if the text changed.
    pub fn replace_reasoning_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.reasoning, text)
    }

    /// Append a delta to the reasoning text stream. Returns `true` if content was added.
    pub fn append_reasoning_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.reasoning, delta)
    }

    /// Update the reasoning stage label. Returns `true` if the stage changed.
    pub fn set_reasoning_stage(&mut self, stage: Option<String>) -> bool {
        if self.reasoning_stage == stage {
            return false;
        }
        self.reasoning_stage = stage;
        true
    }

    /// Length of the accumulated reasoning text in bytes.
    #[must_use]
    pub fn reasoning_len(&self) -> usize {
        self.reasoning.text.len()
    }

    /// Whether the reasoning text stream has been started.
    #[must_use]
    pub fn reasoning_started(&self) -> bool {
        self.reasoning.started
    }

    /// Emit a snapshot of the current reasoning text as an item event.
    pub fn emit_reasoning_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let stage = self.reasoning_stage.clone();
        emit_text_snapshot(&mut self.pending_events, &mut self.reasoning, item_id, move |text| {
            ThreadItemDetails::Reasoning(Box::new(ReasoningItem { diagnostic: None, text, stage: stage.clone() }))
        })
    }

    /// Emit an update event reflecting the current reasoning stage.
    pub fn emit_reasoning_stage_update(&mut self) -> bool {
        if !self.reasoning.started {
            return false;
        }
        let Some(item_id) = self.reasoning.item_id.clone() else {
            return false;
        };
        self.pending_events.push(ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    diagnostic: None,
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                })),
            },
        }));
        true
    }

    /// Complete the reasoning text stream, emitting a final completed event.
    pub fn complete_reasoning_stream(&mut self) -> bool {
        let stage = self.reasoning_stage.clone();
        complete_text_stream(&mut self.pending_events, &mut self.reasoning, move |text| {
            ThreadItemDetails::Reasoning(Box::new(ReasoningItem { diagnostic: None, text, stage: stage.clone() }))
        })
    }

    /// Start tracking a tool call, emitting an item-started event.
    pub fn start_tool_call(&mut self, call_id: &str, tool_name: Option<String>, item_id: Option<String>) -> bool {
        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }
        if buffer.started {
            return false;
        }

        buffer.started = true;
        self.pending_events.push(tool_started_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            None,
            Some(call_id),
        ));
        true
    }

    /// Append an argument delta to an in-progress tool call.
    pub fn append_tool_call_delta(
        &mut self,
        call_id: &str,
        delta: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        if delta.is_empty() {
            return false;
        }

        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if !buffer.started {
            buffer.started = true;
            if buffer.name.is_none() {
                buffer.name = tool_name;
            }
            self.pending_events.push(tool_started_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                None,
                Some(call_id),
            ));
        } else if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        buffer.arguments.push_str(delta);
        // Throttle intermediate `item.updated` events: emit the first delta
        // eagerly (so the UI shows progress), then only when enough new bytes
        // have accumulated and the per-call cap hasn't been reached. The final
        // `complete_tool_call` always emits the full arguments regardless.
        let new_len = buffer.arguments.len();
        let should_emit = buffer.update_events == 0
            || (buffer.update_events < MAX_TOOL_ARG_UPDATE_EVENTS
                && new_len.saturating_sub(buffer.last_emitted_args_len) >= MIN_TOOL_ARG_UPDATE_BYTES);
        if should_emit {
            buffer.last_emitted_args_len = new_len;
            buffer.update_events += 1;
            let arguments = progress_tool_arguments(&buffer.arguments);
            self.pending_events.push(tool_invocation_updated_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                Some(&arguments),
                Some(call_id),
                ToolCallStatus::InProgress,
            ));
        }
        true
    }

    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus, outcome: Option<ToolOutcome>) -> bool {
        let Some(buffer) = self.tool_calls.remove(call_id) else {
            return false;
        };
        if !buffer.started {
            return false;
        }

        let arguments = if buffer.arguments.is_empty() {
            None
        } else {
            Some(progress_tool_arguments(&buffer.arguments))
        };
        let resolved_outcome = outcome.unwrap_or_else(|| tool_outcome_from_status(&status));
        self.pending_events.push(tool_invocation_completed_event(
            buffer.item_id,
            buffer.name.as_deref().unwrap_or_default(),
            arguments.as_ref(),
            Some(call_id),
            status,
            resolved_outcome,
        ));
        true
    }

    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<&str> {
        self.tool_calls.get(call_id).map(|buffer| buffer.item_id.as_str())
    }

    pub fn sync_tool_call_arguments(
        &mut self,
        call_id: &str,
        arguments: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        if !buffer.started {
            buffer.started = true;
            self.pending_events.push(tool_started_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                None,
                Some(call_id),
            ));
        }

        if buffer.arguments == arguments {
            return false;
        }

        buffer.arguments.clear();
        buffer.arguments.push_str(arguments);
        let args = progress_tool_arguments(&buffer.arguments);
        self.pending_events.push(tool_invocation_updated_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            Some(&args),
            Some(call_id),
            ToolCallStatus::InProgress,
        ));
        buffer.last_emitted_args_len = buffer.arguments.len();
        buffer.update_events = buffer.update_events.saturating_add(1);
        true
    }

    pub fn complete_open_items(&mut self) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(ToolCallStatus::Completed);
    }

    pub fn complete_open_text_items(&mut self) {
        let _ = self.complete_assistant_stream();
        let _ = self.complete_reasoning_stream();
    }

    pub fn complete_open_items_with_tool_status(&mut self, status: ToolCallStatus) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(status);
    }

    pub fn complete_open_tool_calls_with_status(&mut self, status: ToolCallStatus) {
        let call_ids = self.tool_calls.keys().cloned().collect::<Vec<_>>();
        for call_id in call_ids {
            let _ = self.complete_tool_call(&call_id, status.clone(), None);
        }
    }

    /// Emit a final `item.updated` carrying the full accumulated arguments for
    /// each open tool call, bypassing the intermediate-update throttle.
    ///
    /// Tool calls remain open (not completed); callers complete them via
    /// [`Self::complete_tool_call`] when execution finishes. This guarantees
    /// the authoritative streamed arguments are visible after streaming ends
    /// even when intermediate deltas were throttled (e.g. small tool calls
    /// whose completing delta fell below the byte threshold). Updates that
    /// would carry the same length as the last emitted snapshot are skipped
    /// to avoid redundant events for large tool calls whose last intermediate
    /// update already captured the full arguments.
    pub fn flush_open_tool_call_arguments(&mut self) {
        let call_ids: Vec<String> = self.tool_calls.keys().cloned().collect();
        for call_id in call_ids {
            let Some(buffer) = self.tool_calls.get_mut(&call_id) else {
                continue;
            };
            if buffer.arguments.is_empty() || buffer.last_emitted_args_len == buffer.arguments.len() {
                continue;
            }
            let arguments = progress_tool_arguments(&buffer.arguments);
            self.pending_events.push(tool_invocation_updated_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                Some(&arguments),
                Some(&call_id),
                ToolCallStatus::InProgress,
            ));
            buffer.last_emitted_args_len = buffer.arguments.len();
        }
    }

    #[must_use]
    pub fn drain_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.pending_events)
    }
}

fn replace_stream_text(state: &mut StreamingTextState, text: &str) -> bool {
    if state.text == text {
        return false;
    }
    state.text.clear();
    state.text.push_str(text);
    true
}

fn append_stream_delta(state: &mut StreamingTextState, delta: &str) -> bool {
    if delta.is_empty() {
        return false;
    }
    state.text.push_str(delta);
    true
}

fn emit_text_snapshot(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    item_id: String,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if state.text.trim().is_empty() {
        return false;
    }

    let item_id = state.item_id.get_or_insert(item_id).clone();
    let item = ThreadItem {
        context: None,
        id: item_id,
        details: build_details(state.text.clone()),
    };

    if state.started {
        pending_events.push(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
    } else {
        state.started = true;
        pending_events.push(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
    }
    true
}

fn complete_text_stream(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if !state.started {
        return false;
    }

    let Some(item_id) = state.item_id.take() else {
        state.started = false;
        state.text.clear();
        return false;
    };

    state.started = false;
    let text = std::mem::take(&mut state.text);
    pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: item_id,
            details: build_details(text),
        },
    }));
    true
}

#[must_use]
pub fn tool_output_item_id(call_item_id: &str) -> String {
    format!("{call_item_id}:output")
}

fn tool_invocation_item(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    outcome: Option<ToolOutcome>,
) -> ThreadItem {
    ThreadItem {
        context: None,
        id: item_id,
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
            status,
            outcome,
        })),
    }
}

fn tool_output_item(
    call_item_id: &str,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
    app_previews: Vec<AppPreviewItem>,
) -> ThreadItem {
    ThreadItem {
        context: None,
        id: tool_output_item_id(call_item_id),
        details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: call_item_id.to_string(),
            tool_call_id: tool_call_id.map(str::to_string),
            spool_path: spool_path.map(str::to_string),
            output: output.into(),
            exit_code,
            status,
            app_previews,
        })),
    }
}

#[must_use]
pub fn tool_started_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, ToolCallStatus::InProgress, None),
    })
}

#[must_use]
pub fn tool_invocation_updated_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status, None),
    })
}

#[must_use]
pub fn tool_invocation_completed_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    outcome: ToolOutcome,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status, Some(outcome)),
    })
}

#[must_use]
pub fn tool_output_started_event(call_item_id: String, tool_call_id: Option<&str>) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_output_item(
            &call_item_id,
            tool_call_id,
            ToolCallStatus::InProgress,
            None,
            None,
            String::new(),
            Vec::new(),
        ),
    })
}

#[must_use]
pub fn tool_output_updated_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_output_item(&call_item_id, tool_call_id, ToolCallStatus::InProgress, None, None, output, Vec::new()),
    })
}

#[must_use]
pub fn tool_output_completed_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
    app_previews: Vec<AppPreviewItem>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item(&call_item_id, tool_call_id, status, exit_code, spool_path, output, app_previews),
    })
}

#[must_use]
#[cold]
pub fn error_item_completed_event(item_id: String, message: impl Into<String>) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: item_id,
            details: ThreadItemDetails::Error(ErrorItem { message: message.into() }),
        },
    })
}

fn progress_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

#[cfg(test)]
mod tests;
