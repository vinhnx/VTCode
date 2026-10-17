//! One navigation synthesis/execution allowance per genuine user request.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationRecovery {
    #[default]
    Ready,
    Synthesizing,
    Consumed,
    Blocked,
}

impl NavigationRecovery {
    pub(crate) fn arm(&mut self) {
        *self = match self {
            Self::Ready | Self::Synthesizing => Self::Synthesizing,
            Self::Consumed | Self::Blocked => Self::Blocked,
        };
    }

    pub(crate) fn consume(&mut self) -> bool {
        if *self != Self::Synthesizing {
            return false;
        }
        *self = Self::Consumed;
        true
    }

    pub(crate) fn blocked(self) -> bool {
        self == Self::Blocked
    }
}

pub(crate) const NAVIGATION_RECOVERY_BLOCK_REASON: &str = "Navigation stalled again after one automatic execution attempt. Evidence and pending work are retained. Type continue to resume.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::state::SessionStats;

    #[test]
    fn navigation_allowance_survives_internal_followups_and_resets_on_user_input() {
        let mut stats = SessionStats::default();
        stats.navigation_recovery.arm();
        assert!(stats.navigation_recovery.consume());
        assert!(!stats.navigation_recovery.consume());
        stats.register_follow_up_prompt("Preflight corrective turn: corrected call");
        assert_eq!(stats.navigation_recovery, NavigationRecovery::Consumed);
        stats.navigation_recovery.arm();
        assert!(stats.navigation_recovery.blocked());
        stats.register_follow_up_prompt("Preflight corrective turn: try again");
        assert!(stats.navigation_recovery.blocked());
        stats.register_follow_up_prompt("continue");
        assert_eq!(stats.navigation_recovery, NavigationRecovery::Ready);
    }
}
