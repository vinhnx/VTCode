//! Context compaction triggered by a mid-session switch of the main model or
//! provider.
//!
//! This module is deliberately isolated from the model-selection plumbing
//! (`model_selection.rs`) and from the inline/TUI event machinery. The "should
//! we compact?" decision and the compaction execution live here behind a small,
//! explicit interface so the logic can be unit-tested without constructing a
//! renderer, an inline loop, or a full session context.

use std::path::Path;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{LLMProvider, Message};

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::turn::compaction::{
    CompactionContext, CompactionOutcome, CompactionState, compact_history_on_model_switch_in_place,
};
use vtcode_core::core::agent::request_envelope::SegmentBoundaryReason;

/// Mutable handles required to compact the conversation when the main model or
/// provider is switched mid-session. All production call sites must populate
/// this; `finalize_model_selection` only reads/writes through it when a real
/// model/provider switch is detected.
pub(crate) struct ModelSwitchCompactionTargets<'a> {
    pub history: &'a mut Vec<Message>,
    pub session_stats: &'a mut SessionStats,
    pub context_manager: &'a mut ContextManager,
    pub session_id: &'a str,
    pub thread_id: &'a str,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
}

/// Outcome of a model-switch compaction attempt. Returned to the caller for
/// rendering so this module stays free of `AnsiRenderer` and is unit-testable.
#[derive(Debug)]
pub(crate) enum ModelSwitchCompactionOutcome {
    /// Same model/provider reselected: nothing to do, history preserved.
    Unchanged,
    /// The feature is opted out via config: compaction is skipped entirely.
    Disabled,
    /// The selection changed but no usable provider client was installed (for
    /// example an unconfigured custom provider), so nothing can be summarized.
    SkippedNoClient,
    /// Model/provider changed but history was empty; only the previous-response
    /// lineage was cleared.
    LineageCleared,
    /// Compaction produced a shorter history.
    Compacted(CompactionOutcome),
    /// A switch occurred but the history was already compact (no change made).
    AlreadyCompact,
    /// Compaction failed; the switch still applies and history is kept intact.
    Failed(anyhow::Error),
}

/// Everything needed to decide and perform compaction after a model switch.
pub(crate) struct ModelSwitchCompactionRequest<'a> {
    pub prev_provider: String,
    pub prev_model: String,
    pub new_provider: String,
    pub new_model: String,
    /// Whether a real provider client for the new selection was installed. When
    /// false the stale client cannot safely summarize, so compaction is skipped.
    pub client_installed: bool,
    /// Whether the feature is enabled (config `agent.harness.compact_on_model_switch`).
    pub enabled: bool,
    pub provider: &'a dyn LLMProvider,
    pub workspace: &'a Path,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub targets: ModelSwitchCompactionTargets<'a>,
}

/// Pure decision helper: did the user actually change the model or provider?
///
/// Provider comparison is case-insensitive so a cosmetic case difference (e.g.
/// `OpenAI` vs `openai`) is not treated as a switch.
pub(crate) fn is_real_model_switch(prev_provider: &str, prev_model: &str, new_provider: &str, new_model: &str) -> bool {
    !prev_provider.eq_ignore_ascii_case(new_provider) || prev_model != new_model
}

/// Marker prefix for the autonomous mid-turn resume directive injected after a
/// real model/provider switch. Used for idempotency: a repeated switch must not
/// stack duplicate resume notes.
const MODEL_SWITCH_RESUME_PREFIX: &str = "Model switched mid-turn:";

