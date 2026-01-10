use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::ui::tui::EditingMode;

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: std::collections::BTreeSet<String>,
    /// Current editing mode: Edit, Plan, or Agent
    pub editing_mode: EditingMode,
}

impl SessionStats {
    pub(crate) fn record_tool(&mut self, name: &str) {
        self.tools.insert(name.to_string());
    }

    pub(crate) fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }

    /// Check if currently in Plan mode (read-only)
    pub(crate) fn is_plan_mode(&self) -> bool {
        matches!(self.editing_mode, EditingMode::Plan)
    }

    /// Check if currently in Agent mode (autonomous)
    pub(crate) fn is_agent_mode(&self) -> bool {
        matches!(self.editing_mode, EditingMode::Agent)
    }

    /// Set plan mode (for backward compatibility)
    pub(crate) fn set_plan_mode(&mut self, enabled: bool) {
        self.editing_mode = if enabled {
            EditingMode::Plan
        } else {
            EditingMode::Edit
        };
    }

    /// Get the current editing mode
    pub(crate) fn editing_mode(&self) -> EditingMode {
        self.editing_mode
    }

    /// Set the editing mode directly
    pub(crate) fn set_editing_mode(&mut self, mode: EditingMode) {
        self.editing_mode = mode;
    }

    /// Cycle to the next mode: Edit → Plan → Agent → Edit
    pub(crate) fn cycle_mode(&mut self) -> EditingMode {
        self.editing_mode = self.editing_mode.next();
        self.editing_mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtrlCSignal {
    Cancel,
    Exit,
}

#[derive(Default)]
pub(crate) struct CtrlCState {
    cancel_requested: AtomicBool,
    exit_requested: AtomicBool,
    exit_armed: AtomicBool,
    last_signal_time: AtomicU64,
}

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_secs(2);

impl CtrlCState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);

        // Debounce: ignore signals within 200ms of each other
        if last > 0 && now.saturating_sub(last) < 200 {
            if self.exit_requested.load(Ordering::SeqCst) {
                return CtrlCSignal::Exit;
            }
            if self.cancel_requested.load(Ordering::SeqCst) {
                return CtrlCSignal::Cancel;
            }
        }

        let window_ms = DOUBLE_CTRL_C_WINDOW.as_millis() as u64;
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_ms;

        if (self.cancel_requested.load(Ordering::SeqCst) || self.exit_armed.load(Ordering::SeqCst))
            && is_within_window
        {
            self.exit_requested.store(true, Ordering::SeqCst);
            CtrlCSignal::Exit
        } else {
            self.cancel_requested.store(true, Ordering::SeqCst);
            self.exit_armed.store(true, Ordering::SeqCst);
            CtrlCSignal::Cancel
        }
    }

    pub(crate) fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.exit_requested.store(false, Ordering::SeqCst);
        self.exit_armed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::Relaxed)
    }

    pub(crate) fn disarm_exit(&self) {
        self.exit_armed.store(false, Ordering::SeqCst);
        self.last_signal_time.store(0, Ordering::SeqCst);
    }
}
