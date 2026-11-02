use anyhow::Result;
use std::path::Path;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::primary_agent::{ActivePrimaryAgent, ActivePrimaryAgentState};
use vtcode_core::tools::registry::ToolRegistry;

use crate::agent::runloop::unified::planning_workflow::PlanExecutionContext;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    builtin_primary_agent_specs, load_primary_agent_specs, resolve_approved_plan_execution_agent,
};

pub(super) const PLAN_APPROVED_EXECUTION_DIRECTIVE: &str = "Execution handoff is active. Any earlier message saying tools are disabled or implementation is paused belongs to the completed planning/recovery turn and no longer applies: tools are enabled and the plan is already approved, so no further confirmation is needed. Execute the approved plan step by step, beginning with the first pending step. Before the first implementation action, use task_tracker with action=list and mark the first pending task in_progress; update each task as its work and verification complete. If the approved steps are read-only review or inspection with no file mutations expected, summarize from the evidence already gathered rather than re-running large inspections, and mark the tracker complete. Verify with the plan's `verify:` commands. Tool calls go through the tool-call channel; a raw <tool_call> block written as text is not executed. Finish with a concise execution summary covering the outcome, changed files, verification performed, and remaining blockers.";
pub(super) const PLAN_APPROVED_FRESH_CONTEXT_HEADER: &str = "This is a fresh execution context. The persisted approved plan below is the source of user intent. Treat it as authoritative and implement it now.";
pub(super) const PLAN_APPROVED_EXECUTION_INPUT: &str = "Implement the approved plan now.";

/// Append the synthetic execution prompt using the same session-state path as
/// a queued follow-up. Approved-plan execution is an internal turn, but it
/// still needs a durable user message so the provider sees a complete prompt
/// and checkpoint metadata points at the actual execution request.
pub(super) fn append_approved_plan_execution_input(runtime: &mut AgentRuntime) -> (String, usize) {
    let input = PLAN_APPROVED_EXECUTION_INPUT.to_string();
    let prompt_message_index = runtime.state.messages.len();
    // Harness-owned execution handoff: user-role for provider steering, but not
    // a prompt the user typed, so reverse history must not offer it.
    runtime.state.add_synthetic_user_message(input.clone());
    (input, prompt_message_index)
}

pub(super) fn build_approved_plan_execution_prompt(
    execution_context: PlanExecutionContext,
    plan_seed: Option<&str>,
) -> String {
    let mut prompt = String::new();
    if matches!(execution_context, PlanExecutionContext::Fresh) {
        prompt.push_str(PLAN_APPROVED_FRESH_CONTEXT_HEADER);
        prompt.push_str("\n\n");
    }
    prompt.push_str(PLAN_APPROVED_EXECUTION_DIRECTIVE);
    if let Some(seed) = plan_seed.filter(|seed| !seed.trim().is_empty()) {
        prompt.push_str("\n\nApproved plan context:\n");
        prompt.push_str(seed);
    }
    prompt
}

pub(super) async fn apply_primary_agent_tool_policy_overrides(
    tool_registry: &ToolRegistry,
    active_agent: &ActivePrimaryAgent,
) {
    for (tool_name, policy) in &active_agent.tool_policy_overrides {
        if let Err(err) = tool_registry.set_tool_policy(tool_name, policy.clone()).await {
            tracing::warn!(
                tool = %tool_name,
                error = %err,
                "Failed to apply restored primary-agent tool policy"
            );
        }
    }
}

/// Shared failure report for approved-plan agent selection. Keeps the
/// `switch_path=plan_approval` log fields and the user-facing message
/// identical across every orchestration call site so recoverable selection
/// failures cannot drift into hard-abort behavior again.
pub(super) fn report_plan_approval_selection_failure(requested_agent: &str, err: &anyhow::Error) -> String {
    tracing::error!(
        target: "vtcode.planning_workflow",
        switch_path = "plan_approval",
        requested_agent = %requested_agent,
        error = %err,
        "Could not select write-capable agent after plan approval; plan remains approved and can be retried"
    );
    format!("Could not switch to an implementation agent after plan approval: {err}")
}

