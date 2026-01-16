use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::exec::events::ThreadEvent;
use crate::gemini::{Content, Part};
use crate::llm::provider::Message;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::core::agent::conversation::build_messages_from_conversation;

// ============================================================================
// Context Manager: Call/Output Pairing Invariants (OpenAI Codex pattern)
// ============================================================================

/// Unique identifier for a tool call
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub String);

/// Status of a tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStatus {
    Success,
    Failed,
    Canceled,
    Timeout,
}

impl OutputStatus {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Timeout => "timeout",
        }
    }
}

/// Items that participate in call/output pairing for validation
#[derive(Debug, Clone)]
pub enum PairableHistoryItem {
    /// Tool call without output (yet)
    ToolCall {
        call_id: ToolCallId,
        tool_name: String,
    },
    /// Tool output for a previous call
    ToolOutput {
        call_id: ToolCallId,
        status: OutputStatus,
    },
}

/// Record of a missing output in conversation history
#[derive(Debug, Clone)]
pub struct MissingOutput {
    pub call_id: ToolCallId,
    pub tool_name: String,
}

/// Validation report for conversation history state
#[derive(Debug, Default, Clone)]
pub struct HistoryValidationReport {
    /// Tool calls without corresponding outputs
    pub missing_outputs: Vec<MissingOutput>,
    /// Outputs without corresponding calls (orphans)
    pub orphan_outputs: Vec<ToolCallId>,
}

impl HistoryValidationReport {
    /// Check if history is in a valid state
    pub fn is_valid(&self) -> bool {
        self.missing_outputs.is_empty() && self.orphan_outputs.is_empty()
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        if self.is_valid() {
            "History invariants are valid".to_string()
        } else {
            format!(
                "{} missing outputs, {} orphan outputs",
                self.missing_outputs.len(),
                self.orphan_outputs.len()
            )
        }
    }
}

#[inline]
pub(crate) fn record_turn_duration(
    turn_durations: &mut Vec<u128>,
    recorded: &mut bool,
    start: &std::time::Instant,
) {
    if !*recorded {
        turn_durations.push(start.elapsed().as_millis());
        *recorded = true;
    }
}

/// API failure tracking for exponential backoff
pub struct ApiFailureTracker {
    pub consecutive_failures: u32,
    pub last_failure: Option<std::time::Instant>,
}

impl Default for ApiFailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiFailureTracker {
    pub fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure: None,
        }
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure = Some(std::time::Instant::now());
    }

    pub fn reset(&mut self) {
        self.consecutive_failures = 0;
        self.last_failure = None;
    }

    pub fn should_circuit_break(&self) -> bool {
        self.consecutive_failures >= 3
    }

    pub fn backoff_duration(&self) -> Duration {
        let base_ms = 1000;
        let max_ms = 30000;
        let backoff_ms = base_ms * 2_u64.pow(self.consecutive_failures.saturating_sub(1));
        Duration::from_millis(backoff_ms.min(max_ms))
    }
}

pub struct TaskRunState {
    pub conversation: Vec<Content>,
    pub conversation_messages: Vec<Message>,
    pub created_contexts: Vec<String>,
    pub modified_files: Vec<String>,
    pub executed_commands: Vec<String>,
    pub warnings: Vec<String>,
    pub last_file_path: Option<String>,
    pub last_dir_path: Option<String>,
    pub has_completed: bool,
    pub completion_outcome: TaskOutcome,
    pub turns_executed: usize,
    pub turn_durations_ms: Vec<u128>,
    pub max_tool_loops: usize,
    pub consecutive_tool_loops: usize,
    pub max_tool_loop_streak: usize,
    pub tool_loop_limit_hit: bool,
    pub consecutive_idle_turns: usize,
    // Optimization: Track last processed message index for incremental Gemini message building
    pub last_processed_message_idx: usize,
    pub max_context_tokens: usize,
}

