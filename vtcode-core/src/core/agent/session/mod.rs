//! Centralized agent session state management.

use crate::core::agent::error_recovery::ErrorRecoveryState;
use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::core::pending_actions::PendingActions;
use crate::core::state_schema::SchemaVersion;
use crate::exec::events::Usage;
use crate::llm::provider::{Message, ResponsesContinuationState, responses_continuation_key};
use crate::llm::providers::gemini::wire::{Content, FunctionResponse, Part};
use hashbrown::HashMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vtcode_exec_events::ThreadEvent;

/// Manages the state of an active agent session, including conversation history,
/// statistics, and turn-based constraints.
pub struct AgentSessionState {
    /// The thread or session ID.
    pub session_id: String,

    /// Provider-specific conversation history (e.g., Gemini style).
    pub conversation: Vec<Content>,

    /// Standardized conversation messages (OpenAI/Anthropic style).
    pub messages: Vec<Message>,

    /// Schema version for durable state persistence.
    pub schema_version: SchemaVersion,

    /// Statistics for the current session.
    pub stats: SessionStats,

    /// Constraints and limits for the session.
    pub constraints: SessionConstraints,

    /// Outcome of the session if completed.
    pub outcome: TaskOutcome,
    /// Provider stop reason associated with the last model turn, when available.
    pub stop_reason: Option<String>,
    /// Estimated total API cost in USD for the session, when available.
    pub total_cost_usd: Option<f64>,

    /// Whether the session has completed.
    pub is_completed: bool,

    /// Current reasoning stage.
    pub current_stage: Option<String>,

    // Tracking for side-effects and progress
    pub created_contexts: Vec<String>,
    pub modified_files: Vec<String>,
    pub executed_commands: Vec<String>,
    pub warnings: Vec<String>,
    pub last_file_path: Option<String>,
    pub last_dir_path: Option<String>,

    // Internal loop state
    pub consecutive_tool_loops: usize,
    pub tool_loop_limit_hit: bool,
    /// Consecutive escalation events in the current escalation chain.
    /// Reset to 0 when tool calls dispatch without escalation.
    pub consecutive_escalations: u32,
    /// Rolling window of progress hashes for stagnation detection.
    /// Each entry is a hash of the assistant response content + key state.
    pub progress_hashes: Vec<u64>,
    /// Consecutive turns with matching progress hashes.
    pub stagnant_turns: usize,
    pub last_processed_message_idx: usize,
    /// Responses-style continuation state keyed by normalized provider/model pairs.
    pub previous_response_chains: HashMap<(String, String), ResponsesContinuationState>,
    /// Agent-local recent error diagnostics for interrupted or repeated tool failures.
    pub error_recovery: Arc<Mutex<ErrorRecoveryState>>,
    /// Pending tool actions that have been issued but not yet returned.
    pub pending_actions: PendingActions,

    // Legacy / Stats fields for compatibility
    pub consecutive_idle_turns: usize,
    pub max_tool_loop_streak: usize,
    pub turn_count: usize,
    pub turn_total_ms: u128,
    pub turn_max_ms: u128,
    pub turn_durations_ms: Vec<u128>,
    /// Per-tool execution latencies recorded during the current turn.
    /// Entries are (tool_name, duration_ms).
    pub turn_tool_latencies: Vec<(String, u64)>,

    /// Cached total estimated token count for the conversation history.
    /// Updated incrementally on each push to avoid O(n) scans per turn.
    cached_total_tokens: usize,
}

/// Statistics tracked during an agent session.
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub turns_executed: usize,
    pub total_duration: Duration,
    pub turn_durations: Vec<Duration>,
    pub total_usage: Usage,
}

impl SessionStats {
    pub fn merge_usage(&mut self, usage: crate::llm::provider::Usage) {
        self.total_usage.input_tokens = self
            .total_usage
            .input_tokens
            .saturating_add(usage.prompt_tokens as u64);
        self.total_usage.output_tokens = self
            .total_usage
            .output_tokens
            .saturating_add(usage.completion_tokens as u64);
        let cached = usage.cache_read_tokens_or_fallback();
        if cached > 0 {
            self.total_usage.cached_input_tokens = self
                .total_usage
                .cached_input_tokens
                .saturating_add(cached as u64);
        }
        let cache_creation = usage.cache_creation_tokens_or_zero();
        if cache_creation > 0 {
            self.total_usage.cache_creation_tokens = self
                .total_usage
                .cache_creation_tokens
                .saturating_add(cache_creation as u64);
        }
    }
}