/// Select the write-capable agent that must own an approved-plan execution.
///
/// Approval is a hard runtime boundary. If discovery returns a stale or
/// read-only catalog, falling back to the built-in `build` spec is safer than
/// continuing with the planning agent and letting the first mutation fail
/// under the planning policy.
pub(super) async fn select_approved_plan_execution_agent(
    active_primary_agent: &mut ActivePrimaryAgentState,
    tool_registry: &ToolRegistry,
    workspace: &Path,
    requested_agent: Option<&str>,
    configured_default: Option<&str>,
) -> Result<String> {
    let mut specs = match load_primary_agent_specs(tool_registry, workspace).await {
        Ok(specs) => specs,
        Err(err) => {
            tracing::warn!(error = %err, "Primary-agent discovery failed during approved-plan handoff; using built-ins");
            builtin_primary_agent_specs()
        }
    };
    let mut execution_agent = resolve_approved_plan_execution_agent(requested_agent, configured_default, &specs);
    if execution_agent.is_none() {
        specs = builtin_primary_agent_specs();
        execution_agent = resolve_approved_plan_execution_agent(requested_agent, configured_default, &specs);
    }

    let execution_agent = execution_agent.ok_or_else(|| {
        anyhow::anyhow!(
            "No write-capable primary agent is available to execute the approved plan; the plan was not started"
        )
    })?;
    active_primary_agent
        .select_from_specs(&specs, &execution_agent)
        .map_err(|err| {
            anyhow::anyhow!("Failed to activate approved-plan execution agent '{execution_agent}': {err}")
        })?;
    Ok(execution_agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_approval_selection_failure_message_is_recoverable() {
        let err = anyhow::anyhow!("no write-capable agent");
        let msg = report_plan_approval_selection_failure("build", &err);
        assert!(msg.contains("Could not switch to an implementation agent"));
        assert!(msg.contains("no write-capable agent"));
    }

    #[test]
    fn fresh_execution_prompt_starts_with_fresh_context_header() {
        let prompt = build_approved_plan_execution_prompt(PlanExecutionContext::Fresh, Some("step: build"));

        assert!(prompt.starts_with(PLAN_APPROVED_FRESH_CONTEXT_HEADER));
    }

    #[test]
    fn approved_plan_is_appended_after_handoff_instructions() {
        let plan_seed = "step: build the feature";
        let prompt = build_approved_plan_execution_prompt(PlanExecutionContext::Fresh, Some(plan_seed));
        let directive_end = prompt
            .find(PLAN_APPROVED_EXECUTION_DIRECTIVE)
            .expect("execution directive should be present")
            + PLAN_APPROVED_EXECUTION_DIRECTIVE.len();
        let plan_start = prompt.find(plan_seed).expect("approved plan should be present");

        assert!(plan_start > directive_end);
    }

    #[test]
    fn current_context_prompt_does_not_use_fresh_context_header() {
        let prompt = build_approved_plan_execution_prompt(PlanExecutionContext::Current, None);

        assert!(prompt.starts_with(PLAN_APPROVED_EXECUTION_DIRECTIVE));
        assert!(!prompt.contains(PLAN_APPROVED_FRESH_CONTEXT_HEADER));
    }

    #[test]
    fn empty_plan_seed_does_not_create_empty_context_section() {
        let prompt = build_approved_plan_execution_prompt(PlanExecutionContext::Current, Some(" \n\t"));

        assert_eq!(prompt, PLAN_APPROVED_EXECUTION_DIRECTIVE);
        assert!(!prompt.contains("Approved plan context:"));
    }

    #[test]
    fn execution_directive_avoids_redundant_inspection_for_review_plans() {
        let prior_evidence = "Review outcome: background completion emits one terminal event; no file edits expected.";
        let prompt = build_approved_plan_execution_prompt(PlanExecutionContext::Current, Some(prior_evidence));
        assert!(prompt.contains("read-only review or inspection"));
        assert!(prompt.contains("rather than re-running large inspections"));
        assert!(prompt.contains("summarize from the evidence already gathered"));
        assert!(prompt.contains("mark the tracker complete"));
        assert!(prompt.contains(prior_evidence));
    }

    #[test]
    fn approved_plan_execution_input_is_appended_as_a_single_user_message() {
        let state = vtcode_core::core::agent::session::AgentSessionState::new("session".to_string(), 16, 4, 128_000);
        let mut runtime = AgentRuntime::new(state, None, None);

        let (input, prompt_message_index) = append_approved_plan_execution_input(&mut runtime);

        assert_eq!(input, PLAN_APPROVED_EXECUTION_INPUT);
        assert_eq!(prompt_message_index, 0);
        assert_eq!(runtime.state.messages.len(), 1);
        assert_eq!(runtime.state.messages[0].role, vtcode_core::llm::provider::MessageRole::User);
        assert_eq!(runtime.state.messages[0].get_text_content(), PLAN_APPROVED_EXECUTION_INPUT);
    }
}
