//! Internal, opt-in startup timing shared by the binary and terminal UI.
//!
//! The recorder intentionally emits only phase names and durations. It is
//! initialized before tracing so early bootstrap phases are observable without
//! changing the normal logging setup.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

type FirstRenderHook = Box<dyn FnOnce() + Send + 'static>;

struct StartupTraceState {
    enabled: bool,
    started_at: Instant,
    first_render_seen: AtomicBool,
    first_render_hook: Mutex<Option<FirstRenderHook>>,
    phases: Mutex<Vec<(String, f64)>>,
    summary_emitted: AtomicBool,
}

static STATE: OnceLock<StartupTraceState> = OnceLock::new();
static HOOK_STATE: AtomicU8 = AtomicU8::new(0);

/// Env var that opts into phase recording.
const STARTUP_TRACE_ENV: &str = "VTCODE_STARTUP_TRACE";

/// Whether startup tracing is enabled.
///
/// Parsed through the canonical env-flag set (`1`/`true`/`yes`/`on`/`debug`,
/// trimmed and case-insensitive) rather than a local literal comparison.
#[must_use]
pub fn startup_trace_enabled() -> bool {
    crate::utils::env_flag_enabled(STARTUP_TRACE_ENV)
}

fn state() -> &'static StartupTraceState {
    STATE.get_or_init(|| StartupTraceState {
        enabled: startup_trace_enabled(),
        started_at: Instant::now(),
        first_render_seen: AtomicBool::new(false),
        first_render_hook: Mutex::new(None),
        phases: Mutex::new(Vec::new()),
        summary_emitted: AtomicBool::new(false),
    })
}

fn push_phase_entry(name: &str, duration_ms: f64) {
    let state = state();
    if !state.enabled {
        return;
    }
    let mut phases = state.phases.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Keep the machine-parseable summary unambiguous: `phases=<name=ms,...>`.
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch == ',' || ch == '=' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect();
    phases.push((sanitized, duration_ms));
}

/// Initialize startup timing before tracing is configured.
pub fn initialize() {
    let _ = state();
}

/// Start a phase timer when startup tracing is enabled.
#[must_use]
pub fn phase_started() -> Option<Instant> {
    state().enabled.then(Instant::now)
}

/// Record a completed phase as a duration in milliseconds.
pub fn record_phase(phase: &str, started_at: Option<Instant>) {
    let Some(started_at) = started_at else {
        return;
    };
    let elapsed_ms = started_at.elapsed().as_secs_f64() * 1_000.0;
    push_phase_entry(phase, elapsed_ms);
    if state().enabled {
        eprintln!("VTCODE_STARTUP_TRACE phase={phase} duration_ms={elapsed_ms:.3}");
    }
}

/// Record a process-level startup milestone without requiring a phase timer.
/// This is useful for short-lived commands whose output is the completion
/// boundary rather than an interactive frame.
pub fn record_milestone(name: &str) {
    let state = state();
    if state.enabled {
        let elapsed_ms = state.started_at.elapsed().as_secs_f64() * 1_000.0;
        push_phase_entry(name, elapsed_ms);
        eprintln!("VTCODE_STARTUP_TRACE phase={name} elapsed_ms={elapsed_ms:.3}");
    }
}

/// Record a phase using an elapsed duration supplied by a caller that already
/// owns a timer. This keeps startup tracing usable across crate boundaries.
pub fn record_duration(name: &str, duration: std::time::Duration) {
    if state().enabled {
        let duration_ms = duration.as_secs_f64() * 1_000.0;
        push_phase_entry(name, duration_ms);
        eprintln!("VTCODE_STARTUP_TRACE phase={name} duration_ms={duration_ms:.3}");
    }
}

/// Emit one ordered, machine-parseable end-of-startup summary.
///
/// Format: `VTCODE_STARTUP_TRACE phase=summary total_ms=<n> phases=<name=ms,...>`.
/// Silent unless `VTCODE_STARTUP_TRACE=1`. Emits at most once per process so
/// both the one-shot boundary and the first-render path can call it safely.
pub fn emit_summary() {
    let state = state();
    if !state.enabled {
        return;
    }
    if state.summary_emitted.swap(true, Ordering::AcqRel) {
        return;
    }
    let total_ms = state.started_at.elapsed().as_secs_f64() * 1_000.0;
    let phases = state.phases.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let rendered = phases
        .iter()
        .map(|(name, duration_ms)| format!("{name}={duration_ms:.3}"))
        .collect::<Vec<_>>()
        .join(",");
    eprintln!("VTCODE_STARTUP_TRACE phase=summary total_ms={total_ms:.3} phases={rendered}");
}

/// Install work that should run after the first interactive frame is drawn.
///
/// The hook is also installed when tracing is disabled because the startup
/// prompt-size warning uses the same first-render boundary. Hooks are
/// one-shot and are safe to register after the frame in tests or embedded
/// integrations.
pub fn install_first_render_hook<F>(hook: F)
where
    F: FnOnce() + Send + 'static,
{
    let state = state();
    if state.first_render_seen.load(Ordering::Acquire) {
        hook();
        return;
    }

    let mut pending = state.first_render_hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.first_render_seen.load(Ordering::Acquire) {
        drop(pending);
        hook();
    } else {
        *pending = Some(Box::new(hook));
        HOOK_STATE.store(1, Ordering::Release);
    }
}

/// Record and publish the first interactive frame boundary.
pub fn record_first_render() {
    let state = state();
    if !state.enabled && HOOK_STATE.load(Ordering::Acquire) == 0 {
        if !state.first_render_seen.load(Ordering::Acquire) {
            state.first_render_seen.store(true, Ordering::Release);
        }
        return;
    }
    if state.first_render_seen.swap(true, Ordering::AcqRel) {
        return;
    }

    if state.enabled {
        let first_render_elapsed_ms = state.started_at.elapsed().as_secs_f64() * 1_000.0;
        push_phase_entry("first_ui_render", first_render_elapsed_ms);
        eprintln!("VTCODE_STARTUP_TRACE phase=first_ui_render duration_ms={first_render_elapsed_ms:.3}");
        emit_summary();
    }

    if HOOK_STATE.swap(0, Ordering::AcqRel) == 1 {
        let hook = state
            .first_render_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(hook) = hook {
            hook();
        }
    }
}
