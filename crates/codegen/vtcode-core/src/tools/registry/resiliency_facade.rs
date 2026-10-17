//! Resiliency helpers for ToolRegistry.

use std::time::Duration;

use serde_json::Value;

use super::{ToolLatencyStats, ToolRegistry, ToolTimeoutCategory};

impl ToolRegistry {
    fn scale_duration(duration: Duration, num: u32, denom: u32) -> Duration {
        if denom == 0 {
            return duration;
        }
        let millis = duration.as_millis();
        let scaled = millis.saturating_mul(num as u128).saturating_div(denom as u128);
        Duration::from_millis(scaled as u64)
    }

    /// Effective outer timeout for a call, including the long-running special
    /// case that depends on the call's action.
    ///
    /// Explicit waits (`action: "wait"`) enforce their own deadline inside the
    /// command-session executor, so wrapping them in a second deadline would
    /// race the session settlement — they get no outer timeout. Long-running
    /// *runs* (an explicit long `yield_time_ms`) have no internal deadline, so
    /// they get the generous long-running ceiling: enough that the adaptively
    /// shrunk default ceiling cannot kill a healthy build, while still
    /// bounding a hung command instead of blocking the turn forever.
    pub(super) fn effective_timeout_for_call(&self, category: ToolTimeoutCategory, args: &Value) -> Option<Duration> {
        if category == ToolTimeoutCategory::LongRunningCommand {
            if crate::tools::tool_intent::command_session_action_is(args, "wait") {
                return None;
            }
            return self.timeout_policy.read().ceiling_for(category);
        }
        self.effective_timeout(category)
    }

    pub(super) fn effective_timeout(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        let base = self.timeout_policy.read().ceiling_for(category);
        let adaptive = self.resiliency.lock().adaptive_timeout_ceiling.get(&category).copied();

        match (base, adaptive) {
            (Some(b), Some(a)) if a.as_millis() > 0 => Some(std::cmp::min(b, a)),
            (Some(b), _) => Some(b),
            (None, Some(a)) if a.as_millis() > 0 => Some(a),
            _ => None,
        }
    }

