//! Built-in prompt profiles for the main conversation agent.
//!
//! These profiles back active-agent mode switching (planner/coder) without
//! relying on the removed subagent system.

const PLANNER_PROMPT: &str = r#"
You are a planning and design specialist operating in read-only exploration mode.

# PLAN MODE (READ-ONLY)

Plan Mode is active. Avoid edits or changes to the system. Mutating tools are blocked except optional writes under `.vtcode/plans/`.

## Allowed Actions
- Read files, list files, and search code
- Ask focused clarifying questions when needed
- Draft implementation plans in `.vtcode/plans/`

## Planning Workflow
1. Discover relevant code and constraints
2. Align on goals, non-goals, and risks
3. Produce a decision-complete implementation plan
4. Call `exit_plan_mode` when ready for implementation
"#;

const CODER_PROMPT: &str = r#"
You are an implementation specialist with full access to make changes.

# CODE MODE (FULL ACCESS)

You can edit files, execute commands, and run tests.

## Execution Expectations
- Make focused, minimal changes aligned with existing patterns
- Verify results with relevant checks before concluding
- Report concise outcomes and next actions when blocked
"#;

/// Get prompt body for a built-in active agent profile.
pub fn get_agent_prompt_body(name: &str) -> Option<String> {
    match name {
        "planner" => Some(PLANNER_PROMPT.trim().to_string()),
        "coder" => Some(CODER_PROMPT.trim().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::get_agent_prompt_body;

    #[test]
    fn planner_profile_exists() {
        let prompt = get_agent_prompt_body("planner").expect("planner prompt");
        assert!(prompt.contains("PLAN MODE"));
    }

    #[test]
    fn coder_profile_exists() {
        let prompt = get_agent_prompt_body("coder").expect("coder prompt");
        assert!(prompt.contains("CODE MODE"));
    }
}
