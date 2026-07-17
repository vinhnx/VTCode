use super::AgentRunner;
use crate::compaction::auto::{AutoCompactionInput, auto_compact_messages};
use crate::compaction::memory_envelope::{MemoryEnvelopePlacement, local_compaction_config};
use crate::core::agent::compaction_checkpoint::write_compaction_checkpoint;
use crate::core::agent::context_reset::maybe_write_reset_after_compaction;
use crate::core::agent::conversation::conversation_from_messages;
use crate::core::agent::session::AgentSessionState;
use crate::exec::events::CompactionTrigger;
use tracing::{info, warn};

impl AgentRunner {
    /// Compress the conversation trace when automatic compaction should fire.
    ///
    /// Delegates to the shared compaction orchestrator (the same path the
    /// binary unified runloop uses) so both runloops keep identical
    /// continuity behavior: the provider-native or local-LLM summary is built,
    /// a recoverable full-history artifact is written, and a session memory
    /// envelope is injected so the model retains what it was doing. A
    /// `thread.compact_boundary` event is emitted and the Gemini `conversation`
    /// view is rebuilt from the compacted universal `messages`.
    pub(super) async fn maybe_auto_compact(
        &mut self,
        session_state: &mut AgentSessionState,
        event_recorder: &mut crate::core::agent::events::ExecEventRecorder,
        turn_model: &str,
        preserve_recent_turns: usize,
    ) {
        let mut engine_cfg = local_compaction_config(Some(self.config()), false);
        // Honor the existing `context.preserve_recent_turns` knob: keep at least
        // this many recent messages verbatim before any summarization fires.
        engine_cfg.keep_last_messages = preserve_recent_turns;
        let outcome = match auto_compact_messages(
            AutoCompactionInput {
                provider: self.provider_client.as_ref(),
                model: turn_model,
                session_id: &self.session_id,
                workspace_root: self._workspace.as_path(),
                vt_cfg: Some(self.config()),
                current_token_usage: session_state.total_tokens(),
                touched_files: &session_state.modified_files,
                engine_cfg,
                manual_options: crate::compaction::ManualCompactionOptions::default(),
                placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            },
            std::sync::Arc::make_mut(&mut session_state.messages),
        )
        .await
        {
            Ok(Some(outcome)) => outcome,
            Ok(None) => return,
            Err(error) => {
                warn!(
                    error = %error,
                    "Automatic context compaction failed; continuing with full history"
                );
                return;
            }
        };

        // Rebuild the Gemini `conversation` view from the compacted universal
        // `messages` and keep the processed-message cursor consistent.
        session_state.conversation = conversation_from_messages(&session_state.messages);
        session_state.normalize();
        session_state.last_processed_message_idx = session_state.conversation.len();

        // Write compaction summary to persistent artifacts for later sessions.
        // This follows the context engineering principle: "the summary should be
        // written into a persistent artifact, such as progress.md, so that later
        // sessions can read it."
        if let Some(ref envelope) = outcome.envelope {
            write_compaction_checkpoint(self._workspace.as_path(), envelope);
        }

        // If context reset is configured for on_compaction, write a reset
        // manifest so the next session starts from a clean context rather
        // than the compacted summary. This is distinct from compaction:
        // compaction preserves conversational continuity; context reset
        // deliberately discards it to clear noise and bad assumptions.
        let reset_mode = self.config().agent.harness.context_reset_mode.as_str();
        if maybe_write_reset_after_compaction(self._workspace.as_path(), reset_mode) {
            info!(
                "Context reset manifest written after compaction (mode: {})",
                reset_mode
            );
        }

        event_recorder.compact_boundary(
            CompactionTrigger::Auto,
            outcome.mode,
            outcome.original_len,
            outcome.compacted_len,
            outcome.history_artifact_path.as_deref(),
        );

        info!(
            provider = %self.provider_client.name(),
            model = turn_model,
            original_len = outcome.original_len,
            compacted_len = outcome.compacted_len,
            compaction_mode = %outcome.mode.as_str(),
            "Applied automatic conversation compaction (core loop)"
        );
    }
}