impl TaskRunState {
    pub fn new(
        conversation: Vec<Content>,
        conversation_messages: Vec<Message>,
        max_tool_loops: usize,
        max_context_tokens: usize,
    ) -> Self {
        Self {
            conversation,
            conversation_messages,
            created_contexts: Vec::with_capacity(16), // Typical session creates ~5-10 contexts
            modified_files: Vec::with_capacity(32),   // Typical session modifies ~10-20 files
            executed_commands: Vec::with_capacity(64), // Typical session executes ~20-40 commands
            warnings: Vec::with_capacity(16),         // Typical session has ~5-10 warnings
            last_file_path: None,
            last_dir_path: None,
            has_completed: false,
            completion_outcome: TaskOutcome::Unknown,
            turns_executed: 0,
            turn_durations_ms: Vec::with_capacity(max_tool_loops), // Pre-allocate for expected number of turns
            last_processed_message_idx: 0,
            max_tool_loops,
            consecutive_tool_loops: 0,
            max_tool_loop_streak: 0,
            tool_loop_limit_hit: false,
            consecutive_idle_turns: 0,
            max_context_tokens,
        }
    }

    pub fn record_turn(&mut self, start: &std::time::Instant, recorded: &mut bool) {
        record_turn_duration(&mut self.turn_durations_ms, recorded, start);
    }

    /// Get current budget utilization (0.0 to 1.0)
    pub fn utilization(&self) -> f64 {
        if self.max_context_tokens == 0 {
            return 0.0;
        }
        self.total_tokens() as f64 / self.max_context_tokens as f64
    }

    /// Calculate total estimated tokens in the conversation.
    pub fn total_tokens(&self) -> usize {
        self.conversation_messages
            .iter()
            .map(|m| m.estimate_tokens())
            .sum()
    }

    /// Find a safe split point for history trimming that doesn't break tool call/output pairs.
    ///
    /// Returns an index into `self.conversation` that is safe to split at.
    /// A split point is safe if no tool response in the "keep" set (index..len)
    /// has its corresponding tool call in the "discard" set (0..index).
    pub fn find_safe_split_point(&self, preferred_split_at: usize) -> usize {
        if preferred_split_at == 0 || preferred_split_at >= self.conversation.len() {
            return preferred_split_at;
        }

        let mut safe_split_at = preferred_split_at;

        loop {
            if safe_split_at == 0 {
                break;
            }

            // Check if splitting at safe_split_at is safe.
            // A split is UNSAFE if there's a tool response in [safe_split_at..len]
            // whose call is in [0..safe_split_at].

            let mut call_ids_in_discard = HashSet::new();
            let mut response_ids_in_keep = HashSet::new();

            // Messages 1..safe_split_at+1 are the "discard" set (excluding system prompt at 0)
            for i in 1..=safe_split_at {
                if let Some(msg) = self.conversation_messages.get(i) {
                    if let Some(tool_calls) = &msg.tool_calls {
                        for call in tool_calls {
                            call_ids_in_discard.insert(call.id.clone());
                        }
                    }
                }
            }

            // Messages safe_split_at+1..len are the "keep" set
            for i in (safe_split_at + 1)..self.conversation_messages.len() {
                if let Some(msg) = self.conversation_messages.get(i) {
                    if let Some(id) = &msg.tool_call_id {
                        response_ids_in_keep.insert(id.clone());
                    }
                }
            }

            // If any response in keep has its call in discard, it's unsafe.
            let has_orphan_response = response_ids_in_keep
                .iter()
                .any(|id| call_ids_in_discard.contains(id));

            if !has_orphan_response {
                break;
            }

            // Move split point earlier to include the call in the keep set.
            safe_split_at -= 1;
        }

        safe_split_at
    }

    pub fn finalize_outcome(&mut self, max_turns: usize) {
        if self.completion_outcome == TaskOutcome::Unknown {
            if self.has_completed {
                self.completion_outcome = TaskOutcome::Success;
            } else if self.tool_loop_limit_hit {
                self.completion_outcome = TaskOutcome::tool_loop_limit_reached(
                    self.max_tool_loops,
                    self.consecutive_tool_loops,
                );
            } else if self.turns_executed >= max_turns {
                self.completion_outcome =
                    TaskOutcome::turn_limit_reached(max_turns, self.turns_executed);
            }
        }
    }

    pub fn register_tool_loop(&mut self) -> usize {
        self.consecutive_tool_loops += 1;
        if self.consecutive_tool_loops > self.max_tool_loop_streak {
            self.max_tool_loop_streak = self.consecutive_tool_loops;
        }
        self.consecutive_tool_loops
    }

