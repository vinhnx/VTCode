//! Centralized agent session state management.

use crate::core::agent::error_recovery::ErrorRecoveryState;
use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::core::pending_actions::PendingActions;
use crate::core::state_schema::SchemaVersion;
use crate::exec::events::Usage;
use crate::llm::provider::{Message, ResponsesContinuationState, responses_continuation_key};
use crate::llm::providers::gemini::wire::{Content, FunctionResponse, Part};
use crate::llm::request_gap::RequestGapTracker;
use hashbrown::HashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vtcode_commons::ErrorCategory;
use vtcode_exec_events::ThreadEvent;

#[derive(Debug, Clone)]
pub struct ToolExecutionObservation {
    pub tool_name: String,
    pub attempts: u32,
    pub duration_ms: u64,
    pub error_category: Option<ErrorCategory>,
}

/// Manages the state of an active agent session, including conversation history,
/// statistics, and turn-based constraints.
pub struct AgentSessionState {
    /// The thread or session ID.
    pub session_id: String,

    /// Provider-specific conversation history (e.g., Gemini style).
    pub conversation: Vec<Content>,

    /// Standardized conversation messages (OpenAI/Anthropic style).
    ///
    /// Stored in an `Arc` so request construction can share the history with
    /// the provider without cloning the full conversation every turn.
    /// Mutations use copy-on-write; the common unique-owner path remains O(1).
    pub messages: Arc<Vec<Message>>,

    /// Schema version for durable state persistence.
    pub schema_version: SchemaVersion,

    /// Statistics for the current session.
    pub stats: SessionStats,
    /// Auto-compaction suppression state: `SUPPRESS_NONE` allows compaction;
    /// other values gate automatic compaction until cleared by success, model
    /// switch, or explicit `/compact`.
    pub auto_compact_suppressed: u8,

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
    pub progress_hashes: VecDeque<u64>,
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
    /// One canonical terminal observation per tool invocation in this turn.
    pub turn_tool_observations: Vec<ToolExecutionObservation>,
    /// Cached total estimated token count for the conversation history.
    /// Updated incrementally on each push to avoid O(n) scans per turn.
    cached_total_tokens: usize,

    /// Tracks the idle gap since the last dispatched LLM request, so a long
    /// enough pause can warn that the provider prompt cache has likely
    /// expired. Shared with the interactive session state; see
    /// [`RequestGapTracker`].
    request_gap: RequestGapTracker,

    /// Reasoning effort used for the last dispatched request, used to detect
    /// mid-task changes that invalidate the provider prompt cache.
    last_reasoning_effort: Option<crate::config::types::ReasoningEffortLevel>,
    /// Model used for the last dispatched request, used to detect mid-task
    /// model switches. Prompt caches are unique per model, so switching
    /// rebuilds the cache at full input cost even when the rest of the
    /// prefix is unchanged.
    last_model: Option<String>,
}

/// Statistics tracked during an agent session.
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub turns_executed: usize,
    pub total_duration: Duration,
    pub turn_durations: Vec<Duration>,
    pub total_usage: Usage,
    /// Provider name for the active session, used to normalize per-turn usage
    /// (see [`crate::llm::usage_cost::provider_reports_exclusive_input`]). An
    /// empty string is treated as a non-exclusive-input provider, which
    /// preserves existing behavior for callers that never set it.
    pub provider_name: String,
    /// Rolling prompt-cache health fed by every merged turn. Fires at most
    /// two session-scoped alerts (sustained misses, low hit rate) mirroring
    /// the "monitor cache hit rate like uptime" discipline.
    pub prompt_cache_health: super::cache_health::PromptCacheHealthMonitor,
}

