# Planning Workflow

The planning workflow lets you iterate with the agent on what you want to build before implementation starts. It is driven by the built-in `plan` primary agent and the `/plan` slash command.

## Overview

During planning, the agent can:

- read files and inspect project structure
- search code with grep, structural search, and other read-only tools
- analyse patterns and constraints before proposing changes
- run explicitly safe inspection or validation commands when the active permission policy allows them

During planning, mutating tools should be denied unless a project explicitly allows durable planning artefacts such as files under `.vtcode/plans/`.

`task_tracker` is available for checklist state. Planning output should use `<proposed_plan>...</proposed_plan>` when the agent is ready for user review.

## Usage

### Start With The Planning Agent

Set the default primary agent to `plan` when you want new sessions to start with the built-in planning agent:

```toml
default_primary_agent = "plan"
```

You can also press `Tab` on an empty idle composer to cycle to the `plan` primary agent.

### Use `/plan`

`/plan` starts or continues the planning workflow. It is a workflow command, not a session state selector.

While a turn is actively processing, `/plan` is dropped with a notice (mode switches are locked for the duration of a turn). The automatic in-turn planning intent detection still engages on its own; only explicit `/plan` entry while busy is deferred.

```text
/plan
```

### Typical Workflow

1. Select the `plan` primary agent or run `/plan`.
2. Describe the goal and constraints.
3. Iterate on repository facts, risks, and open decisions.
4. Review the emitted `<proposed_plan>` block.
5. Switch to a build-oriented primary agent such as `build` or `auto` when you are ready to implement.

## Plan Output Format

Planning output should stay decision-complete but sparse:

```markdown
Repository facts checked:
- [file, symbol, or behaviour confirmed from the repo]
- [existing pattern or constraint verified before planning]

Next open decision: [if any], otherwise: No remaining scope decisions.

<proposed_plan>
# [Task Title]

## Summary
[2-4 lines: goal, user impact, what will change, what will not]

## Implementation Steps
1. [Step] -> files: [paths] -> verify: [check]
2. [Step] -> files: [paths] -> verify: [check]
3. [Step] -> files: [paths] -> verify: [check]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s) based on detected toolchain]
2. Tests: [project test command(s) based on detected toolchain]
3. Targeted behaviour checks: [explicit commands/manual checks]

## Assumptions and Defaults
1. [Explicit assumption]
2. [Default chosen when user did not specify]
3. [Out-of-scope items intentionally not changed]
</proposed_plan>
```

Only `Next open decision` is used as the explicit reopen marker for follow-up planning.

## Review Gate

After a plan is ready, a confirmation popup presents a structured summary (phases/steps
checklist, or the raw plan when the structured data is absent) and a decision gate. The
default selection for a complete draft is **Auto-accept**; for a draft still missing
required content it is **Edit plan**.

Approval options:

- **Execute** — approve and execute the plan on the current primary agent with per-step
  HITL permission confirmations.
- **Auto-accept** — approve and auto-execute the plan on the current primary agent
  (skip per-step confirmations). This is the default for a complete draft.
- **Switch to build agent** — hand the plan off to the `build` primary agent and execute
  it with per-step HITL confirmations (manual edit approvals).
- **Switch to auto agent** — hand the plan off to the `auto` primary agent and
  auto-execute the plan (skip per-step confirmations).
- **Edit plan** — return to the planning workflow to revise (type `/edit` or select it).
- **Cancel** — discard the plan without executing.

Handoff options perform a true primary-agent switch: the chosen agent becomes active and
executes the approved plan. `finish_planning` is invoked either way, disabling planning
mode and enabling mutating tools.

## Best Practices

1. Be specific about files, functions, constraints, and desired behaviour.
2. Ask the agent to state trade-offs before implementation begins.
3. Keep the planning agent read-oriented and switch to `build`, `auto`, or `review` for the next phase.

## See Also

- [Command reference](../user-guide/commands.md)
- [Subagents and primary agents](../user-guide/subagents.md)
- [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md)