    pub fn reset_tool_loop_guard(&mut self) {
        self.consecutive_tool_loops = 0;
    }

    pub fn mark_tool_loop_limit_hit(&mut self) {
        // Idempotent: skip if already marked
        if self.tool_loop_limit_hit {
            return;
        }
        self.tool_loop_limit_hit = true;
        self.completion_outcome =
            TaskOutcome::tool_loop_limit_reached(self.max_tool_loops, self.consecutive_tool_loops);
    }

    pub fn into_results(
        self,
        summary: String,
        thread_events: Vec<ThreadEvent>,
        total_duration_ms: u128,
    ) -> TaskResults {
        let total_turn_duration_ms: u128 = self.turn_durations_ms.iter().sum();
        let average_turn_duration_ms = if !self.turn_durations_ms.is_empty() {
            Some(total_turn_duration_ms as f64 / self.turn_durations_ms.len() as f64)
        } else {
            None
        };
        let max_turn_duration_ms = self.turn_durations_ms.iter().copied().max();
        let completion_outcome = self.completion_outcome;

        TaskResults {
            created_contexts: self.created_contexts,
            modified_files: self.modified_files,
            executed_commands: self.executed_commands,
            summary,
            warnings: self.warnings,
            thread_events,
            outcome: completion_outcome,
            turns_executed: self.turns_executed,
            total_duration_ms,
            average_turn_duration_ms,
            max_turn_duration_ms,
            turn_durations_ms: self.turn_durations_ms,
        }
    }