/// Constraints applied to an agent session.
#[derive(Debug, Clone)]
pub struct SessionConstraints {
    pub max_turns: usize,
    pub max_tool_loops: usize,
    pub max_context_tokens: usize,
}

impl AgentSessionState {
    pub fn new(
        session_id: String,
        max_turns: usize,
        max_tool_loops: usize,
        max_context_tokens: usize,
    ) -> Self {
        Self {
            session_id,
            schema_version: SchemaVersion::CURRENT,
            conversation: Vec::new(),
            messages: Vec::new(),
            stats: SessionStats::default(),
            constraints: SessionConstraints {
                max_turns,
                max_tool_loops,
                max_context_tokens,
            },
            outcome: TaskOutcome::Unknown,
            stop_reason: None,
            total_cost_usd: None,
            is_completed: false,
            current_stage: None,
            created_contexts: Vec::with_capacity(16),
            modified_files: Vec::with_capacity(32),
            executed_commands: Vec::with_capacity(64),
            warnings: Vec::with_capacity(16),
            last_file_path: None,
            last_dir_path: None,
            consecutive_tool_loops: 0,
            tool_loop_limit_hit: false,
            consecutive_escalations: 0,
            progress_hashes: Vec::with_capacity(16),
            stagnant_turns: 0,
            last_processed_message_idx: 0,
            previous_response_chains: HashMap::new(),
            error_recovery: Arc::new(Mutex::new(ErrorRecoveryState::default())),
            pending_actions: PendingActions::new(100),
            consecutive_idle_turns: 0,
            max_tool_loop_streak: 0,
            turn_count: 0,
            turn_total_ms: 0,
            turn_max_ms: 0,
            turn_durations_ms: Vec::with_capacity(max_turns),
            turn_tool_latencies: Vec::with_capacity(32),
            cached_total_tokens: 0,
        }
    }

    /// Record a completed turn.
    pub fn record_turn(&mut self, start: &Instant, recorded: &mut bool) {
        if *recorded {
            return;
        }
        let duration = start.elapsed();
        let ms = duration.as_millis() as u64;

        self.stats.turns_executed += 1;
        self.stats.total_duration += duration;
        self.stats.turn_durations.push(duration);

        // Legacy stats
        self.turn_count += 1;
        self.turn_total_ms += ms as u128;
        self.turn_max_ms = self.turn_max_ms.max(ms as u128);
        self.turn_durations_ms.push(ms as u128);

        *recorded = true;
    }

    pub fn finalize_outcome(&mut self, max_turns: usize) {
        if self.outcome != TaskOutcome::Unknown {
            return;
        }
        // Priority order: tool loop limit > completion > turn limit
        if self.tool_loop_limit_hit {
            self.outcome = TaskOutcome::tool_loop_limit_reached(
                self.constraints.max_tool_loops,
                self.consecutive_tool_loops,
            );
        } else if self.is_completed {
            self.outcome = TaskOutcome::Success;
        } else if self.stats.turns_executed >= max_turns {
            self.outcome = TaskOutcome::turn_limit_reached(max_turns, self.stats.turns_executed);
        }
    }

    pub fn register_tool_loop(&mut self) -> usize {
        self.consecutive_tool_loops += 1;
        self.max_tool_loop_streak = self.max_tool_loop_streak.max(self.consecutive_tool_loops);
        self.consecutive_tool_loops
    }

    pub fn reset_tool_loop_guard(&mut self) {
        self.consecutive_tool_loops = 0;
    }

    pub fn previous_response_id_for(&self, provider: &str, model: &str) -> Option<String> {
        self.previous_response_chain_for(provider, model)
            .map(|chain| chain.response_id.clone())
    }

    pub fn previous_response_chain_for(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<&ResponsesContinuationState> {
        responses_continuation_key(provider, model)
            .and_then(|key| self.previous_response_chains.get(&key))
    }

    pub fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: Vec<Message>,
    ) {
        let Some(key) = responses_continuation_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.previous_response_chains.remove(&key);
            return;
        };

