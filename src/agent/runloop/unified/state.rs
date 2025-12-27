use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: std::collections::BTreeSet<String>,
}

impl SessionStats {
    pub(crate) fn record_tool(&mut self, name: &str) {
        self.tools.insert(name.to_string());
    }

    pub(crate) fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
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
            .as_secs();
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);

        let window_secs = DOUBLE_CTRL_C_WINDOW.as_secs();
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_secs;

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