impl SessionStats {
    /// Merge one turn's provider usage into the session totals and feed the
    /// prompt-cache health monitor. Returns a health alert the first time a
    /// degraded pattern is confirmed, `None` otherwise.
    pub fn merge_usage(
        &mut self,
        usage: &crate::llm::provider::Usage,
    ) -> Option<super::cache_health::CacheHealthAlert> {
        let normalized = crate::llm::usage_cost::normalized_turn_usage(&self.provider_name, usage);
        self.total_usage.add(&normalized);
        self.prompt_cache_health.record_turn(&normalized)
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
    pub fn new(session_id: String, max_turns: usize, max_tool_loops: usize, max_context_tokens: usize) -> Self {
        Self {
            session_id,
            schema_version: SchemaVersion::CURRENT,
            conversation: Vec::new(),
            messages: Arc::new(Vec::new()),
            stats: SessionStats::default(),
            auto_compact_suppressed: crate::compaction::SUPPRESS_NONE,
            constraints: SessionConstraints { max_turns, max_tool_loops, max_context_tokens },
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
            progress_hashes: VecDeque::with_capacity(16),
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
            turn_tool_observations: Vec::with_capacity(32),
            cached_total_tokens: 0,
            request_gap: RequestGapTracker::default(),
            last_reasoning_effort: None,
            last_model: None,
        }
    }

    /// Records that an LLM request was just dispatched, so the next call to
    /// [`Self::cache_gap_exceeds`] can measure the idle gap since this request.
    pub fn note_request_sent(&mut self) {
        self.request_gap.note_request_sent();
    }

    /// Returns the elapsed time since the last dispatched request when it
    /// exceeds `threshold`, or `None` if there was no prior request or the gap
    /// is still within the threshold. Used to warn that the provider prompt
    /// cache has likely expired before the next request re-pays full input
    /// cost.
    pub fn cache_gap_exceeds(&self, threshold: Duration) -> Option<Duration> {
        self.request_gap.cache_gap_exceeds(threshold)
    }

    /// Checks whether `effort` differs from the reasoning effort used for the
    /// previous request in this session, then stores `effort` as the new
    /// baseline. Returns `true` only when a prior effort was recorded and it
    /// differs from `effort` (i.e. this is a genuine mid-task change, not the
    /// first request of the session).
    pub fn note_reasoning_effort_change(&mut self, effort: Option<crate::config::types::ReasoningEffortLevel>) -> bool {
        let changed = matches!(
            (self.last_reasoning_effort, effort),
            (Some(previous), Some(current)) if previous != current
        );
        self.last_reasoning_effort = effort;
        changed
    }

    /// Checks whether `model` differs from the model used for the previous
    /// request in this session, then stores `model` as the new baseline.
    /// Returns `true` only when a prior model was recorded and it differs
    /// from `model` (i.e. this is a genuine mid-task switch, not the first
    /// request of the session). Prompt caches are unique per model, so a
    /// switch re-pays full input cost even for an otherwise identical prefix.
    pub fn note_model_change(&mut self, model: &str) -> bool {
        let changed = self.last_model.as_deref().is_some_and(|previous| previous != model);
        self.last_model = Some(model.to_string());
        changed
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
            self.outcome =
                TaskOutcome::tool_loop_limit_reached(self.constraints.max_tool_loops, self.consecutive_tool_loops);
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

    pub fn previous_response_chain_for(&self, provider: &str, model: &str) -> Option<&ResponsesContinuationState> {
        responses_continuation_key(provider, model).and_then(|key| self.previous_response_chains.get(&key))
    }

    pub fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: Vec<Message>,
    ) {
        self.set_previous_response_chain_shared(provider, model, response_id, Arc::new(messages));
    }

    /// Store a continuation history that is already reference-counted.
    ///
    /// The runtime keeps the exact sent request history alive for provider
    /// continuation without cloning every message a second time.
    pub(crate) fn set_previous_response_chain_shared(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: Arc<Vec<Message>>,
    ) {
        let Some(key) = responses_continuation_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.previous_response_chains.remove(&key);
            return;
        };

        self.previous_response_chains
            .insert(key, ResponsesContinuationState { response_id: response_id.to_string(), messages });
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
        self.outcome =
            TaskOutcome::tool_loop_limit_reached(self.constraints.max_tool_loops, self.consecutive_tool_loops);
    }