    pub fn summarize_conversation_if_needed(
        &mut self,
        system_instruction: &str,
        preserve_recent_turns: usize,
        utilization: f64,
    ) {
        if utilization < 0.90 {
            return;
        }

        if self.conversation.len() <= preserve_recent_turns {
            return;
        }

        let preferred_split_at = self
            .conversation
            .len()
            .saturating_sub(preserve_recent_turns);

        // Context Manager: Find a safe split point that doesn't break tool call/output pairs.
        let split_at = self.find_safe_split_point(preferred_split_at);

        if split_at == 0 {
            return;
        }

        let summarize_list = |items: &[String]| -> String {
            const MAX_ITEMS: usize = 5;
            if items.is_empty() {
                return "none".into();
            }
            let shown: Vec<&str> = items.iter().take(MAX_ITEMS).map(|s| s.as_str()).collect();
            if items.len() > MAX_ITEMS {
                format!("{} [+{} more]", shown.join(", "), items.len() - MAX_ITEMS)
            } else {
                shown.join(", ")
            }
        };

        let summary = format!(
            "Summarized {} earlier turns to stay within context budget. Files: {}; Commands: {}; Warnings: {}.",
            split_at,
            summarize_list(&self.modified_files),
            summarize_list(&self.executed_commands),
            summarize_list(
                &self
                    .warnings
                    .iter()
                    .map(|w| w.to_string())
                    .collect::<Vec<_>>()
            ),
        );

        let mut new_conversation = Vec::with_capacity(1 + preserve_recent_turns);
        new_conversation.push(Content::user_parts(vec![Part::Text {
            text: summary,
            thought_signature: None,
        }]));
        new_conversation.extend_from_slice(&self.conversation[split_at..]);
        self.conversation = new_conversation;
        self.conversation_messages =
            build_messages_from_conversation(system_instruction, &self.conversation);

        // Context Manager: Ensure history invariants are maintained after summarization.
        // Summarization might split a tool call from its response if they span across
        // the split point. Normalization fixes this by adding synthetic outputs or
        // removing orphaned responses.
        self.normalize();

        self.last_processed_message_idx = self.conversation.len();
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    pub fn add_user_message(&mut self, text: String) {
        self.conversation.push(Content {
            role: "user".to_string(),
            parts: vec![Part::Text {
                text,
                thought_signature: None,
            }],
        });
    }

    pub fn add_tool_response_message(&mut self, call_id: String, content: String) {
        self.conversation_messages
            .push(Message::tool_response(call_id, content));
    }

    /// Push a successful tool result to both conversation (for Gemini) and conversation_messages.
    /// This ensures state consistency between the two collections.
    #[inline]
    pub fn push_tool_result(
        &mut self,
        call_id: String,
        tool_name: &str,
        _display_text: String,
        serialized_result: String,
        is_gemini: bool,
    ) {
        if is_gemini {
            let response_value = serde_json::from_str(&serialized_result)
                .unwrap_or_else(|_| serde_json::json!({ "result": serialized_result }));

            self.conversation.push(Content {
                role: "function".to_string(),
                parts: vec![Part::FunctionResponse {
                    function_response: crate::gemini::FunctionResponse {
                        name: tool_name.to_string(),
                        response: response_value,
                        id: Some(call_id.clone()),
                    },
                    thought_signature: None,
                }],
            });
        }
        self.conversation_messages
            .push(Message::tool_response(call_id, serialized_result));
        self.executed_commands.push(tool_name.to_owned());
    }

    /// Push a tool error to both conversation (for Gemini) and conversation_messages.
    #[inline]
    pub fn push_tool_error(
        &mut self,
        call_id: String,
        tool_name: &str,
        error_msg: String,
        is_gemini: bool,
    ) {
        if is_gemini {
            self.conversation.push(Content {
                role: "function".to_string(),
                parts: vec![Part::FunctionResponse {
                    function_response: crate::gemini::FunctionResponse {
                        name: tool_name.to_string(),
                        response: serde_json::json!({ "error": error_msg }),
                        id: Some(call_id.clone()),
                    },
                    thought_signature: None,
                }],
            });
        }
        self.conversation_messages
            .push(Message::tool_response(call_id, error_msg));
    }

    // ========================================================================
    // Context Manager: Call/Output Pairing Validation (OpenAI Codex pattern)
    // ========================================================================

    /// Validate that conversation history maintains call/output invariants.
    ///
    /// This check ensures:
    /// 1. Every tool call has a corresponding output (tool_call_id match)
    /// 2. Every output has a corresponding call
    ///
    /// Returns a report of any violations found (non-fatal).
    pub fn validate_history_invariants(&self) -> HistoryValidationReport {
        validate_history_invariants(&self.conversation_messages)
    }

    /// Ensure all tool calls have corresponding outputs.
    ///
    /// If a tool call is missing its output (due to cancellation, timeout, or crash),
    /// create a synthetic output with status "canceled" to maintain the invariant.
    pub fn ensure_call_outputs_present(&mut self) {
        ensure_call_outputs_present(&mut self.conversation_messages);
    }

    /// Remove outputs without corresponding calls (orphaned outputs).
    ///
    /// Maintains the invariant that every output has a matching call.
    pub fn remove_orphan_outputs(&mut self) {
        remove_orphan_outputs(&mut self.conversation_messages);
    }

    /// Normalize history to enforce call/output pairing invariants.
    ///
    /// This calls ensure_call_outputs_present() and remove_orphan_outputs()
    /// to maintain both critical invariants.
    pub fn normalize(&mut self) {
        normalize_history(&mut self.conversation_messages);
    }

    /// Recover from crashed or interrupted session.
    ///
    /// Auto-fixes history by creating synthetic outputs for incomplete calls.
    pub fn recover_from_crash(&mut self) {
        recover_history_from_crash(&mut self.conversation_messages);
    }
}

// ============================================================================
// Standalone History Invariant Functions
// ============================================================================

/// Validate that conversation history maintains call/output invariants.
pub fn validate_history_invariants(messages: &[Message]) -> HistoryValidationReport {
    let mut call_map: HashMap<String, String> = HashMap::new();
    let mut output_ids: HashSet<String> = HashSet::new();

    // Scan messages to find tool calls and responses
    for msg in messages {
        // Tool calls: assistant messages with tool_calls field
        if let Some(tool_calls) = &msg.tool_calls {
            for tool_call in tool_calls {
                call_map.insert(tool_call.id.clone(), msg.role.to_string());
            }
        }

        // Tool responses: messages with tool_call_id set
        if let Some(tool_call_id) = &msg.tool_call_id {
            output_ids.insert(tool_call_id.clone());
        }
    }

    // Find missing outputs (calls without corresponding responses)
    let mut missing_outputs = Vec::new();
    for (call_id, _role) in &call_map {
        if !output_ids.contains(call_id) {
            missing_outputs.push(MissingOutput {
                call_id: ToolCallId(call_id.clone()),
                tool_name: "unknown".to_string(),
            });
        }
    }

    // Find orphan outputs (responses without matching calls)
    let mut orphan_outputs = Vec::new();
    for output_id in &output_ids {
        if !call_map.contains_key(output_id) {
            orphan_outputs.push(ToolCallId(output_id.clone()));
        }
    }

    HistoryValidationReport {
        missing_outputs,
        orphan_outputs,
    }
}

/// Ensure all tool calls have corresponding outputs in the message list.
pub fn ensure_call_outputs_present(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    // Create synthetic outputs for missing calls in reverse order to avoid index shifting
    for missing in report.missing_outputs.iter().rev() {
        let synthetic_message = Message::tool_response(
            missing.call_id.0.clone(),
            "canceled: Tool execution was interrupted. This synthetic output was created \
             during history normalization to maintain conversation invariants."
                .to_string(),
        );

        tracing::warn!(
            "Creating synthetic output for call {} due to missing execution result",
            missing.call_id.0
        );

        // Find the position to insert: right after the corresponding call
        let insert_pos = messages
            .iter()
            .position(|msg| {
                msg.tool_calls
                    .as_ref()
                    .is_some_and(|calls| calls.iter().any(|call| call.id == missing.call_id.0))
            })
            .map(|pos| pos + 1);

        if let Some(pos) = insert_pos {
            messages.insert(pos, synthetic_message);
        } else {
            // If we can't find the call, just append the synthetic output
            messages.push(synthetic_message);
        }
    }
}

/// Remove outputs without corresponding calls (orphaned outputs) from the message list.
pub fn remove_orphan_outputs(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if report.orphan_outputs.is_empty() {
        return;
    }

    let orphan_ids: HashSet<String> = report
        .orphan_outputs
        .iter()
        .map(|id| id.0.clone())
        .collect();

    let initial_len = messages.len();

    // Retain only messages that either:
    // - Don't have a tool_call_id (not a tool response)
    // - Have a tool_call_id that matches an existing call
    messages.retain(|msg| {
        if msg
            .tool_call_id
            .as_ref()
            .is_some_and(|id| orphan_ids.contains(id))
        {
            tracing::warn!(
                "Removing orphan output for call {}",
                msg.tool_call_id.as_ref().unwrap()
            );
            return false;
        }
        true
    });

    if messages.len() != initial_len {
        tracing::info!("Removed {} orphan outputs", initial_len - messages.len());
    }
}

/// Normalize history to enforce call/output pairing invariants.
pub fn normalize_history(messages: &mut Vec<Message>) {
    ensure_call_outputs_present(messages);
    remove_orphan_outputs(messages);

    // Log if issues were found
    let report = validate_history_invariants(messages);
    if !report.is_valid() {
        tracing::warn!("History validation: {}", report.summary());
    } else {
        tracing::debug!("History normalized successfully");
    }
}

/// Recover from crashed or interrupted session by fixing history invariants.
pub fn recover_history_from_crash(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if !report.missing_outputs.is_empty() {
        tracing::warn!(
            "Found {} missing outputs during recovery",
            report.missing_outputs.len()
        );
        ensure_call_outputs_present(messages);
    }

    if !report.orphan_outputs.is_empty() {
        tracing::warn!(
            "Found {} orphan outputs during recovery",
            report.orphan_outputs.len()
        );
        remove_orphan_outputs(messages);
    }

    if report.is_valid() {
        tracing::info!("History invariants are valid");
    }
}

// ============================================================================
// Tests: Context Manager - Call/Output Pairing Invariants
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: Create a test TaskRunState with empty conversation
    fn create_test_state() -> TaskRunState {
        TaskRunState::new(Vec::new(), Vec::new(), 10, 10000)
    }

