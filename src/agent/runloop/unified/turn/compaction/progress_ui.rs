//! Async progress UI for context compaction.
//!
//! The compaction engine (`vtcode-core::compaction` + `turn::compaction`) runs
//! as a single awaited future. Without a concurrent status task the UI freezes
//! until the engine returns, then prints one post-hoc line. This module keeps
//! the two in sync: the guard starts a `PlaceholderSpinner` ("Compacting
//! context...") plus a lightweight elapsed updater, brackets the engine await,
//! and formats the terminal `Context compacted · {duration}` line from the
//! engine's authoritative outcome.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::palettes::format_duration_label;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::ui_interaction::{PlaceholderSpinner, start_loading_status};

/// Spinner message shown while the engine runs.
const COMPACTING_MESSAGE: &str = "Compacting context...";

/// How often the elapsed suffix refreshes. Matches the spinner tick granularity
/// without spamming the status channel.
const ELAPSED_TICK_MS: u64 = 200;

/// RAII guard bracketing one compaction pass.
///
/// The background task only writes status text; the engine future owns the real
/// work. Dropping (or `finish`) aborts the ticker and restores input status so
/// the final `renderer.line()` is the single source of truth.
#[must_use = "drop the guard only after the engine await so the spinner stays in sync"]
pub(crate) struct CompactionProgressGuard {
    spinner: Arc<PlaceholderSpinner>,
    start: Instant,
    updater: Option<tokio::task::JoinHandle<()>>,
}

impl CompactionProgressGuard {
    /// Show `Compacting context...` async while the caller awaits the engine.
    pub(crate) fn start(handle: &InlineHandle, input_status: &InputStatusState) -> Self {
        let spinner = Arc::new(start_loading_status(handle, input_status, COMPACTING_MESSAGE));
        let start = Instant::now();
        let spinner_clone = Arc::clone(&spinner);
        let updater = tokio::task::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(ELAPSED_TICK_MS));
            loop {
                interval.tick().await;
                let elapsed = start.elapsed();
                spinner_clone.update_message(format!("Compacting context... ({})", format_duration_label(elapsed)));
            }
        });
        Self { spinner, start, updater: Some(updater) }
    }

    /// Elapsed time since the guard started.
    pub(crate) fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Stop the ticker and restore input status. The caller then emits the
    /// authoritative final line derived from the engine outcome.
    pub(crate) fn finish(mut self) -> Duration {
        let elapsed = self.elapsed();
        if let Some(updater) = self.updater.take() {
            updater.abort();
        }
        self.spinner.finish();
        elapsed
    }
}

impl Drop for CompactionProgressGuard {
    fn drop(&mut self) {
        if let Some(updater) = self.updater.take() {
            updater.abort();
        }
        self.spinner.finish();
    }
}

/// Final line for a successful pass: `Context compacted · 1m 34s (...)`.
pub(crate) fn format_compacted_summary(
    original_len: usize,
    compacted_len: usize,
    mode: &str,
    elapsed: Duration,
) -> String {
    format!(
        "Context compacted · {} ({} -> {} context items, {} compaction). Prompt cache reuse may be lower on the next request.",
        format_duration_label(elapsed),
        original_len,
        compacted_len,
        mode
    )
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
    let single_line = safe.split_whitespace().collect::<Vec<_>>().join(" ");
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
mod tests {
    use super::*;

    #[test]
    fn compacted_summary_includes_counts_mode_and_duration() {
        let label = format_compacted_summary(86, 12, "local", Duration::from_secs(94));
        assert!(label.starts_with("Context compacted · "));
        assert!(label.contains("86 -> 12"));
        assert!(label.contains("local"));
        assert!(label.contains("1m 34s"));
    }

    #[test]
    fn already_compact_includes_duration() {
        let label = format_already_compact(Duration::from_secs(3));
        assert!(label.contains("already compact"));
        assert!(label.contains("3s"));
    }

    #[test]
    fn failed_label_includes_duration() {
        let error = anyhow::anyhow!("Unknown parameter: 'stream'. Body: {{\"error\":\"duplicate\"}}")
            .context("Failed to compact history via provider-native compaction");
        let label = format_compaction_failed(Duration::from_secs(5), &error);
        assert!(label.contains("compaction failed"));
        assert!(label.contains("5s"));
        assert!(label.contains("History preserved"));
        assert!(label.contains("Unknown parameter: 'stream'"));
        assert!(!label.contains("duplicate"));
    }

    #[test]
    fn failed_label_bounds_untrusted_unicode_diagnostics() {
        let error = anyhow::anyhow!("bad response\n{}", "故障".repeat(1000));
        let label = format_compaction_failed(Duration::from_secs(1), &error);
        assert!(!label.contains('\n'));
        assert!(label.chars().count() < 650);
        assert!(label.contains("..."));
    }

    #[test]
    fn compaction_debug_details_preserve_chain_and_redact_secrets() {
        let secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
        let error = anyhow::anyhow!("provider rejected api_key={secret}").context("Compaction request failed");
        let details = format_compaction_error_details(&error);
        assert!(details.contains("Compaction request failed"));
        assert!(details.contains("provider rejected"));
        assert!(!details.contains(secret));
        let label = format_compaction_failed(Duration::from_secs(1), &error);
        assert!(!label.contains(secret));
    }
}
