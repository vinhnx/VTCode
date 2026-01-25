use super::AgentRunner;
use crate::core::agent::conversation::build_messages_from_conversation;
use crate::core::agent::state::TaskRunState;
use crate::gemini::{Content, Part};
use tracing::{info, warn};

impl AgentRunner {
    pub(super) fn summarize_conversation_if_needed(
        &self,
        system_instruction: &str,
        task_state: &mut TaskRunState,
        preserve_recent_turns: usize,
        utilization: f64,
    ) {
        if utilization < 0.90 {
            return;
        }

        if task_state.conversation.len() <= preserve_recent_turns {
            return;
        }

        let preferred_split_at = task_state
            .conversation
            .len()
            .saturating_sub(preserve_recent_turns);

        // Context Manager: Find a safe split point that doesn't break tool call/output pairs.
        let split_at = task_state.find_safe_split_point(preferred_split_at);

        if split_at == 0 {
            return;
        }

        // Dynamic context discovery: Write full history to file before summarization
        // This allows the agent to recover details via grep_file if needed
        let history_file_path = self.persist_history_before_summarization(
            &task_state.conversation[..split_at],
            task_state.turns_executed,
            &task_state.modified_files,
            &task_state.executed_commands,
        );

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

        let base_summary = format!(
            "Summarized {} earlier turns to stay within context budget. Files: {}; Commands: {}; Warnings: {}.",
            split_at,
            summarize_list(&task_state.modified_files),
            summarize_list(&task_state.executed_commands),
            summarize_list(
                &task_state
                    .warnings
                    .iter()
                    .map(|w| w.to_string())
                    .collect::<Vec<_>>()
            ),
        );

        // Include history file reference in summary if available
        let summary = if let Some(path) = history_file_path {
            format!(
                "{}\n\nFull conversation history saved to: {}\nUse grep_file to search for specific details if needed.",
                base_summary,
                path.display()
            )
        } else {
            base_summary
        };

        let mut new_conversation = Vec::with_capacity(1 + preserve_recent_turns);
        new_conversation.push(Content::user_parts(vec![Part::Text {
            text: summary,
            thought_signature: None,
        }]));
        new_conversation.extend_from_slice(&task_state.conversation[split_at..]);
        task_state.conversation = new_conversation;
        task_state.conversation_messages =
            build_messages_from_conversation(system_instruction, &task_state.conversation);

        // Context Manager: Ensure history invariants are maintained after summarization.
        task_state.normalize();

        task_state.last_processed_message_idx = task_state.conversation.len();
    }

    /// Persist conversation history to a file before summarization
    ///
    /// This implements Cursor-style dynamic context discovery: full history
    /// is written to `.vtcode/history/` so the agent can recover details
    /// via grep_file if the summary loses important information.
    pub(super) fn persist_history_before_summarization(
        &self,
        conversation: &[Content],
        turn_number: usize,
        modified_files: &[String],
        executed_commands: &[String],
    ) -> Option<std::path::PathBuf> {
        use crate::context::history_files::{HistoryFileManager, content_to_history_messages};

        // Create history manager for this session
        let mut manager = HistoryFileManager::new(&self._workspace, &self.session_id);

        // Convert conversation to history messages
        let messages = content_to_history_messages(conversation, 0);

        // Write history file
        match manager.write_history_sync(
            &messages,
            turn_number,
            "summarization",
            modified_files,
            executed_commands,
        ) {
            Ok(result) => {
                info!(
                    path = %result.file_path.display(),
                    messages = result.metadata.message_count,
                    "Persisted conversation history before summarization"
                );
                Some(result.file_path)
            }
            Err(e) => {
                warn!(error = %e, "Failed to persist conversation history before summarization");
                None
            }
        }
    }
}