    /// Test 1: Valid history with matched calls and outputs
    #[test]
    fn test_validate_history_valid_matched_pairs() {
        let mut state = create_test_state();

        // Add an assistant message with a tool call
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        // Add a tool response with matching call_id
        state.conversation_messages.push(Message::tool_response(
            "call_1".to_string(),
            "file1.rs\nfile2.rs".to_string(),
        ));

        let report = state.validate_history_invariants();

        assert!(
            report.is_valid(),
            "Valid paired call/output should pass validation"
        );
        assert!(report.missing_outputs.is_empty(), "No missing outputs");
        assert!(report.orphan_outputs.is_empty(), "No orphan outputs");
    }

    /// Test 2: Missing output (tool call without response)
    #[test]
    fn test_validate_history_missing_output() {
        let mut state = create_test_state();

        // Add a tool call without corresponding response
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        let report = state.validate_history_invariants();

        assert!(
            !report.is_valid(),
            "Missing output should invalidate history"
        );
        assert_eq!(
            report.missing_outputs.len(),
            1,
            "Should detect one missing output"
        );
        assert_eq!(
            report.missing_outputs[0].call_id.0, "call_1",
            "Should identify correct call_id"
        );
        assert!(
            report.orphan_outputs.is_empty(),
            "Should have no orphan outputs"
        );
    }

