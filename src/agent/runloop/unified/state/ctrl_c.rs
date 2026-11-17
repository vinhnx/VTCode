//! Ctrl+C signal state machine for session runloops.
//!
//! Extracted from `state.rs` beside the pre-existing `preflight_recovery`
//! and `request_envelope` submodules. No behavior change: phase transitions,
//! debounce/window timing, and steer-delivery accounting are verbatim, and the
//! `pub(crate)` surface re-exported from `state.rs` is unchanged.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtrlCSignal {
    Cancel,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
enum CtrlCPhase {
    #[default]
    Idle = 0,
    CancelRequested = 1,
    ExitArmed = 2,
    ExitRequested = 3,
}

impl CtrlCPhase {
    fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::CancelRequested,
            2 => Self::ExitArmed,
            3 => Self::ExitRequested,
            _ => Self::Idle,
        }
    }

    fn signal(self) -> CtrlCSignal {
        match self {
            Self::ExitRequested => CtrlCSignal::Exit,
            Self::Idle | Self::CancelRequested | Self::ExitArmed => CtrlCSignal::Cancel,
        }
    }
}

/// State machine for handling Ctrl+C signals with priority guarantees.
///
/// # Priority Guarantees
///
/// This state machine ensures that Ctrl+C (SIGINT) is always the highest priority
/// and cannot be blocked by any other process. The following guarantees are enforced:
///
/// 1. **First Ctrl+C**: Immediately cancels the current operation (Cancel signal)
/// 2. **Second Ctrl+C (within 1 second)**: Escalates to exit; a 200ms debounce
///    sub-window suppresses accidental double-taps so a rapid second tap returns
///    `Cancel` (debounced) rather than `Exit` (see State Machine below)
/// 3. **Emergency exit path**: On double Ctrl+C, the program calls `std::process::exit(130)`
///    which bypasses all other operations and cleanup routines
/// 4. **No signal masking**: SIGINT is never blocked or masked anywhere in the codebase
/// 5. **Atomic operations**: All state transitions use lock-free atomic operations,
///    ensuring no mutex contention can block signal handling
///
/// # Exit Command Priority
///
/// The `/exit`, `/quit`, `exit`, and `quit` commands are processed immediately
/// and cannot be blocked by any other operation. They return
/// `InteractionOutcome::Exit { reason: SessionEndReason::Exit }` which is checked
/// at the top of every interaction loop iteration.
///
/// # State Machine
///
/// The state machine transitions through four phases:
/// - `Idle` → First Ctrl+C → `CancelRequested` (returns `CtrlCSignal::Cancel`)
/// - `CancelRequested` → Second Ctrl+C within 200ms → `CancelRequested` (returns `CtrlCSignal::Cancel`, debounced)
/// - `CancelRequested` → Second Ctrl+C between 200ms and 1s → `ExitRequested` (returns `CtrlCSignal::Exit`)
/// - `CancelRequested` → Second Ctrl+C after 1s → `CancelRequested` (returns `CtrlCSignal::Cancel`, window expired)
/// - `ExitArmed` → Second Ctrl+C within 1s → `ExitRequested` (returns `CtrlCSignal::Exit`)
/// - `ExitRequested` → Any subsequent Ctrl+C → `ExitRequested` (returns `CtrlCSignal::Exit`)
///
/// # Debounce Mechanism
///
/// A 200ms debounce window prevents rapid repeated signals from prematurely
/// escalating the state machine. This ensures that accidental double-taps
/// don't immediately exit the program. Only after 200ms has elapsed since
/// the first signal will a second signal trigger escalation to exit.
pub(crate) struct CtrlCState {
    phase: AtomicU8,
    exit_started: std::sync::OnceLock<Instant>,
    signal_clock_origin: Instant,
    last_signal_time: AtomicU64,
    /// Shared event-delivery counter: incremented by the UI event callback
    /// each time a Steer input is accepted by the live steering channel,
    /// decremented by the runloop Steer handler. Callback and handler run
    /// sequentially per event (callback first, then the channel), so an
    /// undelivered steer falls through to the durable queue instead of
    /// vanishing when the agent is temporarily unavailable (no steering
    /// sender, closed channel). A counter — not a flag — so a burst of
    /// steers delivered before the runloop drains does not double-queue the
    /// second and later messages.
    steer_delivered: AtomicUsize,
    /// Stop-signal escalation: set only by a host's forced stop (desktop
    /// force stop, forced watchdog). The runloop's cancel paths widen the
    /// exec-session teardown from foreground-only to every session —
    /// including background ones — when this is set. Cleared by both reset
    /// paths so a later cooperative stop never inherits stale force.
    force_kill: AtomicBool,
}

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_millis(1000);

