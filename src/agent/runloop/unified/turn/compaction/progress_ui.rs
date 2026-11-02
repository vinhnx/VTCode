//! Transcript progress and compact completion summaries for context compaction.

use std::time::{Duration, Instant};

use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
use vtcode_ui::tui::app::{InlineHandle, ProgressGuard};

use crate::agent::runloop::unified::palettes::format_duration_label;
use vtcode_commons::format_token_count;

/// RAII guard bracketing one compaction pass.
///
/// A scoped phase keeps the current operation ID and restores its previous
/// phase. If no operation is active, the guard owns a new operation whose drop
/// finishes the transcript row.
#[must_use = "keep the guard alive until the compaction future settles"]
pub(crate) struct CompactionProgressGuard {
    handle: InlineHandle,
    operation: vtcode_commons::ui_protocol::ProgressOperation,
    started_at: Instant,
    phase: ProgressPhase,
    previous_phase: Option<ProgressPhase>,
    owned_progress: Option<ProgressGuard>,
}

impl CompactionProgressGuard {
    /// Show the estimated input size while the caller awaits the engine.
    pub(crate) fn start(handle: &InlineHandle, estimated_input_tokens: Option<usize>) -> Self {
        let started_at = Instant::now();
        let phase = ProgressPhase::Compacting { estimated_input_tokens, started_at };
        if let Some(ProgressUpdate::Phase { operation, phase: previous_phase }) = handle.replace_progress_phase(phase) {
            return Self {
                handle: handle.clone(),
                operation,
                started_at,
                phase,
                previous_phase: Some(previous_phase),
                owned_progress: None,
            };
        }

        let owned_progress = handle.begin_progress(phase);
        let operation = owned_progress.operation();
        Self {
            handle: handle.clone(),
            operation,
            started_at,
            phase,
            previous_phase: None,
            owned_progress: Some(owned_progress),
        }
    }

    /// Elapsed time since the guard started.
    pub(crate) fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Finish the scoped phase and return its elapsed time.
    pub(crate) fn finish(self) -> Duration {
        self.elapsed()
    }
}

impl Drop for CompactionProgressGuard {
    fn drop(&mut self) {
        if self.owned_progress.is_none()
            && let Some(previous_phase) = self.previous_phase
        {
            self.handle.restore_progress_phase(self.operation, self.phase, previous_phase);
        }
    }
}

/// Final line for a successful pass with estimated token savings.
pub(crate) fn format_compacted_summary(
    estimated_tokens_before: Option<usize>,
    estimated_tokens_after: Option<usize>,
    mode: &str,
    elapsed: Duration,
) -> String {
    let token_change = match (estimated_tokens_before, estimated_tokens_after) {
        (Some(before), Some(after)) if before >= after => {
            format!("↓ {} tokens", format_token_count(before - after))
        }
        (Some(before), Some(after)) => format!("↑ {} tokens", format_token_count(after - before)),
        _ => "token savings unavailable".to_string(),
    };
    format!("Context compacted · {} · {mode} · {token_change}", format_duration_label(elapsed))
}

/// Final line when history was already compact.
pub(crate) fn format_already_compact(elapsed: Duration) -> String {
    format!("Context already compact · {}.", format_duration_label(elapsed))
}

/// Bounded, secret-redacted error chain for debug logs.
pub(crate) fn format_compaction_error_details(error: &anyhow::Error) -> String {
    vtcode_commons::sanitizer::sanitize_provider_diagnostic(format!("{error:#}").as_bytes())
}

/// Display the bounded root cause; the sanitized chain stays in the debug log.
pub(crate) fn format_compaction_failed(elapsed: Duration, error: &anyhow::Error) -> String {
    let cause = error.root_cause().to_string();
    let cause = cause.split_once(". Body:").map_or(cause.as_str(), |(summary, _)| summary);
    let safe = vtcode_commons::sanitizer::sanitize_provider_diagnostic(cause.as_bytes());
    let single_line = vtcode_commons::formatting::collapse_whitespace(&safe);
    let mut summary: String = single_line.chars().take(512).collect();
    if single_line.chars().count() > 512 {
        summary.push_str("...");
    }
    format!(
        "Context compaction failed · {}. History preserved. Reason: {summary}. Details are in the debug log.",
        format_duration_label(elapsed)
    )
}

#[cfg(test)]
mod tests;