        self.previous_response_chains.insert(
            key,
            ResponsesContinuationState {
                response_id: response_id.to_string(),
                messages,
            },
        );
    }

    pub fn clear_previous_response_chain_for(&mut self, provider: &str, model: &str) {
        if let Some(key) = responses_continuation_key(provider, model) {
            self.previous_response_chains.remove(&key);
        }
    }

    pub fn clear_previous_response_chain(&mut self) {
        self.previous_response_chains.clear();
    }

    pub fn mark_tool_loop_limit_hit(&mut self) {
        if self.tool_loop_limit_hit {
            return;
        }
        self.tool_loop_limit_hit = true;
        self.outcome = TaskOutcome::tool_loop_limit_reached(
            self.constraints.max_tool_loops,
            self.consecutive_tool_loops,
        );
    }

    /// Add a user message to the history with metadata.
    pub fn add_user_message(&mut self, text: String) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let tokens = text.len().saturating_div(4); // rough estimate: ~4 chars per token
        let metadata = crate::core::message_metadata::MessageMetadata::user_input(now, tokens);
        self.conversation.push(Content::user_text(text.as_str()));
        let msg = Message::user(text).with_metadata(metadata);
        // Role overhead (~4 tokens) + content tokens
        let msg_tokens = msg.estimate_tokens();
        self.cached_total_tokens = self.cached_total_tokens.saturating_add(msg_tokens);
        self.messages.push(msg);
    }

    /// Threshold for consecutive identical progress hashes before stagnation is declared.
    const PROGRESS_STAGNATION_THRESHOLD: usize = 4;

    /// Compute a hash of the current assistant response content for progress tracking.
    fn assistant_response_hash(&self) -> Option<u64> {
        use crate::llm::provider::{MessageContent, MessageRole};
        let last_assistant = self
            .messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)?;
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        match &last_assistant.content {
            MessageContent::Text(t) => t.hash(&mut hasher),
            MessageContent::Parts(parts) => {
                for part in parts {
                    if let crate::llm::provider::ContentPart::Text { text, .. } = part {
                        text.hash(&mut hasher);
                    }
                }
            }
        }
        Some(hasher.finish())
    }

    /// Record the current assistant response hash and return true if stagnation detected.
    pub fn record_progress_hash_and_check_stagnation(&mut self) -> bool {
        let Some(hash) = self.assistant_response_hash() else {
            self.stagnant_turns = 0;
            return false;
        };
        if self.progress_hashes.last() == Some(&hash) {
            self.stagnant_turns += 1;
        } else {
            self.stagnant_turns = 0;
        }
        self.progress_hashes.push(hash);
        if self.progress_hashes.len() > 16 {
            self.progress_hashes.remove(0);
        }
        self.stagnant_turns >= Self::PROGRESS_STAGNATION_THRESHOLD
    }

    /// Attach metadata to the most recent message. Used by the execution loop
    /// to annotate LLM responses and tool results after they are pushed.
    pub fn attach_metadata_to_last(&mut self, source: &str, estimated_tokens: usize) {
        if let Some(last) = self.messages.last_mut() {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let metadata = match source {
                "llm_response" => crate::core::message_metadata::MessageMetadata::llm_response(
                    now,
                    estimated_tokens,
                ),
                "tool_result" => crate::core::message_metadata::MessageMetadata::tool_result(
                    now,
                    estimated_tokens,
                ),
                "system" => {
                    crate::core::message_metadata::MessageMetadata::system(now, estimated_tokens)
                }
                "synthetic" => {
                    crate::core::message_metadata::MessageMetadata::synthetic(now, estimated_tokens)
                }
                _ => crate::core::message_metadata::MessageMetadata::user_input(
                    now,
                    estimated_tokens,
                ),
            };
            last.metadata = Some(metadata);
        }
    }

    /// Check if context limits are approaching.
    pub fn utilization(&self) -> f64 {
        if self.constraints.max_context_tokens == 0 {
            return 0.0;
        }
        self.total_tokens() as f64 / self.constraints.max_context_tokens as f64
    }

    /// Calculate total estimated tokens in the conversation.
    /// Returns the cached value updated incrementally on each push.
    /// Use [`reconcile_token_count`] after mutations that bypass push methods.
    #[inline]
    pub fn total_tokens(&self) -> usize {
        self.cached_total_tokens
    }

    /// Recompute the cached token count from scratch by scanning all messages.
    /// Call this after mutations that bypass push methods (e.g., `normalize_history`,
    /// direct `messages` field access, or deserialization).
    pub fn reconcile_token_count(&mut self) {
        self.cached_total_tokens = self.messages.iter().map(|m| m.estimate_tokens()).sum();
    }

    /// Manually adjust the cached token count. Use when a message is added
    /// or removed outside of the standard push methods.
    #[inline]
    pub fn adjust_token_count(&mut self, delta: isize) {
        self.cached_total_tokens = (self.cached_total_tokens as isize)
            .saturating_add(delta)
            .max(0) as usize;
    }

    /// Pre-flight check: does the assembled prompt fit within the context window?
    ///
    /// Estimates total tokens for the full request (conversation history +
    /// system prompt + tool definitions) and compares against the available
    /// budget (`max_context_tokens - reserved_output_tokens`).
    ///
    /// Returns `(fits, estimated_total, available_budget)`.
    pub fn preflight_token_check(
        &self,
        system_prompt_tokens: usize,
        tool_def_tokens: usize,
        reserved_output_tokens: usize,
    ) -> (bool, usize, usize) {
        let budget = self
            .constraints
            .max_context_tokens
            .saturating_sub(reserved_output_tokens);
        let estimated = self
            .total_tokens()
            .saturating_add(system_prompt_tokens)
            .saturating_add(tool_def_tokens);
        (estimated <= budget, estimated, budget)
    }

    /// Find a safe split point for history trimming that doesn't break tool call/output pairs.
    pub fn find_safe_split_point(&self, preferred_split_at: usize) -> usize {
        crate::core::agent::state::safe_history_split_point(
            &self.messages,
            self.conversation.len(),
            preferred_split_at,
        )
    }

    /// Normalize history to enforce call/output pairing invariants.
    pub fn normalize(&mut self) {
        crate::core::agent::state::normalize_history(&mut self.messages);
        self.reconcile_token_count();
    }

    pub fn into_results(
        self,
        summary: String,
        thread_events: Vec<ThreadEvent>,
        total_duration_ms: u128,
    ) -> TaskResults {
        let average_turn_duration_ms = if self.turn_count > 0 {
            Some(self.turn_total_ms as f64 / self.turn_count as f64)
        } else {
            None
        };
        let max_turn_duration_ms = if self.turn_count > 0 {
            Some(self.turn_max_ms)
        } else {
            None
        };

        TaskResults {
            created_contexts: self.created_contexts,
            modified_files: self.modified_files,
            executed_commands: self.executed_commands,
            summary,
            stop_reason: self.stop_reason,
            total_cost_usd: self.total_cost_usd,
            warnings: self.warnings,
            thread_events,
            outcome: self.outcome,
            turns_executed: self.stats.turns_executed,
            total_duration_ms,
            average_turn_duration_ms,
            max_turn_duration_ms,
            turn_durations_ms: self.turn_durations_ms,
        }
    }

    /// Push a tool event (result or error) to both conversation (for Gemini) and messages.
    ///
    /// Shared implementation for `push_tool_result` and `push_tool_error` to
    /// eliminate the duplicated Gemini FunctionResponse construction.
    fn push_tool_event(
        &mut self,
        call_id: String,
        tool_name: &str,
        value: &serde_json::Value,
        is_gemini: bool,
    ) {
        if is_gemini {
            self.conversation.push(Content {
                role: "function".to_string(),
                parts: vec![Part::FunctionResponse {
                    function_response: FunctionResponse {
                        name: tool_name.to_string(),
                        response: value.clone(),
                        id: Some(call_id.clone()),
                    },
                    thought_signature: None,
                }],
            });
        }
        let serialized = serde_json::to_string(value).expect("Value serialization is infallible");
        let msg = Message::tool_response(call_id, serialized);
        let tokens = msg.estimate_tokens();
        self.cached_total_tokens = self.cached_total_tokens.saturating_add(tokens);
        self.messages.push(msg);
    }

    /// Push a successful tool result to both conversation (for Gemini) and messages.
    pub fn push_tool_result(
        &mut self,
        call_id: String,
        tool_name: &str,
        result: &serde_json::Value,
        is_gemini: bool,
    ) {
        self.push_tool_event(call_id, tool_name, result, is_gemini);
        self.executed_commands.push(tool_name.to_owned());
    }

    /// Push a tool error to both conversation (for Gemini) and messages.
    pub fn push_tool_error(
        &mut self,
        call_id: String,
        tool_name: &str,
        error_payload: &serde_json::Value,
        is_gemini: bool,
    ) {
        self.push_tool_event(call_id, tool_name, error_payload, is_gemini);
    }
}