    /// Test 3: Orphan output (response without corresponding call)
    #[test]
    fn test_validate_history_orphan_output() {
        let mut state = create_test_state();

        // Add a tool response without a preceding call
        state.conversation_messages.push(Message::tool_response(
            "orphan_call".to_string(),
            "Some result".to_string(),
        ));

        let report = state.validate_history_invariants();

        assert!(
            !report.is_valid(),
            "Orphan output should invalidate history"
        );
        assert!(
            report.missing_outputs.is_empty(),
            "Should have no missing outputs"
        );
        assert_eq!(
            report.orphan_outputs.len(),
            1,
            "Should detect one orphan output"
        );
        assert_eq!(
            report.orphan_outputs[0].0, "orphan_call",
            "Should identify correct orphan call_id"
        );
    }

    /// Test 4: ensure_call_outputs_present creates synthetic outputs
    #[test]
    fn test_ensure_call_outputs_present() {
        let mut state = create_test_state();

        // Add a tool call without response
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        let initial_len = state.conversation_messages.len();

        // Ensure outputs are present (should create synthetic output)
        state.ensure_call_outputs_present();

        assert_eq!(
            state.conversation_messages.len(),
            initial_len + 1,
            "Should add one synthetic output"
        );

        // Verify the synthetic output was added
        let last_msg = &state.conversation_messages[initial_len];
        assert_eq!(
            last_msg.tool_call_id,
            Some("call_1".to_string()),
            "Synthetic output should have correct call_id"
        );
        assert!(
            last_msg.content.as_text().contains("canceled"),
            "Synthetic output should indicate cancellation"
        );

        // Validate after fix
        let report = state.validate_history_invariants();
        assert!(
            report.is_valid(),
            "History should be valid after normalization"
        );
    }

    /// Test 5: remove_orphan_outputs filters out orphaned responses
    #[test]
    fn test_remove_orphan_outputs() {
        let mut state = create_test_state();

        // Add a tool call
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "list_files".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        // Add valid response for call_1
        state.conversation_messages.push(Message::tool_response(
            "call_1".to_string(),
            "valid result".to_string(),
        ));

        // Add orphan response (no matching call)
        state.conversation_messages.push(Message::tool_response(
            "orphan_call".to_string(),
            "orphan result".to_string(),
        ));

        let initial_len = state.conversation_messages.len();

        // Remove orphans
        state.remove_orphan_outputs();

        assert_eq!(
            state.conversation_messages.len(),
            initial_len - 1,
            "Should remove one orphan output"
        );

        // Verify call_1 output is still there, orphan is gone
        let has_call_1_output = state
            .conversation_messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().map_or(false, |id| id == "call_1"));
        assert!(has_call_1_output, "Valid output should be retained");

        let has_orphan = state.conversation_messages.iter().any(|msg| {
            msg.tool_call_id
                .as_ref()
                .map_or(false, |id| id == "orphan_call")
        });
        assert!(!has_orphan, "Orphan output should be removed");

