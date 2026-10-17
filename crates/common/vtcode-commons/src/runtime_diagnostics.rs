//! Stable Tokio runtime diagnostics (no `tokio_unstable` required).
//!
//! Implements step 1 of the fast-Tokio principles: measure first. We capture
//! the stable subset of [`tokio::runtime::RuntimeMetrics`] — worker count,
//! alive tasks, global (injection) queue depth, and total worker busy time.
//! The article's headline signal is the global queue staying deep ("in a
//! healthy application it should generally stay close to empty"); that metric
//! is stable, so it ships here.
//!
//! Per-worker local-queue depth, steal/overflow counts, blocking-pool depth,
//! and the poll-time/schedule-latency histograms are all gated behind
//! `RUSTFLAGS="--cfg tokio_unstable"` in current Tokio. Enabling that cfg
//! changes the whole dependency graph build, so it stays opt-in follow-up work
//! rather than a default here.

use std::time::Duration;
use tokio::runtime::Handle;

/// Env var enabling periodic runtime snapshot logs.
const RUNTIME_METRICS_ENV: &str = "VTCODE_RUNTIME_METRICS";

/// Snapshot of stable runtime counters at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeSnapshot {
    /// Worker threads configured on the runtime.
    pub workers_len: usize,
    /// Currently alive tasks.
    pub alive_tasks_len: usize,
    /// Tasks pending in the global (injection) queue.
    pub global_queue_depth_len: usize,
    /// Sum of per-worker busy time since runtime creation.
    pub total_busy: Duration,
}

/// Returns true when runtime diagnostics logging is enabled.
///
/// Enabled by `VTCODE_RUNTIME_METRICS=1|true|yes|on|debug` or when
/// `VTCODE_STARTUP_TRACE=1` (startup trace already implies diagnostics).
#[must_use]
pub fn runtime_diagnostics_enabled() -> bool {
    crate::utils::env_flag_enabled(RUNTIME_METRICS_ENV) || startup_trace_enabled()
}

fn startup_trace_enabled() -> bool {
    std::env::var("VTCODE_STARTUP_TRACE").is_ok_and(|value| value == "1")
}

/// Env var that overrides the Tokio worker-thread count.
const RUNTIME_WORKERS_ENV: &str = "VTCODE_RUNTIME_WORKERS";

/// Resolve an explicit worker-thread count for the main runtime.
///
/// Returns `None` when unset or not a positive integer, so the default
/// one-worker-per-core behaviour is preserved. Reserving cores for non-Tokio
/// background work is the isolation lever the fast-Tokio guidance recommends
/// ("you rarely need every core for Tokio"); operators that co-locate VT Code
/// with other processes can lower this without rebuilding.
#[must_use]
pub fn configured_worker_threads() -> Option<usize> {
    std::env::var(RUNTIME_WORKERS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|workers| *workers > 0)
}

/// Capture stable counters from a runtime handle.
#[must_use]
pub fn snapshot(handle: &Handle) -> RuntimeSnapshot {
    let metrics = handle.metrics();
    RuntimeSnapshot {
        workers_len: metrics.num_workers(),
        alive_tasks_len: metrics.num_alive_tasks(),
        global_queue_depth_len: metrics.global_queue_depth(),
        total_busy: total_busy(&metrics),
    }
}

#[cfg(target_has_atomic = "64")]
fn total_busy(metrics: &tokio::runtime::RuntimeMetrics) -> Duration {
    let mut busy = Duration::ZERO;
    for worker in 0..metrics.num_workers() {
        busy = busy.saturating_add(metrics.worker_total_busy_duration(worker));
    }
    busy
}

#[cfg(not(target_has_atomic = "64"))]
fn total_busy(_metrics: &tokio::runtime::RuntimeMetrics) -> Duration {
    // Worker busy duration requires 64-bit atomics; report zero otherwise.
    Duration::ZERO
}

/// Log one snapshot at `DEBUG` level; no-op unless diagnostics are enabled.
///
/// Callers should gate periodic reporting on [`runtime_diagnostics_enabled`]
/// to keep the hot path free when observability is off.
pub fn log_snapshot(handle: &Handle, context: &str) {
    if !runtime_diagnostics_enabled() {
        return;
    }
    let snapshot = snapshot(handle);
    let worker_busy_ms = u64::try_from(snapshot.total_busy.as_millis()).unwrap_or(u64::MAX);
    tracing::debug!(
        target = "vtcode.runtime",
        context,
        workers = snapshot.workers_len,
        alive_tasks = snapshot.alive_tasks_len,
        global_queue_depth = snapshot.global_queue_depth_len,
        worker_busy_ms,
        "tokio runtime snapshot"
    );
}

/// Default interval between periodic runtime snapshots.
const REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn a low-frequency reporter for long-lived processes.
///
/// The reporter ticks every 60s and logs via [`log_snapshot`]. Only spawns when
/// [`runtime_diagnostics_enabled`] is true, otherwise returns `None`. Dropping
/// the returned handle detaches the task; it is cancelled when the runtime
/// drops.
pub fn spawn_periodic_reporter(handle: &Handle) -> Option<tokio::task::JoinHandle<()>> {
    if !runtime_diagnostics_enabled() {
        return None;
    }
    Some(spawn_reporter(handle, REPORT_INTERVAL))
}

/// Spawn the reporter on the supplied runtime handle.
///
/// Uses [`Handle::spawn`] rather than [`tokio::spawn`]: bootstrap starts the
/// reporter before `Runtime::block_on` establishes an ambient runtime context,
/// and `tokio::spawn` panics when no runtime context is active (the same reason
/// `agent::probe` uses `handle.spawn_blocking`).
fn spawn_reporter(handle: &Handle, interval: Duration) -> tokio::task::JoinHandle<()> {
    let metrics_handle = handle.clone();
    handle.spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip bursts of missed ticks rather than replaying them.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // `interval` yields immediately on its first tick; consume it so the
        // first periodic snapshot lands one interval after startup instead of
        // duplicating the caller's boot snapshot.
        let _ = ticker.tick().await;
        loop {
            let _ = ticker.tick().await;
            log_snapshot(&metrics_handle, "periodic");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_env_flag_is_disabled() {
        // `VTCODE_RUNTIME_METRICS_TEST_ABSENT_XYZ` is never set, so the flag
        // parser must report disabled without mutating process env.
        assert!(!crate::utils::env_flag_enabled("VTCODE_RUNTIME_METRICS_TEST_ABSENT_XYZ"));
    }

    #[test]
    fn absent_worker_override_uses_default() {
        // No override is configured in the isolated test env, so the default
        // one-worker-per-core behaviour must be preserved.
        if std::env::var("VTCODE_RUNTIME_WORKERS").is_err() {
            assert_eq!(configured_worker_threads(), None);
        }
    }

    #[tokio::test]
    async fn snapshot_reflects_current_runtime() {
        let snapshot = snapshot(&Handle::current());
        // At least one worker exists on either runtime flavor.
        assert!(snapshot.workers_len >= 1);
    }

    #[tokio::test]
    async fn log_snapshot_is_quiet_when_disabled() {
        // Must not panic and must stay a no-op when diagnostics are off.
        log_snapshot(&Handle::current(), "test");
    }

    #[test]
    fn reporter_spawns_outside_ambient_runtime_context() {
        // Bootstrap starts the reporter before `Runtime::block_on`, so spawning
        // must go through the supplied handle: `tokio::spawn` panics with
        // "there is no reactor running" when no runtime context is active.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("current-thread runtime");
        let reporter = spawn_reporter(runtime.handle(), Duration::from_secs(3600));
        reporter.abort();
    }
}
