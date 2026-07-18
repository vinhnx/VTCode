const PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN: usize = 48;

pub(super) fn effective_max_tool_calls_for_turn(configured_limit: usize, planning_active: bool) -> usize {
    if configured_limit == 0 {
        0
    } else if planning_active {
        configured_limit.max(PLANNING_WORKFLOW_MIN_TOOL_CALLS_PER_TURN)
    } else {
        configured_limit
    }
}