    /// Mutable access to the conversation history.
    ///
    /// Returns mutable history with copy-on-write when a request still shares it.
    #[inline]
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        Arc::make_mut(&mut self.messages)
    }

    /// Add a user message to the history with metadata.
    pub fn add_user_message(&mut self, text: String) {
        self.add_user_message_with_intent(text, None);
    }

    /// Add a harness-generated `User`-role notice (recovery nudge, tracker
    /// continuation, evaluator retry).
    ///
    /// The role must stay `User` so the provider treats it as steering, but the
    /// message is tagged synthetic so reverse history can skip it.
    pub fn add_synthetic_user_message(&mut self, text: String) {
        self.add_synthetic_user_message_with_intent(text, None);
    }

    /// Add a synthetic `User`-role message with its originating follow-up ID.
    pub fn add_synthetic_user_message_with_intent(&mut self, text: String, intent_id: Option<String>) {
        let metadata = Self::user_message_metadata(&text, intent_id, true);
        self.push_user_message(text, metadata);
    }

    /// Add a user message and optionally tag it with the steering intent that
    /// produced it. The tag survives message serialization for recovery.
    pub fn add_user_message_with_intent(&mut self, text: String, intent_id: Option<String>) {
        let metadata = Self::user_message_metadata(&text, intent_id, false);
        self.push_user_message(text, metadata);
    }

    fn user_message_metadata(
        text: &str,
        intent_id: Option<String>,
        synthetic: bool,
    ) -> crate::core::message_metadata::MessageMetadata {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let tokens = text.len().saturating_div(4); // rough estimate: ~4 chars per token
        let metadata = if synthetic {
            crate::core::message_metadata::MessageMetadata::harness_notice(now, tokens)
        } else {
            crate::core::message_metadata::MessageMetadata::user_input(now, tokens)
        };
        match intent_id {
            Some(id) => metadata.with_intent_id(id),
            None => metadata,
        }
    }

    fn push_user_message(&mut self, text: String, metadata: crate::core::message_metadata::MessageMetadata) {
        self.conversation.push(Content::user_text(text.as_str()));
        let msg = Message::user(text).with_metadata(metadata);
        // Role overhead (~4 tokens) + content tokens
        let msg_tokens = msg.estimate_tokens();
        self.cached_total_tokens = self.cached_total_tokens.saturating_add(msg_tokens);
        self.messages_mut().push(msg);
    }

    /// Threshold for consecutive identical progress hashes before stagnation is declared.
    const PROGRESS_STAGNATION_THRESHOLD: usize = 4;

    /// Compute a hash of the current assistant response content for progress tracking.
    fn assistant_response_hash(&self) -> Option<u64> {
        use crate::llm::provider::{MessageContent, MessageRole};
        let last_assistant = self.messages.iter().rev().find(|m| m.role == MessageRole::Assistant)?;
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
        if self.progress_hashes.back() == Some(&hash) {
            self.stagnant_turns += 1;
        } else {
            self.stagnant_turns = 0;
        }
        self.progress_hashes.push_back(hash);
        if self.progress_hashes.len() > 16 {
            self.progress_hashes.pop_front();
        }
        self.stagnant_turns >= Self::PROGRESS_STAGNATION_THRESHOLD
    }

    /// Attach metadata to the most recent message. Used by the execution loop
    /// to annotate LLM responses and tool results after they are pushed.
    pub fn attach_metadata_to_last(&mut self, source: &str, estimated_tokens: usize) {
        if let Some(last) = self.messages_mut().last_mut() {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let metadata = match source {
                "llm_response" => crate::core::message_metadata::MessageMetadata::llm_response(now, estimated_tokens),
                "tool_result" => crate::core::message_metadata::MessageMetadata::tool_result(now, estimated_tokens),
                "system" => crate::core::message_metadata::MessageMetadata::system(now, estimated_tokens),
                "synthetic" => crate::core::message_metadata::MessageMetadata::synthetic(now, estimated_tokens),
                _ => crate::core::message_metadata::MessageMetadata::user_input(now, estimated_tokens),
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
    /// Use [`Self::reconcile_token_count`] after mutations that bypass push methods.
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
        if delta >= 0 {
            // `delta >= 0` is checked above, so the conversion is infallible
            self.cached_total_tokens = self
                .cached_total_tokens
                .saturating_add(usize::try_from(delta).expect("delta >= 0 checked above"));
        } else {
            self.cached_total_tokens = self.cached_total_tokens.saturating_sub(delta.unsigned_abs());
        }
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
        let budget = self.constraints.max_context_tokens.saturating_sub(reserved_output_tokens);
        let estimated = self
            .total_tokens()
            .saturating_add(system_prompt_tokens)
            .saturating_add(tool_def_tokens);
        (estimated <= budget, estimated, budget)
    }

    /// Find a safe split point for history trimming that doesn't break tool call/output pairs.
    pub fn find_safe_split_point(&self, preferred_split_at: usize) -> usize {
        crate::core::agent::state::safe_history_split_point(&self.messages, self.conversation.len(), preferred_split_at)
    }

    /// Normalize history to enforce call/output pairing invariants.
    pub fn normalize(&mut self) {
        crate::core::agent::state::normalize_history(self.messages_mut());
        self.reconcile_token_count();
    }

    /// Clear all conversation history for a context reset.
    ///
    /// Following the context engineering pattern: "Context reset uses external
    /// artifacts as startup material to open a clean new context/session. It
    /// does not preserve the full conversation history."
    ///
    /// This clears `messages`, `conversation`, resets the token count, and
    /// resets the processed-message cursor. The orient context (injected via
    /// the system prompt) provides the agent with durable artifact references
    /// to reorient from. Response continuation chains are also cleared since
    /// they reference the discarded history.
    pub fn clear_conversation_history(&mut self) {
        self.messages_mut().clear();
        self.conversation.clear();
        self.cached_total_tokens = 0;
        self.last_processed_message_idx = 0;
        self.previous_response_chains.clear();
        self.consecutive_tool_loops = 0;
        self.tool_loop_limit_hit = false;
        self.consecutive_escalations = 0;
        self.progress_hashes.clear();
        self.stagnant_turns = 0;
        self.pending_actions = PendingActions::new(100);
        self.consecutive_idle_turns = 0;
        self.max_tool_loop_streak = 0;
        self.turn_tool_observations.clear();
        self.current_stage = None;
        self.auto_compact_suppressed = crate::compaction::SUPPRESS_NONE;
        self.error_recovery.lock().reset();
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
    fn push_tool_event(&mut self, call_id: String, tool_name: &str, value: &serde_json::Value, is_gemini: bool) {
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
        self.messages_mut().push(msg);
    }

    /// Push a successful tool result to both conversation (for Gemini) and messages.
    pub fn push_tool_result(&mut self, call_id: String, tool_name: &str, result: &serde_json::Value, is_gemini: bool) {
        self.push_tool_event(call_id, tool_name, result, is_gemini);
        // Deduplicate tool names — a long session may call the same tool
        // hundreds of times, and the results report only needs unique entries.
        let tool_name_owned = tool_name.to_owned();
        if !self.executed_commands.contains(&tool_name_owned) {
            self.executed_commands.push(tool_name_owned);
        }
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

    /// Record a session warning, skipping exact duplicates.
    ///
    /// In long-running sessions, the same warning can fire repeatedly (e.g.
    /// "Tool was rate limited; halting further tool calls this turn." on every
    /// rate-limited call). Deduplicating keeps the `TaskResults.warnings` Vec
    /// bounded and the evaluator prompt clean. The Vec is also capped at
    /// `MAX_SESSION_WARNINGS` as a safety net against unbounded growth from
    /// unique-but-repetitive warnings. When the cap is reached a single
    /// elision marker is appended so consumers know warnings were truncated
    /// rather than silently losing data.
    pub fn push_warning(&mut self, warning: impl Into<String>) {
        let warning = warning.into();
        if self.warnings.contains(&warning) {
            return;
        }
        if self.warnings.len() < MAX_SESSION_WARNINGS {
            self.warnings.push(warning);
        } else if self.warnings.last().is_none_or(|w| w != WARNINGS_ELIDED_MARKER) {
            self.warnings.push(WARNINGS_ELIDED_MARKER.to_string());
        }
    }
}

/// Maximum number of warnings retained in session state. Exact duplicates are
/// always skipped; this cap limits unique-but-repetitive warnings. One extra
/// elision marker may be appended beyond this count.
const MAX_SESSION_WARNINGS: usize = 200;
const WARNINGS_ELIDED_MARKER: &str = "[Additional unique warnings elided — session warning cap reached]";

#[cfg(test)]
mod tests {
    use super::AgentSessionState;
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::Message;
    use crate::llm::providers::gemini::wire::Part;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    /// `cache_gap_exceeds`/`note_request_sent` delegate to the shared
    /// `RequestGapTracker`, which owns the full behavioral test coverage
    /// (see `crate::llm::request_gap`); this just pins the delegation.
    #[test]
    fn cache_gap_exceeds_delegates_to_request_gap_tracker() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert_eq!(state.cache_gap_exceeds(Duration::from_millis(1)), None);

        state.note_request_sent();
        thread::sleep(Duration::from_millis(15));
        let gap = state.cache_gap_exceeds(Duration::from_millis(5));
        assert!(gap.is_some_and(|elapsed| elapsed >= Duration::from_millis(15)));
    }

    #[test]
    fn note_reasoning_effort_change_is_false_on_first_request() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
    }

    #[test]
    fn note_reasoning_effort_change_is_false_when_unchanged() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
        assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
    }

    #[test]
    fn note_reasoning_effort_change_is_true_when_changed() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
        assert!(state.note_reasoning_effort_change(Some(ReasoningEffortLevel::High)));
        // Baseline is now High; requesting High again is not a change.
        assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::High)));
    }

    #[test]
    fn note_model_change_is_false_on_first_request() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_model_change("gpt-5.6"));
    }

    #[test]
    fn note_model_change_is_false_when_unchanged() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_model_change("gpt-5.6"));
        assert!(!state.note_model_change("gpt-5.6"));
    }

    #[test]
    fn note_model_change_is_true_when_changed() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        assert!(!state.note_model_change("gpt-5.6"));
        assert!(state.note_model_change("claude-opus-4-6"));
        // Baseline is now Opus; requesting Opus again is not a change.
        assert!(!state.note_model_change("claude-opus-4-6"));
    }

    #[test]
    fn previous_response_chain_is_scoped_to_provider_and_model() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let messages_52 = vec![Message::user("hello".to_string())];
        let messages_54 = vec![Message::user("continue".to_string())];

        state.set_previous_response_chain("openai", "gpt-5.6", Some("resp_123"), messages_52.clone());
        state.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_456"), messages_54.clone());

        assert_eq!(state.previous_response_id_for("openai", "gpt-5.6"), Some("resp_123".to_string()));
        assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), Some("resp_456".to_string()));
        assert_eq!(state.previous_response_id_for("gemini", "gpt-5.6"), None);

        state.clear_previous_response_chain_for("openai", "gpt-5.6");

        assert_eq!(state.previous_response_id_for("openai", "gpt-5.6"), None);
        assert_eq!(state.previous_response_chain_for("openai", "gpt-5.6"), None);
        assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), Some("resp_456".to_string()));
        assert_eq!(
            state
                .previous_response_chain_for("openai", "gpt-5.6-sol")
                .map(|chain| chain.messages.as_slice()),
            Some(messages_54.as_slice())
        );

        state.clear_previous_response_chain();
        assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), None);
        assert_eq!(state.previous_response_chain_for("openai", "gpt-5.6-sol"), None);
    }

    #[test]
    fn shared_previous_response_chain_reuses_history_arc() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let messages = Arc::new(vec![Message::user("hello".to_string())]);

        state.set_previous_response_chain_shared("openai", "gpt-5.6-sol", Some("resp_123"), Arc::clone(&messages));

        let stored = &state
            .previous_response_chain_for("openai", "gpt-5.6-sol")
            .expect("continuation state")
            .messages;
        assert!(Arc::ptr_eq(&messages, stored));
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
            Part::FunctionResponse { function_response, .. } => {
                assert_eq!(function_response.response["error"]["message"], "missing file");
            }
            other => panic!("expected function response, got {other:?}"),
        }
        let expected_serialized = serde_json::to_string(&payload).unwrap();
        assert_eq!(state.messages[0], Message::tool_response("call_1".to_string(), expected_serialized));
    }

    #[test]
    fn harness_notices_are_tagged_synthetic_while_user_messages_are_not() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.add_user_message("fix the flaky test".to_string());
        state.add_synthetic_user_message("It looks like you're repeating the same response.".to_string());
        state.add_user_message_with_intent("steer: stop after this".to_string(), Some("intent-1".to_string()));

        let sources: Vec<Option<&str>> = state
            .messages
            .iter()
            .map(|message| message.metadata.as_ref().and_then(|metadata| metadata.source()))
            .collect();
        assert_eq!(sources, vec![Some("user_input"), Some("synthetic"), Some("user_input")]);
        assert!(
            state.messages[1]
                .metadata
                .as_ref()
                .is_some_and(vtcode_commons::message_metadata::MessageMetadata::is_synthetic)
        );
        // Tags survive archive round-trips so reverse history can filter them.
        let archived: Vec<crate::utils::session_archive::SessionMessage> =
            state.messages.iter().map(Into::into).collect();
        assert!(
            archived[1]
                .metadata
                .as_deref()
                .is_some_and(vtcode_commons::message_metadata::MessageMetadata::is_synthetic)
        );
        assert!(archived[0].metadata.as_deref().is_some_and(|m| !m.is_synthetic()));
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
        let direct = state.messages.iter().map(|m| m.estimate_tokens()).sum::<usize>();
        assert_eq!(state.total_tokens(), direct);
        assert!(state.total_tokens() > 0);
    }

    #[test]
    fn reconcile_token_count_resyncs_after_external_mutation() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.add_user_message("test message".to_string());
        let before = state.total_tokens();

        // Simulate external mutation (bypassing push methods)
        state.messages_mut().push(Message::assistant("extra response".to_string()));
        assert_ne!(state.total_tokens(), before + Message::assistant("extra response".to_string()).estimate_tokens());

        // Reconcile should fix it
        state.reconcile_token_count();
        let expected = state.messages.iter().map(|m| m.estimate_tokens()).sum::<usize>();
        assert_eq!(state.total_tokens(), expected);
    }

    #[test]
    fn clear_conversation_history_resets_all_state() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.messages_mut().push(Message::user("hello".to_string()));
        state.messages_mut().push(Message::assistant("hi".to_string()));
        state.conversation.push(crate::llm::providers::gemini::wire::Content {
            role: "user".to_string(),
            parts: vec![Part::Text { text: "hello".to_string(), thought_signature: None }],
        });
        state.reconcile_token_count();
        state.last_processed_message_idx = 2;
        state.progress_hashes.push_back(123);
        state.stagnant_turns = 3;
        state.set_previous_response_chain("openai", "gpt-5", Some("resp_1"), vec![]);

        assert!(!state.messages.is_empty());
        assert!(!state.conversation.is_empty());
        assert!(state.total_tokens() > 0);

        state.clear_conversation_history();

        assert!(state.messages.is_empty());
        assert!(state.conversation.is_empty());
        assert_eq!(state.total_tokens(), 0);
        assert_eq!(state.last_processed_message_idx, 0);
        assert!(state.progress_hashes.is_empty());
        assert_eq!(state.stagnant_turns, 0);
        assert!(state.previous_response_chains.is_empty());
    }

    #[test]
    fn push_warning_skips_exact_duplicates() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.push_warning("rate limited");
        state.push_warning("rate limited");
        state.push_warning("rate limited");
        assert_eq!(state.warnings.len(), 1);
        assert_eq!(state.warnings[0], "rate limited");
    }

    #[test]
    fn push_warning_retains_distinct_warnings() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        state.push_warning("first warning");
        state.push_warning("second warning");
        state.push_warning("third warning");
        assert_eq!(state.warnings.len(), 3);
        assert_eq!(state.warnings[0], "first warning");
        assert_eq!(state.warnings[1], "second warning");
        assert_eq!(state.warnings[2], "third warning");
    }

    #[test]
    fn push_warning_inserts_elision_marker_at_cap() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        // Fill up to the cap with unique warnings.
        for i in 0..super::MAX_SESSION_WARNINGS {
            state.push_warning(format!("warning {i}"));
        }
        assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS);
        assert!(!state.warnings.iter().any(|w| w == super::WARNINGS_ELIDED_MARKER));

        // Next unique warning triggers the marker exactly once.
        state.push_warning("overflow A");
        assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS + 1);
        assert_eq!(state.warnings.last().unwrap(), super::WARNINGS_ELIDED_MARKER);

        // Further unique warnings are silently dropped — marker stays as the
        // last entry and the count does not grow.
        state.push_warning("overflow B");
        state.push_warning("overflow C");
        assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS + 1);
        assert_eq!(state.warnings.last().unwrap(), super::WARNINGS_ELIDED_MARKER);
        // The overflow warnings themselves were not stored.
        assert!(!state.warnings.iter().any(|w| w == "overflow A"));
        assert!(!state.warnings.iter().any(|w| w == "overflow B"));
    }
}
