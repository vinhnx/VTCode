//! Bounded compaction before the post-tool tool-enabled retry.
//!
//! This boundary keeps recovery-specific history preservation and turn-index
//! rebasing out of the main turn loop. The request type is deliberately
//! explicit: recovery compaction may rewrite only the older prefix and must
//! receive the live memory-envelope and session-stat dependencies it updates.

use anyhow::Result;
use vtcode_core::llm::provider as uni;

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::turn::compaction::{
    CompactionContext, CompactionOutcome, CompactionState, SessionMemoryEnvelopeUpdate,
    compact_history_for_recovery_in_place,
};
use crate::agent::runloop::unified::turn::context::last_user_message_index;

pub(super) struct RecoveryCompactionRequest<'a> {
    pub(super) history: &'a mut Vec<uni::Message>,
    pub(super) turn_history_start_len: &'a mut usize,
    pub(super) compaction_context: CompactionContext<'a>,
    pub(super) session_stats: &'a mut SessionStats,
    pub(super) context_manager: &'a mut ContextManager,
    pub(super) steering_update: SessionMemoryEnvelopeUpdate,
}

/// Compact the older history prefix while preserving the current request and
/// its tool results. Returns the engine outcome when the prefix was replaced,
/// so the caller can render the authoritative `Context compacted` line.
pub(super) async fn compact_before_tool_enabled_retry(
    request: RecoveryCompactionRequest<'_>,
) -> Result<Option<CompactionOutcome>> {
    let RecoveryCompactionRequest {
        history,
        turn_history_start_len,
        compaction_context,
        session_stats,
        context_manager,
        steering_update,
    } = request;
    let original_history_len = history.len();
    let preserve_from_index = current_turn_preserve_index(history, *turn_history_start_len);
    let outcome = compact_history_for_recovery_in_place(
        compaction_context,
        CompactionState::new(history, session_stats, context_manager).with_steering_update(steering_update),
        preserve_from_index,
    )
    .await?;

    let Some(outcome) = outcome else {
        tracing::debug!(preserve_from_index, "Post-tool recovery compaction had no reducible prefix");
        return Ok(None);
    };

    let preserved_suffix_len = original_history_len.saturating_sub(preserve_from_index);
    *turn_history_start_len = outcome.compacted_len.saturating_sub(preserved_suffix_len);
    tracing::info!(
        original_len = outcome.original_len,
        compacted_len = outcome.compacted_len,
        preserve_from_index,
        turn_history_start_len = *turn_history_start_len,
        "Compacted the older prefix before the bounded tool-enabled recovery request"
    );
    Ok(Some(outcome))
}

/// Return the beginning of the current request segment for recovery
/// compaction. Transient system notes can be appended after the user's
/// message, so using the loop's baseline directly could summarize the request
/// that the retry still needs verbatim.
pub(super) fn current_turn_preserve_index(history: &[uni::Message], turn_history_start_len: usize) -> usize {
    let turn_start = turn_history_start_len.min(history.len());
    last_user_message_index(&history[..turn_start]).unwrap_or(turn_start)
}