#[cfg(test)]
mod tests {
    use super::AgentSessionState;
    use crate::llm::provider::Message;
    use crate::llm::providers::gemini::wire::Part;

    #[test]
    fn previous_response_chain_is_scoped_to_provider_and_model() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let messages_52 = vec![Message::user("hello".to_string())];
        let messages_54 = vec![Message::user("continue".to_string())];

        state.set_previous_response_chain(
            "openai",
            "gpt-5.2",
            Some("resp_123"),
            messages_52.clone(),
        );
        state.set_previous_response_chain(
            "openai",
            "gpt-5.4",
            Some("resp_456"),
            messages_54.clone(),
        );

        assert_eq!(
            state.previous_response_id_for("openai", "gpt-5.2"),
            Some("resp_123".to_string())
        );
        assert_eq!(
            state.previous_response_id_for("openai", "gpt-5.4"),
            Some("resp_456".to_string())
        );
        assert_eq!(state.previous_response_id_for("gemini", "gpt-5.2"), None);

        state.clear_previous_response_chain_for("openai", "gpt-5.2");

        assert_eq!(state.previous_response_id_for("openai", "gpt-5.2"), None);
        assert_eq!(state.previous_response_chain_for("openai", "gpt-5.2"), None);
        assert_eq!(
            state.previous_response_id_for("openai", "gpt-5.4"),
            Some("resp_456".to_string())
        );
        assert_eq!(
            state
                .previous_response_chain_for("openai", "gpt-5.4")
                .map(|chain| chain.messages.as_slice()),
            Some(messages_54.as_slice())
        );

