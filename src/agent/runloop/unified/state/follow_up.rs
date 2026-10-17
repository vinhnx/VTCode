//! Follow-up prompt tracking and stalled-turn recovery classification.
//!
//! Extracted from state.rs beside the pre-existing preflight_recovery,
//! request_envelope, ctrl_c, verification, and prompt_cache submodules. No
//! behavior change: follow-up streak accounting, stall classification, and
//! the user-typed follow-up matcher are verbatim, and FollowUpPromptAction
//! and is_follow_up_prompt_like are re-exported from the parent.

use super::SessionStats;
use super::preflight_recovery::PreflightRecovery;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FollowUpPromptAction {
    None,
    ForceConclusion,
    RecoverFromStall { stall_reason: Option<String> },
}

impl FollowUpPromptAction {
    pub(crate) const fn should_force_autonomous_response(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) const fn is_stalled_recovery(&self) -> bool {
        matches!(self, Self::RecoverFromStall { .. })
    }

    pub(crate) fn stall_reason(&self) -> Option<&str> {
        match self {
            Self::RecoverFromStall { stall_reason } => stall_reason.as_deref(),
            Self::None | Self::ForceConclusion => None,
        }
    }
}

const FOLLOW_UP_STALLED_THRESHOLD: usize = 1;
const FOLLOW_UP_DEFAULT_THRESHOLD: usize = 3;

impl SessionStats {
    pub(crate) fn register_follow_up_prompt(&mut self, input: &str) -> FollowUpPromptAction {
        // Internal continuations belong to the existing recovery episode.
        // Treating their verbose text as fresh user input resets the budget
        // on every retry and makes the cross-turn limit unreachable.
        if crate::agent::runloop::unified::turn::is_internal_harness_follow_up(input) {
            return FollowUpPromptAction::None;
        }
        self.preflight_recovery = PreflightRecovery::default();
        self.navigation_recovery = Default::default();
        let suppression_active = self.consume_follow_up_prompt_suppression();
        let is_follow_up = is_follow_up_prompt_like(input);

        if is_follow_up {
            if suppression_active {
                return FollowUpPromptAction::None;
            }
            self.follow_up_prompt_streak = self.follow_up_prompt_streak.saturating_add(1);
        } else {
            self.follow_up_prompt_streak = 0;
            self.turn_stalled = false;
            self.turn_stall_reason = None;
            self.reset_verification_recovery_episode();
            self.reset_tracker_continuation_budget();
            self.tracker_completed_count_high_water = 0;
            return FollowUpPromptAction::None;
        }

        let threshold = if self.turn_stalled {
            FOLLOW_UP_STALLED_THRESHOLD
        } else {
            FOLLOW_UP_DEFAULT_THRESHOLD
        };
        if self.follow_up_prompt_streak < threshold {
            return FollowUpPromptAction::None;
        }

        if self.turn_stalled {
            FollowUpPromptAction::RecoverFromStall { stall_reason: self.turn_stall_reason.clone() }
        } else {
            FollowUpPromptAction::ForceConclusion
        }
    }

    pub(crate) fn mark_turn_stalled(&mut self, stalled: bool, reason: Option<String>) {
        self.turn_stalled = stalled;
        if !stalled {
            self.follow_up_prompt_streak = 0;
            self.suppress_next_follow_up_prompt = false;
            self.turn_stall_reason = None;
        } else {
            self.turn_stall_reason = reason;
        }
    }

    #[cfg(test)]
    pub(crate) fn turn_stalled(&self) -> bool {
        self.turn_stalled
    }

    pub(crate) fn turn_stall_reason(&self) -> Option<&str> {
        self.turn_stall_reason.as_deref()
    }

    pub(crate) fn suppress_next_follow_up_prompt(&mut self) {
        self.suppress_next_follow_up_prompt = true;
    }

    fn consume_follow_up_prompt_suppression(&mut self) -> bool {
        std::mem::take(&mut self.suppress_next_follow_up_prompt)
    }
}

pub(crate) fn is_follow_up_prompt_like(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
        .to_ascii_lowercase();
    if normalized.starts_with("continue autonomously from the last stalled turn") {
        return true;
    }
    let words: Vec<&str> = normalized.split_whitespace().collect();
    matches!(
        words.as_slice(),
        ["continue"]
            | ["retry"]
            | ["proceed"]
            | ["go", "on"]
            | ["go", "ahead"]
            | ["keep", "going"]
            | ["please", "continue"]
            | ["continue", "please"]
            | ["please", "retry"]
            | ["retry", "please"]
            | ["continue", "with", "recommendation"]
            | ["continue", "with", "your", "recommendation"]
    )
}