        // Validate after cleanup
        let report = state.validate_history_invariants();
        assert!(report.is_valid(), "History should be valid after cleanup");
    }

    /// Test 6: normalize() applies both fixes
    #[test]
    fn test_normalize_combined_fixes() {
        let mut state = create_test_state();

        // Add call_1 without output
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        // Add valid response for call_2
        let call2 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "call_2".to_string(),
                "write_file".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call2);

        state.conversation_messages.push(Message::tool_response(
            "call_2".to_string(),
            "written".to_string(),
        ));

        // Add orphan response
        state.conversation_messages.push(Message::tool_response(
            "orphan".to_string(),
            "orphan result".to_string(),
        ));

        // Normalize should:
        // 1. Create synthetic output for call_1
        // 2. Remove orphan output
        state.normalize();

        let report = state.validate_history_invariants();
        assert!(
            report.is_valid(),
            "After normalize, history should be valid"
        );

        // Verify call_1 has synthetic output
        let call_1_has_output = state
            .conversation_messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().map_or(false, |id| id == "call_1"));
        assert!(
            call_1_has_output,
            "call_1 should have synthetic output after normalization"
        );

        // Verify orphan is gone
        let has_orphan = state
            .conversation_messages
            .iter()
            .any(|msg| msg.tool_call_id.as_ref().map_or(false, |id| id == "orphan"));
        assert!(!has_orphan, "Orphan should be removed after normalization");
    }

    /// Test 7: recover_from_crash() handles both missing and orphan outputs
    #[test]
    fn test_recover_from_crash() {
        let mut state = create_test_state();

        // Set up a broken state similar to a crash scenario
        // Missing output
        let call1 = Message::assistant_with_tools(
            "".to_string(),
            vec![crate::llm::provider::ToolCall::function(
                "crashed_call".to_string(),
                "dangerous_op".to_string(),
                "{}".to_string(),
            )],
        );
        state.conversation_messages.push(call1);

        // Orphan output (stale from previous session)
        state.conversation_messages.push(Message::tool_response(
            "old_call".to_string(),
            "stale result".to_string(),
        ));

        // Recover
        state.recover_from_crash();

        // Verify history is now valid
        let report = state.validate_history_invariants();
        assert!(report.is_valid(), "After recovery, history should be valid");

        // Verify synthetic output was created
        let has_recovered_call = state.conversation_messages.iter().any(|msg| {
            msg.tool_call_id
                .as_ref()
                .map_or(false, |id| id == "crashed_call")
        });
        assert!(
            has_recovered_call,
            "Crashed call should have recovered synthetic output"
        );

        // Verify orphan was removed
        let has_orphan = state.conversation_messages.iter().any(|msg| {
            msg.tool_call_id
                .as_ref()
                .map_or(false, |id| id == "old_call")
        });
        assert!(
            !has_orphan,
            "Orphan output should be removed during recovery"
        );
    }

    /// Test 8: HistoryValidationReport summary messages
    #[test]
    fn test_validation_report_summary() {
        let valid_report = HistoryValidationReport {
            missing_outputs: vec![],
            orphan_outputs: vec![],
        };
        assert_eq!(valid_report.summary(), "History invariants are valid");
        assert!(valid_report.is_valid());

        let invalid_report = HistoryValidationReport {
            missing_outputs: vec![
                MissingOutput {
                    call_id: ToolCallId("call_1".to_string()),
                    tool_name: "tool_a".to_string(),
                },
                MissingOutput {
                    call_id: ToolCallId("call_2".to_string()),
                    tool_name: "tool_b".to_string(),
                },
            ],
            orphan_outputs: vec![ToolCallId("orphan_1".to_string())],
        };
        assert_eq!(
            invalid_report.summary(),
            "2 missing outputs, 1 orphan outputs"
        );
        assert!(!invalid_report.is_valid());
    }

    /// Test 9: Multiple tool calls with selective missing outputs
    #[test]
    fn test_multiple_calls_partial_outputs() {
        let mut state = create_test_state();

        // Add 3 tool calls
        for i in 1..=3 {
            let msg = Message::assistant_with_tools(
                "".to_string(),
                vec![crate::llm::provider::ToolCall::function(
                    format!("call_{}", i),
                    format!("tool_{}", i),
                    "{}".to_string(),
                )],
            );
            state.conversation_messages.push(msg);
        }

        // Add outputs for calls 1 and 3, but not 2
        state.conversation_messages.push(Message::tool_response(
            "call_1".to_string(),
            "result_1".to_string(),
        ));
        state.conversation_messages.push(Message::tool_response(
            "call_3".to_string(),
            "result_3".to_string(),
        ));

        let report = state.validate_history_invariants();

        assert!(
            !report.is_valid(),
            "Should be invalid with missing output for call_2"
        );
        assert_eq!(
            report.missing_outputs.len(),
            1,
            "Should have exactly one missing output"
        );
        assert_eq!(
            report.missing_outputs[0].call_id.0, "call_2",
            "Should identify call_2 as missing"
        );

        // Normalize and verify
        state.normalize();
        let final_report = state.validate_history_invariants();
        assert!(
            final_report.is_valid(),
            "After normalization, all invariants should be satisfied"
        );
    }

    /// Test 10: OutputStatus enum conversion
    #[test]
    fn test_output_status_as_str() {
        assert_eq!(OutputStatus::Success.as_str(), "success");
        assert_eq!(OutputStatus::Failed.as_str(), "failed");
        assert_eq!(OutputStatus::Canceled.as_str(), "canceled");
        assert_eq!(OutputStatus::Timeout.as_str(), "timeout");
    }

    /// Test 11: total_tokens() estimation
    #[test]
    fn test_total_tokens() {
        let mut state = create_test_state();
        state
            .conversation_messages
            .push(Message::user("Hello".to_string())); // ~4 + 1 = 5
        state
            .conversation_messages
            .push(Message::assistant("Hi".to_string())); // ~4 + 1 = 5

        let tokens = state.total_tokens();
        assert!(tokens > 0);
    }

    /// Test 12: find_safe_split_point() maintains call/output pairs
    #[test]
    fn test_find_safe_split_point() {
        let mut state = create_test_state();

        // Turn 1: User
        state.conversation.push(Content::user_text("User 1"));
        state
            .conversation_messages
            .push(Message::user("User 1".to_string()));

        // Turn 2: Assistant (Call A)
        state.conversation.push(Content {
            role: "model".to_string(),
            parts: vec![Part::Text {
                text: "Calling A".to_string(),
                thought_signature: None,
            }],
        });
        state
            .conversation_messages
            .push(Message::assistant_with_tools(
                "Calling A".to_string(),
                vec![crate::llm::provider::ToolCall::function(
                    "call_a".to_string(),
                    "tool_a".to_string(),
                    "{}".to_string(),
                )],
            ));

        // Turn 3: User (Response A)
        state.conversation.push(Content::user_text("Result A"));
        state.conversation_messages.push(Message::tool_response(
            "call_a".to_string(),
            "Result A".to_string(),
        ));

        // Turn 4: Assistant (Call B)
        state.conversation.push(Content {
            role: "model".to_string(),
            parts: vec![Part::Text {
                text: "Calling B".to_string(),
                thought_signature: None,
            }],
        });
        state
            .conversation_messages
            .push(Message::assistant_with_tools(
                "Calling B".to_string(),
                vec![crate::llm::provider::ToolCall::function(
                    "call_b".to_string(),
                    "tool_b".to_string(),
                    "{}".to_string(),
                )],
            ));

        // Turn 5: User (Response B)
        state.conversation.push(Content::user_text("Result B"));
        state.conversation_messages.push(Message::tool_response(
            "call_b".to_string(),
            "Result B".to_string(),
        ));

        // Total conversation length is 5.
        assert_eq!(state.conversation.len(), 5);

        // If we want to preserve 2 turns (keeping turns 4 and 5), preferred_split_at is 3.
        // Turn 3 is Response A. Turn 2 is Call A.
        // If we split at 3, we keep Response A but lose Call A.
        // find_safe_split_point(3) should move it to 2 to include Call A in the keep set.

        let safe_split = state.find_safe_split_point(3);
        assert_eq!(safe_split, 2, "Should move split point to include Call A");

        // If we want to preserve 1 turn (keeping turn 5), preferred_split_at is 4.
        // Turn 4 is Call B. Turn 5 is Response B.
        // Splitting at 4 is safe because Call B and Response B are both in the keep set.
        let safe_split_2 = state.find_safe_split_point(4);
        assert_eq!(safe_split_2, 4, "Should stay at 4 as it is safe");
    }
}
