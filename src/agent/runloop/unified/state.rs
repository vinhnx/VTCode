use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: BTreeSet<String>,
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
}

impl CtrlCState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        if self.cancel_requested.swap(true, Ordering::SeqCst)
            || self.exit_armed.swap(false, Ordering::SeqCst)
        {
            self.exit_requested.store(true, Ordering::SeqCst);
            CtrlCSignal::Exit
        } else {
            CtrlCSignal::Cancel
        }
    }

    pub(crate) fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.exit_armed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }

    pub(crate) fn disarm_exit(&self) {
        self.exit_armed.store(false, Ordering::SeqCst);
    }
}
