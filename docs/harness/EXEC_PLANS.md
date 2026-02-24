# Execution Plans

Execution plans are self-contained, living design documents for complex multi-step work in VT Code. They capture the goal, context, steps, decisions made during execution, and retrospective notes.

## Why Exec Plans?

- **Agent continuity**: an agent picking up mid-task can read the exec plan and resume without the original conversation context.
- **Decision logging**: captures why choices were made, not just what was done.
- **Retrospectives**: completed plans become learning artifacts for future work.

## Exec Plans vs Plan Mode

These are distinct concepts:

- **Exec Plans** are persistent design documents stored in `docs/harness/exec-plans/`. They capture goals, decisions, and retrospectives for complex work. They survive across sessions.
- **Plan Mode** is a runtime safety state in VT Code that blocks mutating tools. It uses `.vtcode/plans/` for ephemeral session plans and has an `exit_plan_mode` tool to transition to implementation.

Exec Plans are written to `docs/harness/exec-plans/active/` and are part of the repository's knowledge base.
Plan Mode session plans are written to `.vtcode/plans/` and are ephemeral per-session artifacts.

## Directory Structure

```
docs/harness/exec-plans/
├── active/           # Plans currently being worked on
│   ├── 001-mcp-oauth-coverage.md
│   └── 002-tui-event-refactor.md
└── completed/        # Finished plans (kept for reference)
    └── 000-example-completed.md
```

Plans are numbered sequentially. Use `NNN-short-description.md` format.

## Mandatory Sections

Every exec plan must contain these sections:

### 1. Goal

One sentence describing the desired end state. Must be verifiable.

### 2. Context

Why this work is needed. Link to related issues, tech debt items, or quality scores.

### 3. Steps

Ordered list of steps with status markers:

- `[ ]` — not started
- `[~]` — in progress
- `[x]` — completed
- `[!]` — blocked (include reason)

Each step should be **composable**:

- Scoped to one module/risk boundary.
- Actionable in one execution pass.
- Paired with a concrete expected outcome.
- Paired with one verification check (command, test, or observable condition).

Preferred step pattern:

`Step N: <action>. Outcome: <observable result>. Verify: <command/check>.`

### 4. Decision Log

Timestamped entries for non-obvious choices made during execution:

```
- 2026-02-13: Chose X over Y because Z.
- 2026-02-14: Discovered constraint W, adjusted step 3.
```

### 5. Retrospective

Filled in after completion. What went well, what didn't, what to do differently.

---

## Template

```markdown
# EP-NNN: Title

**Status**: active | completed | abandoned
**Author**: human | agent
**Created**: YYYY-MM-DD
**Completed**: YYYY-MM-DD (if applicable)

## Goal

[One sentence: what does "done" look like?]

## Context

[Why is this work needed? Link to TECH_DEBT_TRACKER.md items, QUALITY_SCORE.md domains, or GitHub issues.]

## Steps

- [ ] Step 1: description
- [ ] Step 2: description
- [ ] Step 3: description

## Decision Log

- YYYY-MM-DD: [decision and rationale]

## Retrospective

### What went well

-

### What didn't

-

### What to change

-
```

---

## Agent Guidelines

### Creating an Exec Plan

Create an exec plan when:

- The task requires 5+ steps across multiple files or modules.
- Architectural decisions need to be recorded.
- The work might span multiple agent sessions.

Do not create an exec plan for:

- Single-file changes.
- Bug fixes with obvious root causes.
- Documentation-only updates.

### Updating an Exec Plan

- Mark steps as completed (`[x]`) as you finish them.
- Keep exactly one step `[~]` (in progress) at a time.
- Add decision log entries for any non-trivial choice.
- If scope changes, update the steps and add a decision log entry explaining why.

### Completing an Exec Plan

1. Mark all steps `[x]`.
2. Fill in the Retrospective section.
3. Move the file from `active/` to `completed/`.
4. Update status to `completed` with the completion date.

### Referencing Exec Plans

- From AGENTS.md: `See docs/harness/exec-plans/active/NNN-description.md`
- From code comments (rare, only for complex architectural changes): `// See EP-NNN`
- From tech debt tracker: link to the exec plan in the Description column.

## Relationship to Tech Debt Tracker

Exec plans and tech debt items are complementary:

- **Tech debt items** identify what needs fixing (the "what").
- **Exec plans** describe how to fix it (the "how").

A tech debt item may reference an exec plan. An exec plan may be created to resolve one or more tech debt items.
