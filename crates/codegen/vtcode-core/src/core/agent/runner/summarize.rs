use super::AgentRunner;
use crate::compaction::CompactionParentContext;
use crate::compaction::auto::{AutoCompactionInput, auto_compact_messages};
use crate::compaction::memory_envelope::{MemoryEnvelopePlacement, local_compaction_config};
use crate::core::agent::compaction_checkpoint::write_compaction_checkpoint_async;
use crate::core::agent::context_reset::maybe_write_reset_after_compaction_with_context_async;
use crate::core::agent::conversation::conversation_from_messages;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::state::{
    clear_old_tool_results_with_recent_batches, normalize_history_for_request_shared,
    resolve_tool_result_clearing_trigger, should_apply_local_tool_result_clearing,
};
use crate::exec::events::CompactionTrigger;
use crate::llm::provider::Message;
use std::sync::Arc;
use tracing::{info, warn};

impl AgentRunner {
    pub(super) fn project_request_history(
        &self,
        session_state: &AgentSessionState,
        turn_model: &str,
        prompt_overhead_tokens: usize,
        reserved_output_tokens: usize,
    ) -> Arc<Vec<Message>> {
        let normalized = normalize_history_for_request_shared(Arc::clone(&session_state.messages));
        let clearing = &self.config().agent.harness.tool_result_clearing;
        // Headless requests attach no native context_management edits. A
        // provider capability alone must not suppress the local fallback.
        if !should_apply_local_tool_result_clearing(self.provider_client.name(), false, clearing.enabled) {
            return normalized;
        }
        Arc::new(clear_old_tool_results_with_recent_batches(
            &normalized,
            resolve_tool_result_clearing_trigger(
                self.config(),
                self.provider_client.as_ref(),
                turn_model,
                prompt_overhead_tokens,
                reserved_output_tokens,
            ),
            clearing.keep_tool_uses,
            clearing.keep_recent_tool_batches,
            clearing.clear_at_least_tokens,
            clearing.clear_tool_inputs,
        ))
    }

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
        projected_token_usage: usize,
        reserved_output_tokens: usize,
        request_envelope: &mut crate::core::agent::request_envelope::SessionRequestEnvelope,
    ) -> bool {
        let mut engine_cfg = local_compaction_config(Some(self.config()), false);
        // Honor the existing `context.preserve_recent_turns` knob: keep at least
        // this many recent messages verbatim before any summarization fires.
        engine_cfg.keep_last_messages = preserve_recent_turns;
        let input = AutoCompactionInput {
            provider: self.provider_client.as_ref(),
            model: turn_model,
            session_id: &self.session_id,
            workspace_root: self._workspace.as_path(),
            vt_cfg: Some(self.config()),
            current_token_usage: projected_token_usage,
            reserved_output_tokens,
            touched_files: &session_state.modified_files,
            engine_cfg,
            manual_options: crate::compaction::ManualCompactionOptions::default(),
            placement: MemoryEnvelopePlacement::BeforeLastUserOrSummary,
            prefire: None,
            auto_compact_suppressed: &mut session_state.auto_compact_suppressed,
            force_compaction: false,
            steering_update: None,
            // Cache-safe forking: reuse the live segment's exact system
            // prompt + ordered tools so the compaction fork hits the
            // provider prompt cache instead of re-paying full input cost.
            parent_context: Some(CompactionParentContext {
                system_prompt: Some(request_envelope.system_prompt()),
                tools: {
                    let tools = request_envelope.ordered_tools();
                    (!tools.is_empty()).then_some(tools)
                },
            }),
        };
        if !input.should_compact() {
            return false;
        }
        let outcome = match auto_compact_messages(input, Arc::make_mut(&mut session_state.messages)).await {
            Ok(Some(outcome)) => {
                session_state.stats.prompt_cache_health.record_compaction_boundary();
                outcome
            }
            Ok(None) => return false,
            Err(error) => {
                warn!(
                    error = ?error,
                    "Automatic context compaction failed; continuing with full history"
                );
                return false;
            }
        };
        session_state.stats.prompt_cache_health.record_compaction_boundary();

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
            write_compaction_checkpoint_async(self._workspace.as_path(), envelope).await;
        }

        let previous_envelope = request_envelope.clone();
        let next_segment_id = format!("{}-compact-{}", previous_envelope.segment_id(), outcome.original_len);
        *request_envelope = previous_envelope.begin_segment(next_segment_id);

        // If context reset is configured for on_compaction, write a reset
        // manifest so the next session starts from a clean context rather
        // than the compacted summary. This is distinct from compaction:
        // compaction preserves conversational continuity; context reset
        // deliberately discards it to clear noise and bad assumptions.
        let reset_mode = self.config().agent.harness.context_reset_mode.as_str().to_owned();
        let workspace = self._workspace.clone();
        let mut checkpoint_paths = outcome.history_artifact_path.iter().cloned().collect::<Vec<_>>();
        if outcome.envelope.is_some() {
            checkpoint_paths.extend([
                "memories/progress.md".to_string(),
                "memories/compaction_summary.md".to_string(),
            ]);
        }
        match maybe_write_reset_after_compaction_with_context_async(
            workspace.as_path(),
            &reset_mode,
            Some(self.session_id.clone()),
            Some(format!("compaction-{}", outcome.original_len)),
            checkpoint_paths,
            Some(format!("segment:{}", request_envelope.segment_id())),
        )
        .await
        {
            Ok(true) => info!("Context reset manifest written after compaction (mode: {})", reset_mode),
            Ok(false) => {}
            Err(error) => warn!(error = ?error, "Failed to write context reset manifest after compaction"),
        }

        event_recorder.compact_boundary(
            CompactionTrigger::Auto,
            outcome.mode,
            outcome.original_len,
            outcome.compacted_len,
            outcome.history_artifact_path.as_deref(),
            Some(&previous_envelope),
            Some(request_envelope),
        );

        info!(
            provider = %self.provider_client.name(),
            model = turn_model,
            original_len = outcome.original_len,
            compacted_len = outcome.compacted_len,
            compaction_mode = %outcome.mode.as_str(),
            "Applied automatic conversation compaction (core loop)"
        );
        true
    }
}
