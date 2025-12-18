use crate::core::agent::task::{TaskOutcome, TaskResults};
use crate::exec::events::ThreadEvent;
use crate::gemini::{Content, Part};
use crate::llm::provider::Message;
use std::time::Duration;

use crate::core::agent::conversation::build_messages_from_conversation;

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
}

impl TaskRunState {
    pub fn new(
        conversation: Vec<Content>,
        conversation_messages: Vec<Message>,
        max_tool_loops: usize,
    ) -> Self {
        Self {
            conversation,
            conversation_messages,
            created_contexts: Vec::with_capacity(16), // Typical session creates ~5-10 contexts
            modified_files: Vec::with_capacity(32),   // Typical session modifies ~10-20 files
            executed_commands: Vec::with_capacity(64), // Typical session executes ~20-40 commands
            warnings: Vec::with_capacity(16),         // Typical session has ~5-10 warnings
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
        }
    }

    pub fn record_turn(&mut self, start: &std::time::Instant, recorded: &mut bool) {
        record_turn_duration(&mut self.turn_durations_ms, recorded, start);
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

        let split_at = self
            .conversation
            .len()
            .saturating_sub(preserve_recent_turns);
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
}