impl Default for CtrlCState {
    fn default() -> Self {
        Self {
            phase: AtomicU8::default(),
            exit_started: std::sync::OnceLock::new(),
            signal_clock_origin: Instant::now(),
            last_signal_time: AtomicU64::default(),
            steer_delivered: AtomicUsize::default(),
            force_kill: AtomicBool::default(),
        }
    }
}

impl CtrlCState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn phase(&self) -> CtrlCPhase {
        CtrlCPhase::from_raw(self.phase.load(Ordering::SeqCst))
    }

    fn set_phase(&self, phase: CtrlCPhase) {
        if phase == CtrlCPhase::ExitRequested {
            self.exit_started.get_or_init(Instant::now);
        }
        // Exit is terminal for this session, including when a UI reset races
        // the callback that accepts exit.
        let _ = self.phase.try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            (current != CtrlCPhase::ExitRequested as u8).then_some(phase as u8)
        });
    }

    /// Register a Ctrl+C signal and return the appropriate signal type.
    ///
    /// # Priority Guarantee
    ///
    /// This method is called from the signal handler and is guaranteed to:
    /// 1. Always process the signal immediately (no blocking)
    /// 2. Never be blocked by any other operation
    /// 3. Return `CtrlCSignal::Exit` on double Ctrl+C, which triggers
    ///    `emergency_terminal_cleanup()` → `std::process::exit(130)`
    ///
    /// # Debounce Behavior
    ///
    /// Rapid repeated signals (within 200ms) are debounced to prevent
    /// accidental state escalation. However, if already in `ExitArmed` or
    /// `ExitRequested` phase, rapid signals immediately escalate to exit.
    ///
    /// # Window Behavior
    ///
    /// The second Ctrl+C must arrive within 1 second of the first to trigger
    /// exit. After this window, the state machine resets to `CancelRequested`
    /// on the next signal.
    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        // Zero is reserved for no previous signal, including immediately after construction.
        let now = self.signal_clock_origin.elapsed().as_millis().min(u128::from(u64::MAX - 1)) as u64 + 1;
        self.register_signal_at(now)
    }

    fn register_signal_at(&self, now: u64) -> CtrlCSignal {
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);
        let current_phase = self.phase();

        // Debounce repeated cancel signals, but allow an already-armed stop to
        // escalate immediately so a quick second press can still exit.
        if last > 0 && now.saturating_sub(last) < 200 {
            if matches!(current_phase, CtrlCPhase::ExitArmed | CtrlCPhase::ExitRequested) {
                self.set_phase(CtrlCPhase::ExitRequested);
                return CtrlCSignal::Exit;
            }
            return current_phase.signal();
        }

        let window_ms = DOUBLE_CTRL_C_WINDOW.as_millis() as u64;
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_ms;

        if matches!(current_phase, CtrlCPhase::CancelRequested | CtrlCPhase::ExitArmed) && is_within_window {
            self.set_phase(CtrlCPhase::ExitRequested);
            return CtrlCSignal::Exit;
        }

        if matches!(current_phase, CtrlCPhase::ExitRequested) {
            return CtrlCSignal::Exit;
        }

        self.set_phase(CtrlCPhase::CancelRequested);
        CtrlCSignal::Cancel
    }

    /// Request cancellation from the local TUI without arming the emergency
    /// double-signal exit path used by the OS signal handler.
    pub(crate) fn request_local_cancel(&self) {
        if !matches!(self.phase(), CtrlCPhase::ExitRequested) {
            self.set_phase(CtrlCPhase::CancelRequested);
        }
        self.last_signal_time.store(0, Ordering::SeqCst);
    }

    /// Record that the outstanding stop was forced. A TUI Ctrl+C never sets
    /// this; only the host's force path (desktop force stop, forced watchdog
    /// escalation) does.
    pub(crate) fn mark_force_kill(&self) {
        self.force_kill.store(true, Ordering::SeqCst);
    }

    /// Whether the outstanding stop was forced: exec-session teardown widens
    /// to every session instead of foreground-only.
    pub(crate) fn is_force_kill(&self) -> bool {
        self.force_kill.load(Ordering::SeqCst)
    }

    pub(crate) fn request_exit(&self) {
        self.exit_started.get_or_init(Instant::now);
        self.phase.store(CtrlCPhase::ExitRequested as u8, Ordering::SeqCst);
    }

    pub(crate) fn exit_deadline(&self) -> Option<tokio::time::Instant> {
        self.exit_started
            .get()
            .map(|started| tokio::time::Instant::from_std(*started + Duration::from_millis(1500)))
    }

    pub(crate) fn exit_elapsed_ms(&self) -> Option<u64> {
        self.exit_started
            .get()
            .map(|started| started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
    }

    pub(crate) fn reset(&self) {
        self.set_phase(CtrlCPhase::Idle);
        self.last_signal_time.store(0, Ordering::SeqCst);
        self.force_kill.store(false, Ordering::SeqCst);
    }

    /// Clear only a cancellation already consumed by the runloop. A new
    /// cancellation racing a submission must remain pending.
    pub(crate) fn reset_handled_cancel(&self) -> bool {
        let cleared = self
            .phase
            .compare_exchange(CtrlCPhase::ExitArmed as u8, CtrlCPhase::Idle as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if cleared {
            self.last_signal_time.store(0, Ordering::SeqCst);
            self.force_kill.store(false, Ordering::SeqCst);
        }
        cleared
    }

    /// Record that the UI event callback accepted a Steer input on the live
    /// steering channel. Called from the callback before the event reaches
    /// the runloop. Safe to call once per delivered steer in a burst.
    pub(crate) fn mark_steer_delivered(&self) {
        self.steer_delivered.fetch_add(1, Ordering::SeqCst);
    }

    /// Consume one steer-delivery credit. `true` means the matching Steer
    /// event was already handed to the steering channel and must not be
    /// queued again; `false` means it fell through and should be queued so
    /// the message is processed once the agent is ready.
    pub(crate) fn take_steer_delivered(&self) -> bool {
        self.steer_delivered
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |pending| Some(pending.saturating_sub(1)))
            .is_ok_and(|prev| prev > 0)
    }

    pub(crate) fn mark_cancel_handled(&self) {
        let _ = self.phase.compare_exchange(
            CtrlCPhase::CancelRequested as u8,
            CtrlCPhase::ExitArmed as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::CancelRequested)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::ExitRequested)
    }

    pub(crate) fn is_cancel_handled(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::ExitArmed)
    }

    /// Check if cancellation or exit has been requested and return an error if so
    pub(crate) fn check_cancellation(&self) -> anyhow::Result<()> {
        if self.is_exit_requested() {
            anyhow::bail!("Exit requested");
        }
        if self.is_cancel_requested() {
            anyhow::bail!("Operation cancelled");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_monotonic_window_boundaries() {
        for (second, expected) in [
            (200, CtrlCSignal::Cancel),
            (201, CtrlCSignal::Exit),
            (1001, CtrlCSignal::Exit),
            (1002, CtrlCSignal::Cancel),
        ] {
            let state = CtrlCState::new();
            assert_eq!(state.register_signal_at(1), CtrlCSignal::Cancel);
            assert_eq!(state.register_signal_at(second), expected);
        }
    }
}
