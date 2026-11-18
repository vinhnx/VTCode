//! Centralized agent session state management.

use crate::core::agent::error_recovery::ErrorRecoveryState;
use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::core::pending_actions::PendingActions;
use crate::core::state_schema::SchemaVersion;
use crate::exec::events::Usage;
use crate::llm::provider::{Message, ResponsesContinuationState, ResponsesContinuationStore};
use crate::llm::providers::gemini::wire::{Content, FunctionResponse, Part};
use crate::llm::request_gap::RequestGapTracker;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vtcode_commons::ErrorCategory;
use vtcode_commons::tokens::heuristic_tokens_from_byte_len;
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
    pub previous_response_chains: ResponsesContinuationStore,
    /// Agent-local recent error diagnostics for interrupted or repeated tool failures.
    pub error_recovery: Arc<Mutex<ErrorRecoveryState>>,
    /// Pending tool actions that have been issued but not yet returned.
    pub pending_actions: PendingActions,

    // Legacy / Stats fields for compatibility
    pub consecutive_idle_turns: usize,
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
    pub max_context_tokens: usize,
}

impl AgentSessionState {
    pub fn new(session_id: String, max_turns: usize, max_context_tokens: usize) -> Self {
        Self {
            session_id,
            schema_version: SchemaVersion::CURRENT,
            conversation: Vec::new(),
            messages: Arc::new(Vec::new()),
            stats: SessionStats::default(),
            auto_compact_suppressed: crate::compaction::SUPPRESS_NONE,
            constraints: SessionConstraints { max_turns, max_context_tokens },
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
            consecutive_escalations: 0,
            progress_hashes: VecDeque::with_capacity(16),
            stagnant_turns: 0,
            last_processed_message_idx: 0,
            previous_response_chains: ResponsesContinuationStore::default(),
            error_recovery: Arc::new(Mutex::new(ErrorRecoveryState::default())),
            pending_actions: PendingActions::new(100),
            consecutive_idle_turns: 0,
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
        if self.is_completed {
            self.outcome = TaskOutcome::Success;
        } else if self.stats.turns_executed >= max_turns {
            self.outcome = TaskOutcome::turn_limit_reached(max_turns, self.stats.turns_executed);
        }
    }

    pub fn previous_response_id_for(&self, provider: &str, model: &str) -> Option<String> {
        self.previous_response_chain_for(provider, model)
            .map(|chain| chain.response_id.clone())
    }

    pub fn previous_response_chain_for(&self, provider: &str, model: &str) -> Option<&ResponsesContinuationState> {
        self.previous_response_chains.get(provider, model)
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
        self.previous_response_chains.set(provider, model, response_id, messages);
    }

    pub fn clear_previous_response_chain_for(&mut self, provider: &str, model: &str) {
        self.previous_response_chains.clear_for(provider, model);
    }

    pub fn clear_previous_response_chain(&mut self) {
        self.previous_response_chains.clear();
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
        let tokens = heuristic_tokens_from_byte_len(text.len());
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
        self.consecutive_escalations = 0;
        self.progress_hashes.clear();
        self.stagnant_turns = 0;
        self.pending_actions = PendingActions::new(100);
        self.consecutive_idle_turns = 0;
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
mod tests;
