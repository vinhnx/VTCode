//! Centralized agent session state management.

use crate::core::agent::error_recovery::ErrorRecoveryState;
use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::exec::events::Usage;
use crate::llm::provider::Message;
use crate::llm::providers::gemini::wire::{Content, FunctionResponse, Part};
use hashbrown::HashMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    pub last_processed_message_idx: usize,
    /// Responses-style continuation pointers keyed by normalized provider/model pairs.
    pub previous_response_ids: HashMap<(String, String), String>,
    /// Agent-local recent error diagnostics for interrupted or repeated tool failures.
    pub error_recovery: Arc<Mutex<ErrorRecoveryState>>,

    // Legacy / Stats fields for compatibility
    pub consecutive_idle_turns: usize,
    pub max_tool_loop_streak: usize,
    pub turn_count: usize,
    pub turn_total_ms: u128,
    pub turn_max_ms: u128,
    pub turn_durations_ms: Vec<u128>,
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
            last_processed_message_idx: 0,
            previous_response_ids: HashMap::new(),
            error_recovery: Arc::new(Mutex::new(ErrorRecoveryState::default())),
            consecutive_idle_turns: 0,
            max_tool_loop_streak: 0,
            turn_count: 0,
            turn_total_ms: 0,
            turn_max_ms: 0,
            turn_durations_ms: Vec::with_capacity(max_turns),
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
        if self.outcome == TaskOutcome::Unknown {
            if self.is_completed {
                self.outcome = TaskOutcome::Success;
            } else if self.tool_loop_limit_hit {
                self.outcome = TaskOutcome::tool_loop_limit_reached(
                    self.constraints.max_tool_loops,
                    self.consecutive_tool_loops,
                );
            } else if self.stats.turns_executed >= max_turns {
                self.outcome =
                    TaskOutcome::turn_limit_reached(max_turns, self.stats.turns_executed);
            }
        }
    }

    pub fn register_tool_loop(&mut self) -> usize {
        self.consecutive_tool_loops += 1;
        self.consecutive_tool_loops
    }

    pub fn reset_tool_loop_guard(&mut self) {
        self.consecutive_tool_loops = 0;
    }

    pub fn previous_response_id_for(&self, provider: &str, model: &str) -> Option<String> {
        previous_response_chain_key(provider, model)
            .and_then(|key| self.previous_response_ids.get(&key).cloned())
    }

    pub fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
    ) {
        let Some(key) = previous_response_chain_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.previous_response_ids.remove(&key);
            return;
        };

        self.previous_response_ids
            .insert(key, response_id.to_string());
    }

    pub fn clear_previous_response_chain_for(&mut self, provider: &str, model: &str) {
        if let Some(key) = previous_response_chain_key(provider, model) {
            self.previous_response_ids.remove(&key);
        }
    }

    pub fn clear_previous_response_chain(&mut self) {
        self.previous_response_ids.clear();
    }

    pub fn mark_tool_loop_limit_hit(&mut self) {
        self.tool_loop_limit_hit = true;
        self.outcome = TaskOutcome::tool_loop_limit_reached(
            self.constraints.max_tool_loops,
            self.consecutive_tool_loops,
        );
    }

    /// Add a user message to the history.
    pub fn add_user_message(&mut self, text: String) {
        self.conversation.push(Content::user_text(text.clone()));
        self.messages.push(Message::user(text));
    }

    /// Check if context limits are approaching.
    pub fn utilization(&self) -> f64 {
        if self.constraints.max_context_tokens == 0 {
            return 0.0;
        }
        self.total_tokens() as f64 / self.constraints.max_context_tokens as f64
    }

    /// Calculate total estimated tokens in the conversation.
    pub fn total_tokens(&self) -> usize {
        self.messages.iter().map(|m| m.estimate_tokens()).sum()
    }

    /// Find a safe split point for history trimming that doesn't break tool call/output pairs.
    pub fn find_safe_split_point(&self, preferred_split_at: usize) -> usize {
        if preferred_split_at == 0 || preferred_split_at >= self.conversation.len() {
            return preferred_split_at;
        }

        let mut call_indices: HashMap<&str, usize> = HashMap::new();
        for (i, msg) in self.messages.iter().enumerate() {
            if let Some(tool_calls) = &msg.tool_calls {
                for call in tool_calls {
                    call_indices.insert(&call.id, i);
                }
            }
        }

        let mut safe_split_at = preferred_split_at;

        loop {
            if safe_split_at == 0 {
                break;
            }

            let has_orphan = ((safe_split_at + 1)..self.messages.len()).any(|i| {
                self.messages
                    .get(i)
                    .and_then(|msg| msg.tool_call_id.as_ref())
                    .and_then(|id| call_indices.get(id.as_str()))
                    .is_some_and(|&call_idx| call_idx <= safe_split_at)
            });

            if !has_orphan {
                break;
            }

            safe_split_at -= 1;
        }

        safe_split_at
    }

    /// Normalize history to enforce call/output pairing invariants.
    pub fn normalize(&mut self) {
        crate::core::agent::state::normalize_history(&mut self.messages);
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

    /// Push a successful tool result to both conversation (for Gemini) and messages.
    pub fn push_tool_result(
        &mut self,
        call_id: String,
        tool_name: &str,
        serialized_result: String,
        is_gemini: bool,
    ) {
        if is_gemini {
            let response_value = serde_json::from_str(&serialized_result)
                .unwrap_or_else(|_| serde_json::json!({ "result": serialized_result }));

            self.conversation.push(Content {
                role: "function".to_string(),
                parts: vec![Part::FunctionResponse {
                    function_response: FunctionResponse {
                        name: tool_name.to_string(),
                        response: response_value,
                        id: Some(call_id.clone()),
                    },
                    thought_signature: None,
                }],
            });
        }
        self.messages
            .push(Message::tool_response(call_id, serialized_result));
        self.executed_commands.push(tool_name.to_owned());
    }

    /// Push a tool error to both conversation (for Gemini) and messages.
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
                    function_response: FunctionResponse {
                        name: tool_name.to_string(),
                        response: serde_json::json!({ "error": error_msg }),
                        id: Some(call_id.clone()),
                    },
                    thought_signature: None,
                }],
            });
        }
        self.messages
            .push(Message::tool_response(call_id, error_msg));
    }
}

fn previous_response_chain_key(provider: &str, model: &str) -> Option<(String, String)> {
    let provider = provider.trim().to_ascii_lowercase();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }

    Some((provider, model.to_string()))
}

#[cfg(test)]
mod tests {
    use super::AgentSessionState;

    #[test]
    fn previous_response_chain_is_scoped_to_provider_and_model() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);

        state.set_previous_response_chain("openai", "gpt-5.2", Some("resp_123"));
        state.set_previous_response_chain("openai", "gpt-5.4", Some("resp_456"));

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
        assert_eq!(
            state.previous_response_id_for("openai", "gpt-5.4"),
            Some("resp_456".to_string())
        );

        state.clear_previous_response_chain();
        assert_eq!(state.previous_response_id_for("openai", "gpt-5.4"), None);
    }
}

// TODO: Move history invariant validation logic from state.rs here or shared module