    pub(super) fn decay_adaptive_timeout(&self, category: ToolTimeoutCategory) {
        let mut state = self.resiliency.lock();
        let tuning = state.adaptive_tuning;

        if let Some(adaptive) = state.adaptive_timeout_ceiling.get_mut(&category) {
            if adaptive.as_millis() == 0 {
                return;
            }
            let before = *adaptive;
            if let Some(base) = self.timeout_policy.read().ceiling_for(category) {
                if *adaptive < base {
                    #[allow(
                        clippy::cast_sign_loss,
                        reason = "Intentional compatibility, platform, or test-only suppression."
                    )]
                    let relaxed_ms = (((*adaptive).as_millis() as f64 * (1.0 / tuning.decay_ratio)).max(0.0)) as u128;
                    let relaxed = Duration::from_millis(relaxed_ms as u64);
                    *adaptive = std::cmp::min(relaxed, base);
                }
            } else {
                // If no base, relax upward modestly
                #[allow(
                    clippy::cast_sign_loss,
                    reason = "Intentional compatibility, platform, or test-only suppression."
                )]
                let relaxed = Duration::from_millis(
                    (((*adaptive).as_millis() as f64 * (1.0 / tuning.decay_ratio)).max(0.0)) as u64,
                );
                *adaptive = relaxed;
            }

            let floor = Duration::from_millis(tuning.min_floor_ms);
            if *adaptive < floor {
                *adaptive = floor;
            }

            if *adaptive != before {
                tracing::debug!(
                    category = %category.label(),
                    previous_ms = %before.as_millis(),
                    new_ms = %adaptive.as_millis(),
                    decay_ratio = %tuning.decay_ratio,
                    "Adaptive timeout relaxed after success streak"
                );
            }
        }
    }

    pub(super) fn record_tool_failure(&self, category: ToolTimeoutCategory) -> bool {
        let mut state = self.resiliency.lock();
        state.success_trackers.insert(category, 0);
        let tracker = state.failure_trackers.entry(category).or_default();
        tracker.record_failure();
        tracker.should_circuit_break()
    }

    pub(super) fn reset_tool_failure(&self, category: ToolTimeoutCategory) {
        let mut state = self.resiliency.lock();
        if let Some(tracker) = state.failure_trackers.get_mut(&category) {
            tracker.reset();
        }
        state.success_trackers.insert(category, 0);
    }

    pub(super) fn record_tool_latency(&self, category: ToolTimeoutCategory, duration: Duration) {
        let mut guard = self.resiliency.lock();
        // Reborrow once so the borrow checker sees `latency_stats` and
        // `adaptive_timeout_ceiling` as disjoint fields, not two `DerefMut` calls on the guard.
        let state = &mut *guard;
        let tuning = state.adaptive_tuning;

        let stats = state.latency_stats.entry(category).or_insert_with(|| ToolLatencyStats::new(50));
        stats.record(duration);

        if let Some(p95) = stats.percentile(0.95) {
            if let Some(ceiling) = self.timeout_policy.read().ceiling_for(category) {
                if p95 > ceiling {
                    tracing::warn!(
                        category = %category.label(),
                        p95_ms = %p95.as_millis(),
                        ceiling_ms = %ceiling.as_millis(),
                        "Observed p95 tool latency exceeds configured ceiling; consider adjusting timeouts"
                    );
                    let adjusted = std::cmp::min(
                        ceiling,
                        std::cmp::max(Duration::from_millis(tuning.min_floor_ms), Self::scale_duration(p95, 11, 10)),
                    );
                    state.adaptive_timeout_ceiling.insert(category, adjusted);
                    tracing::debug!(
                        category = %category.label(),
                        new_ceiling_ms = %adjusted.as_millis(),
                        "Adaptive timeout ceiling applied from p95 latency"
                    );
                }
            } else {
                // No ceiling configured; derive one from p95 with headroom
                let derived =
                    std::cmp::max(Duration::from_millis(tuning.min_floor_ms), Self::scale_duration(p95, 12, 10));
                state.adaptive_timeout_ceiling.insert(category, derived);
                tracing::debug!(
                    category = %category.label(),
                    new_ceiling_ms = %derived.as_millis(),
                    "Adaptive timeout ceiling derived from p95 latency without static ceiling"
                );
            }
        }
    }

    pub(super) fn should_circuit_break(&self, category: ToolTimeoutCategory) -> Option<Duration> {
        self.resiliency
            .lock()
            .failure_trackers
            .get(&category)
            .filter(|tracker| tracker.should_circuit_break())
            .map(|tracker| tracker.backoff_duration())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use serde_json::json;

    #[tokio::test]
    async fn explicit_wait_has_no_outer_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        let wait = registry.effective_timeout_for_call(
            ToolTimeoutCategory::LongRunningCommand,
            &json!({"action": "wait", "session_id": "run-1"}),
        );
        assert!(wait.is_none(), "waits self-bound, so no outer timeout must apply");
    }

    #[tokio::test]
    async fn long_running_run_is_bounded_by_the_long_ceiling() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        let run = registry.effective_timeout_for_call(
            ToolTimeoutCategory::LongRunningCommand,
            &json!({"action": "run", "command": "cargo build", "yield_time_ms": 20_000}),
        );
        assert!(run.is_some(), "a settling long run has no internal deadline and must stay bounded");
        // The long-running ceiling is far above the adaptively-shrunk default
        // that would otherwise kill a healthy build.
        assert!(run.unwrap() >= Duration::from_secs(300));
    }

    #[tokio::test]
    async fn ordinary_calls_keep_the_adaptive_ceiling() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        let ordinary = registry.effective_timeout_for_call(ToolTimeoutCategory::Default, &json!({"cmd": "echo hi"}));
        assert!(ordinary.is_some(), "ordinary calls keep their safety ceiling");
    }

    #[tokio::test]
    async fn long_run_yield_is_classified_and_bounded_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let args = json!({"cmd": "cargo build", "yield_time_ms": 30_000});

        let category = registry.timeout_category_for_args(tools::EXEC_COMMAND, &args).await;
        assert_eq!(category, ToolTimeoutCategory::LongRunningCommand);
        let timeout = registry.effective_timeout_for_call(category, &args);
        assert!(timeout.is_some(), "the long-run classification must not disable the outer timeout");
    }

    #[tokio::test]
    async fn latency_above_ceiling_pins_adaptive_ceiling_to_base_ceiling() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let category = ToolTimeoutCategory::Default;
        let base = registry.timeout_policy.read().ceiling_for(category).expect("default ceiling");

        let before = registry.resiliency.lock().adaptive_timeout_ceiling.get(&category).copied();
        registry.record_tool_latency(category, Duration::from_millis(1));
        assert_eq!(
            registry.resiliency.lock().adaptive_timeout_ceiling.get(&category).copied(),
            before,
            "a fast sample must not change the adaptive ceiling"
        );

        registry.record_tool_latency(category, base + Duration::from_secs(3600));
        let adaptive = registry.resiliency.lock().adaptive_timeout_ceiling.get(&category).copied();
        assert_eq!(adaptive, Some(base), "p95 above the ceiling is capped at the configured ceiling");
    }
}