        state.clear_previous_response_chain();
        assert_eq!(state.previous_response_id_for("openai", "gpt-5.4"), None);
        assert_eq!(state.previous_response_chain_for("openai", "gpt-5.4"), None);
    }

    #[test]
    fn register_tool_loop_tracks_current_and_max_streak() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);

        assert_eq!(state.register_tool_loop(), 1);
        assert_eq!(state.register_tool_loop(), 2);
        assert_eq!(state.consecutive_tool_loops, 2);
        assert_eq!(state.max_tool_loop_streak, 2);

        state.reset_tool_loop_guard();
        assert_eq!(state.register_tool_loop(), 1);
        assert_eq!(state.max_tool_loop_streak, 2);
    }

    #[test]
    fn push_tool_error_preserves_structured_json_for_gemini() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let payload = serde_json::json!({
            "error": {
                "tool_name": "read_file",
                "message": "missing file",
                "category": "ResourceNotFound"
            }
        });

        state.push_tool_error("call_1".to_string(), "read_file", &payload, true);

        match &state.conversation[0].parts[0] {
            Part::FunctionResponse {
                function_response, ..
            } => {
                assert_eq!(
                    function_response.response["error"]["message"],
                    "missing file"
                );
            }
            other => panic!("expected function response, got {other:?}"),
        }
        let expected_serialized = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            state.messages[0],
            Message::tool_response("call_1".to_string(), expected_serialized)
        );
    }

    #[test]
    fn cached_total_tokens_matches_direct_computation() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);

        // Add messages through the standard push methods
        state.add_user_message("Hello, how are you?".to_string());
        state.push_tool_result(
            "call_1".to_string(),
            "read_file",
            &serde_json::json!({"content": "test file content"}),
            false,
        );
        state.push_tool_error(
            "call_2".to_string(),
            "write_file",
            &serde_json::json!({"error": "permission denied"}),
            false,
        );

        // Cached value should match direct computation
        let direct = state
            .messages
            .iter()
            .map(|m| m.estimate_tokens())
            .sum::<usize>();
        assert_eq!(state.total_tokens(), direct);
        assert!(state.total_tokens() > 0);
    }

    #[test]
    fn reconcile_token_count_resyncs_after_external_mutation() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.add_user_message("test message".to_string());
        let before = state.total_tokens();

        // Simulate external mutation (bypassing push methods)
        state
            .messages
            .push(Message::assistant("extra response".to_string()));
        assert_ne!(
            state.total_tokens(),
            before + Message::assistant("extra response".to_string()).estimate_tokens()
        );

        // Reconcile should fix it
        state.reconcile_token_count();
        let expected = state
            .messages
            .iter()
            .map(|m| m.estimate_tokens())
            .sum::<usize>();
        assert_eq!(state.total_tokens(), expected);
    }
}
