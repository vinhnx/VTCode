//! Renderer-independent aggregation of live loading signals.

/// Loading is suppressed when a turn is blocked, regardless of other signals.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadingActivity {
    pub blocked: bool,
    pub turn: bool,
    pub thinking: bool,
    pub progress: bool,
    pub background: bool,
}

impl LoadingActivity {
    #[must_use]
    pub fn is_active(self) -> bool {
        !self.blocked && (self.turn || self.thinking || self.progress || self.background)
    }
}

#[cfg(test)]
mod tests {
    use super::LoadingActivity;

    #[test]
    fn each_signal_activates_loading_and_blocked_suppresses_it() {
        let idle = LoadingActivity::default();
        assert!(!idle.is_active());
        for signal in [
            LoadingActivity { turn: true, ..idle },
            LoadingActivity { thinking: true, ..idle },
            LoadingActivity { progress: true, ..idle },
            LoadingActivity { background: true, ..idle },
        ] {
            assert!(signal.is_active());
            assert!(!LoadingActivity { blocked: true, ..signal }.is_active());
        }
    }
}
