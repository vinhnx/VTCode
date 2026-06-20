DONE: add trust gate to interactive auto (hard block on revert failure)
DONE: fix planning mode tool filtering (fail-closed for unknown tools)
DONE: mode switching order is Build → Duck → Plan → Auto (verified)

Remaining:
- fix lifecycle tool policy hole (primary-agent hooks skip SessionStart/SessionEnd/SubagentStart/SubagentStop)
- unify plan concept (PlanningWorkflowState vs AgentType::Plan)
- unify prompt placement (scattered PLANNING_WORKFLOW_* constants)
- centralize advertisement snapshot and add action-level masking for multi-action tools

===

Perform a detailed audit of the vtcode modes and subagents, focusing specifically on the 'plan', 'build', and 'auto' modes. Identify any existing blockers, technical issues, or workflow inefficiencies. Propose an optimized system architecture and outline a validation process to ensure that each mode functions correctly and achieves its intended objective.

===