/// Build the autonomous resume directive that lets the new model continue
/// seamlessly from the previous model's work without data or context loss.
///
/// The message carries the `prev -> new` route, the preserved verification gate
/// and stall reason, and a bounded touched-file hint so the successor reuses
/// history instead of re-exploring. Callers push it after compaction so it is
/// never summarized away in the same pass.
/// Collapse attacker-controllable text (stall reasons, filenames) into a single
/// line so a workspace filename cannot inject newlines or control chars into the
/// privileged `System` resume directive. Mirrors `normalize_whitespace` handling
/// in the memory envelope.
fn sanitize_resume_fragment(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn build_mid_turn_resume_message(
    prev_provider: &str,
    prev_model: &str,
    new_provider: &str,
    new_model: &str,
    stall_reason: Option<&str>,
    verification_snapshot: (bool, u8),
    touched_files: &[String],
    compaction_applied: bool,
) -> String {
    // Report the handoff honestly: the AlreadyCompact and Failed paths reuse
    // this note without summarizing, so they must not claim a compaction
    // happened (previously every switch asserted "was auto-compacted").
    let continuity = if compaction_applied {
        "Conversation was auto-compacted to preserve context; summary plus recent tail retained."
    } else {
        "Conversation history was left intact; full context retained."
    };
    let mut message = format!(
        "{MODEL_SWITCH_RESUME_PREFIX} {prev_provider}/{prev_model} -> {new_provider}/{new_model}. \
        {continuity} \
        The task and tool outputs in history still apply: continue from where the previous model left off \
        and reuse those outputs instead of re-reading files."
    );
    if let Some(reason) = stall_reason.map(str::trim).filter(|reason| !reason.is_empty()) {
        let sanitized = sanitize_resume_fragment(reason, 220);
        if !sanitized.is_empty() {
            message.push_str(&format!(" Previous turn stalled: {sanitized}."));
        }
    }
    let (verification_pending, fix_remaining) = verification_snapshot;
    if verification_pending {
        message.push_str(&format!(
            " Verification gate pending: edits since the last passing verifier are unverified, and a verifier must exit 0 \
            before the task can complete; {fix_remaining} fix-up edits allowed."
        ));
    }
    let touched: Vec<String> = touched_files
        .iter()
        .map(|file| sanitize_resume_fragment(file, 80))
        .filter(|file| !file.is_empty())
        .take(5)
        .collect();
    if !touched.is_empty() {
        message.push_str(&format!(" Recent files: {}.", touched.join(", ")));
    }
    message
}

/// Returns `true` when `message` is a prior model-switch resume note.
fn is_model_switch_resume(message: &Message) -> bool {
    message.role == vtcode_core::llm::provider::MessageRole::System
        && message.content.as_text().as_ref().starts_with(MODEL_SWITCH_RESUME_PREFIX)
}

/// Remove stale resume notes so a new switch never summarizes or retains an
/// obsolete route. Mirrors `strip_existing_memory_envelope`: transient handoff
/// notes are rebuilt fresh from current `SessionStats` after compaction.
fn strip_prior_mid_turn_resumes(history: &mut Vec<Message>) {
    history.retain(|message| !is_model_switch_resume(message));
}

/// Append the autonomous resume directive after a real model switch.
///
/// Returns `true` when a resume note was injected. No-op when `history` is empty.
/// When history already ends with a resume note, it is replaced (not stacked) so
/// sequential switches stay bounded at one trailing note and refreshed gates
/// (stall/verification/touched) are reflected. Exact duplicates are skipped to
/// avoid churn.
fn inject_mid_turn_resume(
    history: &mut Vec<Message>,
    prev_provider: &str,
    prev_model: &str,
    new_provider: &str,
    new_model: &str,
    session_stats: &SessionStats,
    compaction_applied: bool,
) -> bool {
    if history.is_empty() {
        return false;
    }
    let resume = build_mid_turn_resume_message(
        prev_provider,
        prev_model,
        new_provider,
        new_model,
        session_stats.turn_stall_reason(),
        session_stats.verification_snapshot(),
        &session_stats.recent_touched_files(),
        compaction_applied,
    );
    if let Some(last) = history.last()
        && is_model_switch_resume(last)
    {
        if last.content.as_text().as_ref() == resume {
            return false;
        }
        history.pop();
    }
    history.push(Message::system(resume));
    true
}

/// Start a fresh request segment for the new model route, reset pending
/// compaction pressure, and append the autonomous resume note without claiming
/// a compaction happened. Shared by the already-compact and failed-compaction
/// tails so the two non-compacting outcomes cannot drift (both must segment
/// for prompt-cache safety and both must report history left intact).
fn start_model_segment_and_resume(
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
    prev_provider: &str,
    prev_model: &str,
    new_provider: &str,
    new_model: &str,
) {
    session_stats.begin_request_segment(SegmentBoundaryReason::Model);
    context_manager.take_compaction_pending();
    context_manager.reset_token_pressure_after_compaction();
    inject_mid_turn_resume(history, prev_provider, prev_model, new_provider, new_model, session_stats, false);
}

/// Decide whether a model/provider switch requires context compaction and, if
/// so, perform it. Never fails the caller: compaction errors are surfaced via
/// [`ModelSwitchCompactionOutcome::Failed`] so the model switch itself always
/// applies.
pub(crate) async fn compact_on_model_switch(
    req: ModelSwitchCompactionRequest<'_>,
) -> Result<ModelSwitchCompactionOutcome> {
    if !req.enabled {
        return Ok(ModelSwitchCompactionOutcome::Disabled);
    }

    if !is_real_model_switch(&req.prev_provider, &req.prev_model, &req.new_provider, &req.new_model) {
        return Ok(ModelSwitchCompactionOutcome::Unchanged);
    }

    req.targets.session_stats.auto_compact_suppressed = vtcode_core::compaction::SUPPRESS_NONE;
    req.targets.session_stats.prefire.clear();

    // The selection changed (for example an unconfigured custom provider) but no
    // usable client was installed, so we cannot summarize with the new
    // provider. Compact against the stale client would produce a provider-
    // mismatched summary, so we leave history untouched instead.
    if !req.client_installed {
        return Ok(ModelSwitchCompactionOutcome::SkippedNoClient);
    }

    req.targets.session_stats.clear_previous_response_chain();

    if req.targets.history.is_empty() {
        return Ok(ModelSwitchCompactionOutcome::LineageCleared);
    }

    // Destructure to keep `history`/`session_stats` usable after the compaction
    // call via reborrows. The resume directive is built from the pre-compaction
    // stall/verification snapshot so a mid-turn switch preserves the exact gate
    // the new model must honor. Stale resumes are stripped first so the new
    // summary/envelope never duplicates an obsolete route note.
    let ModelSwitchCompactionRequest {
        prev_provider,
        prev_model,
        new_provider,
        new_model,
        provider,
        workspace,
        vt_cfg,
        targets,
        ..
    } = req;
    let ModelSwitchCompactionTargets {
        history,
        session_stats,
        context_manager,
        session_id,
        thread_id,
        lifecycle_hooks,
        harness_emitter,
    } = targets;
    strip_prior_mid_turn_resumes(history);
    if history.is_empty() {
        return Ok(ModelSwitchCompactionOutcome::LineageCleared);
    }

    match compact_history_on_model_switch_in_place(
        CompactionContext::new(
            provider,
            &new_model,
            session_id,
            thread_id,
            workspace,
            vt_cfg,
            lifecycle_hooks,
            harness_emitter,
        ),
        CompactionState::new(&mut *history, &mut *session_stats, &mut *context_manager),
    )
    .await
    {
        Ok(Some(outcome)) => {
            // Keep `compacted_len` as emitted in `thread.compact_boundary` for
            // telemetry/UI consistency; the trailing resume note is accounted
            // separately in the log line.
            inject_mid_turn_resume(
                history,
                &prev_provider,
                &prev_model,
                &new_provider,
                &new_model,
                session_stats,
                true,
            );
            Ok(ModelSwitchCompactionOutcome::Compacted(outcome))
        }
        Ok(None) => {
            // History was already compact but the route still changed: start a new
            // request segment and reset pressure so the new model does not reuse
            // the old model's cached prefix, then inject autonomous resume.
            start_model_segment_and_resume(
                history,
                session_stats,
                context_manager,
                &prev_provider,
                &prev_model,
                &new_provider,
                &new_model,
            );
            Ok(ModelSwitchCompactionOutcome::AlreadyCompact)
        }
        Err(err) => {
            // Compaction failed but the switch still applies. Preserve full history
            // (bounded to one trailing resume via replace), start a new segment
            // for the new route, and reset pending pressure so the next turn
            // records a fresh estimate instead of storming an immediate retry on
            // stale over-budget pressure.
            start_model_segment_and_resume(
                history,
                session_stats,
                context_manager,
                &prev_provider,
                &prev_model,
                &new_provider,
                &new_model,
            );
            Ok(ModelSwitchCompactionOutcome::Failed(err))
        }
    }
}

#[cfg(test)]
mod tests;
