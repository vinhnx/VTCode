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

### Enter From an Agent Suggestion

The agent can also propose entering the planning workflow on its own when it
judges edits should be planned first. In that case a HITL confirmation prompt
appears:

```text
Enter Planning workflow?

- Enter Planning workflow (Recommended) — enter planning and persist the draft under .vtcode/plans
- Continue without Planning workflow
```

- **Enter Planning workflow** — starts planning; the draft is persisted under
  `.vtcode/plans/` and mutating tools stay disabled until you approve execution.
- **Continue without Planning workflow** — the agent proceeds without planning
  (mutating tools remain enabled).

This gate prevents the agent from silently switching into plan mode; you decide
whether to plan before any edits begin.

### Intent Phrases

You can steer the workflow with short phrases instead of the review-gate UI:

- To **exit planning and present the plan** for approval, type `implement`,
  `approve`, `lgtm`, `ship it`, `yes`/`continue`/`go`/`start`, or select
  **Execute** / **Auto-accept** in the review gate. The plan is shown in an
  inline confirmation overlay (or a text prompt in non-interactive mode);
  the agent will not self-approve by editing the plan file and staying in
  plan mode.
- To **stay in planning**, type `stay in planning` (or revise the
  `<proposed_plan>` block). This overrides any exit phrase.

### Typical Workflow

1. Select the `plan` primary agent or run `/plan`.
2. Describe the goal and constraints.
3. Iterate on repository facts, risks, and open decisions.
4. Review the emitted `<proposed_plan>` block.
5. Switch to a build-oriented primary agent such as `build` or `auto` when you are ready to implement.

## Plan Output Format

Planning output should stay decision-complete but sparse — treat it like a
compact spec, not prose. Keep the whole `<proposed_plan>` under ~1500 tokens;
prefer `file:symbol` references over narrative. This bound exists because an
overly verbose plan is truncated at the model's output-token limit (cut off
mid-plan) and must then be condensed and re-emitted.

File references must be plain text or inline code, never markdown links or
editor/IDE URIs — plans are read in terminals and other non-hyperlink
surfaces:

```markdown
Correct: `src/main.rs:42` or src/main.rs:42
Incorrect: [main.rs:42](vscode-file://vscode-app/.../workbench.html)
Incorrect: [main.rs](file:///Users/you/repo/src/main.rs#L42)
```

```markdown
Repository facts checked:

- [file:symbol or behaviour confirmed from the repo]

Next open decision: [if any], otherwise: No remaining scope decisions.

<proposed_plan>

# [Task Title]

## Summary

[1-3 lines: goal, user impact, what changes / what does not]

## Steps

1. [Action] -> files/symbols -> verify: [check]
2. [Action] -> files/symbols -> verify: [check]

## Validation

- build/lint: [detected toolchain command]
- tests: [detected toolchain command]
- behaviour: [targeted check]

## Assumptions

- [assumption or default chosen]
- [out-of-scope item intentionally not changed]
  </proposed_plan>
```

Only `Next open decision` is used as the explicit reopen marker for follow-up planning.

### Research Scope

Research effort should scale with the request. For a narrow or simple ask,
a handful of targeted reads/searches (roughly 5-10) is usually enough before
drafting `<proposed_plan>` — exhaustively enumerating the whole repository
for a simple request wastes the turn's tool-call and wall-clock budget and
can exhaust it before a plan is produced. For a broad or ambiguous ask,
research proportionally more, but stop and draft as soon as the
scope/decomposition/verification decisions are closed.

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
executes the approved plan. The planning workflow is disabled and mutating tools are enabled
once the user approves the plan.

## Budget Exhaustion

If the session budget or wall-clock limit is reached while planning, the
runloop does **not** force another interview or loop (no further LLM calls are
possible). Instead it finalizes the current plan draft and presents it via the
review gate, so you can approve or revise what was produced rather than the
turn hanging.

## Plan File Persistence

The draft is the single source of truth and always lives on disk under
`.vtcode/plans/<plan>.md`, not only in chat history. Even when the final
synthesis fails and the runloop enters the tool-free recovery pass (where all
tools — including `apply_patch` and `task_tracker` — are disabled), any
`<proposed_plan>` the model emitted inline is extracted and written to the
session plan file before the turn ends. This is why the budget/recovery
exhaustion notices can promise the draft is "preserved in the session plan
file": the recovery path persists it rather than leaving `.vtcode/plans/` at its
`start_planning` template. If you ever see an empty plan dir after planning,
treat it as a regression in this persistence path, not a permissions or
directory-creation problem.

## Best Practices

1. Be specific about files, functions, constraints, and desired behaviour.
2. Ask the agent to state trade-offs before implementation begins.
3. Keep the planning agent read-oriented and switch to `build`, `auto`, or `review` for the next phase.

## See Also

- [Command reference](../user-guide/commands.md)
- [Subagents and primary agents](../user-guide/subagents.md)
- [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md)
