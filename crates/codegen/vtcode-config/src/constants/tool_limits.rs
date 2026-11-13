/// Default maximum number of tool-loop iterations in one agent turn.
pub const DEFAULT_MAX_TOOL_LOOPS: usize = 60;
/// Default maximum number of turns in full-auto execution.
pub const DEFAULT_FULL_AUTO_MAX_TURNS: usize = 100;
/// Default conversation-turn retention limit. This is a context-retention
/// limit, not a tool-execution budget.
pub const DEFAULT_MAX_CONVERSATION_TURNS: usize = 150;

/// Hard cap for ordinary tool-loop extensions.
pub const MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 120;
/// Multiplier used to derive the ordinary extension hard cap.
pub const MAX_TOOL_LOOP_CAP_MULTIPLIER: usize = 3;
/// Maximum ordinary tool-loop extension accepted from one prompt.
pub const MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 50;
/// One-time internal allowance applied when an approved plan enters execution.
/// This is not a user-facing configuration field and remains bounded by the
/// ordinary tool-loop hard cap.
pub const APPROVED_PLAN_TOOL_LOOP_INCREMENT: usize = 50;
/// Minimum tool-loop budget while planning research is active.
pub const PLANNING_WORKFLOW_MIN_TOOL_LOOPS: usize = 60;
/// Hard cap for planning tool-loop extensions.
pub const PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP: usize = 240;
/// Multiplier used to derive the planning extension hard cap.
pub const PLANNING_WORKFLOW_TOOL_LOOP_CAP_MULTIPLIER: usize = 6;
/// Maximum planning tool-loop extension accepted from one prompt.
pub const PLANNING_WORKFLOW_MAX_TOOL_LOOP_INCREMENT_PER_PROMPT: usize = 80;

/// Absolute ceiling for per-turn tool-loop extensions derived from a
/// configured base limit. Values at or above the absolute cap are returned
/// unchanged so an already-generous configuration is never shrunk.
pub const fn tool_loop_hard_cap(base_limit: usize, planning_active: bool) -> usize {
    if planning_active {
        if base_limit >= PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
            return base_limit;
        }
        let scaled = base_limit.saturating_mul(PLANNING_WORKFLOW_TOOL_LOOP_CAP_MULTIPLIER);
        if scaled > PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
            return PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP;
        }
        return scaled;
    }
    if base_limit >= MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
        return base_limit;
    }
    let scaled = base_limit.saturating_mul(MAX_TOOL_LOOP_CAP_MULTIPLIER);
    if scaled > MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP {
        return MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP;
    }
    scaled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_execution_loop_budgets_are_moderate_and_bounded() {
        assert_eq!(DEFAULT_MAX_TOOL_LOOPS, 60);
        assert_eq!(DEFAULT_FULL_AUTO_MAX_TURNS, 100);
        assert_eq!(DEFAULT_MAX_CONVERSATION_TURNS, 150);
    }

    #[test]
    fn planning_and_approved_plan_budgets_are_explicitly_separate() {
        assert_eq!(PLANNING_WORKFLOW_MIN_TOOL_LOOPS, 60);
        assert_eq!(MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP, 120);
        assert_eq!(PLANNING_WORKFLOW_MAX_TOOL_LOOP_LIMIT_ABSOLUTE_CAP, 240);
        assert_eq!(APPROVED_PLAN_TOOL_LOOP_INCREMENT, 50);
    }

    #[test]
    fn tool_loop_hard_cap_scales_and_bounds() {
        assert_eq!(tool_loop_hard_cap(20, false), 60);
        assert_eq!(tool_loop_hard_cap(40, false), 120);
        assert_eq!(tool_loop_hard_cap(60, false), 120);
        assert_eq!(tool_loop_hard_cap(120, false), 120);
        assert_eq!(tool_loop_hard_cap(200, false), 200);
        assert_eq!(tool_loop_hard_cap(0, false), 0);
        assert_eq!(tool_loop_hard_cap(40, true), 240);
        assert_eq!(tool_loop_hard_cap(120, true), 240);
        assert_eq!(tool_loop_hard_cap(300, true), 300);
    }
}
