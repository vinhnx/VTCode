# Planning Workflow

The planning workflow lets you iterate with the agent on what you want to build before implementation starts. It is
driven by the built-in `plan` primary agent and the `/plan` slash command. Plan is discussion-first: it remains a
distinct read-only workflow that investigates the repository, uses evidence and reasonable defaults, and asks only when
a material choice remains.

## Overview

During planning, the agent can:

- read files and inspect project structure
- search code with grep, structural search, and other read-only tools
- analyse patterns and constraints before proposing changes
- run explicitly safe inspection or validation commands when the active permission policy allows them
- ask clarifying questions through `request_user_input`

The footer shows `Planning...` only while a planning turn is active; a completed draft returns to idle, while exhausted
validation repair leaves the turn blocked with Plan mode still selected. Canonical step metadata uses `-> files:` and
`-> verify:`; arrows inside action prose or quoted verification patterns remain content. List separate verification
commands as comma-separated entries rather than
semicolon chains. Documentation checks such as `npx markdownlint-cli2 README.md` are valid verification commands.
Markdown-only steps may use `verify: [skip Markdown lint if unavailable]` as their sole check; code or mixed-target
steps still require an ordinary command or observable check. If the Markdown linter is unavailable, skip that optional
check, report it as skipped, and continue with a diff and local-link review using available tools. Do not install
tooling solely for this check. Lint errors from an available tool are failures to fix, not grounds to skip validation.

The planning agent does not implement changes, shell-write plan files, or use file-writing tools for plan persistence.
It emits exactly one final `<proposed_plan>` block when the plan is ready. The runtime validates and persists that plan,
then exposes approval controls. A completed planning checklist alone does not persist a plan. Untagged Markdown drafts
are retained for validation before fallback interviews: invalid drafts receive bounded repair feedback, and corrected
drafts reach the same persistence and approval gate. Clarifying questions still wait for user input; exhausted repair
attempts end with the validator's reasons instead of claiming readiness. During an approved handoff, the runtime creates
and persists the `task_tracker` before handing off to the write-capable `build` agent or configured `auto` workflow.

The built-in `plan` agent's permission rules allow `read`, `request_user_input`, and `bash` so its wire catalog keeps
`exec_command`, `code_search`, `grep_file`, and the interview tool visible. Read-only enforcement is not delegated to
those permissions: the planning dispatch gate hard-blocks every mutating tool call (and non-allow-listed shell command)
before execution, so granting `bash` admits the tool without weakening plan-mode safety. The `plan` agent is also
excluded by name from approved-plan execution routing — selecting it always re-enters planning, never implementation.

The session-derived regression exercises draft extraction, interview gating, validation repair, persistence,
and the emitted HITL approval overlay. This retains VT Code's validation gate; the corresponding Codex
[plan implementation prompt](https://github.com/openai/codex/blob/f0c5077c34ec7a76b4ad9134835925e6ae3d72ec/codex-rs/tui/src/chatwidget/turn_runtime.rs#L243)
also waits for a plan item before offering implementation. No upstream code was copied.

A cancelled planning task resumes only after fresh user input. Resume notifications wake the runtime but cannot cancel
a provider request unless local cancellation or exit state is set. Provider error text such as “Interrupted by user”
without a local stop request is a provider failure with retained planning context, not a user cancellation. Streaming
setup, streamed events, non-streaming requests, and retry waits share this state-based stop check. Stable tool catalogs
reuse their definitions across mode switches while rebinding planning metadata to the current mode, so prompt/catalog
validation does not repeatedly rebuild a stale Build snapshot during planning.
The pinned [Codex sampling path](https://github.com/openai/codex/blob/f0c5077c34ec7a76b4ad9134835925e6ae3d72ec/codex-rs/core/src/session/turn.rs#L1819)
also checks authoritative cancellation state; VT Code retains its existing `CtrlCState` and notification mechanism.

## Bounded blocked-call recovery

Blocked and denied tool calls are bounded per turn to prevent retry churn. The configured
`tools.max_consecutive_blocked_tool_calls_per_turn` value remains the consecutive-call cap. The total fuse is two times
that cap in normal mode, four times that cap in Plan Mode, and the consecutive cap in recovery mode. The fuse is strict:
with a cap of `3`, Plan Mode permits 12 non-consecutive blocked calls and stops on call 13. A successful or otherwise
allowed call resets the consecutive streak, but not the turn's total blocked-call count.

When a turn stops because of blocked behavior, VT Code forces a session-history checkpoint before writing the blocked
handoff. The handoff advertises `vtcode --resume <archive-id>` only after that archive is successfully persisted and its
identifier is verified. If history persistence is disabled or the checkpoint fails, the handoff explains that resume is
unavailable and does not advertise a misleading command. Interactive sessions return to the next input after the handoff
— except for typed synthesizing or repairing outcomes with remaining allowance, which may auto-continue
a bounded follow-up turn in every mode before control returns to you; manual `continue` matters once that budget
(`agent.harness.continuation.cross_turn_turns`) is exhausted.

Runner paths that do not create session archives also omit the resume command and state that limitation in the handoff.

When a turn blocks while planning is active, the transcript adds plan-mode guidance alongside the generic `continue`
nudge: plan mode is read-only by design, so `Mutation blocked` means the edit was stopped by policy (not by a failing
check) and a repeated `Turn blocked` will re-block if the same mutating tools are retried. The persisted handoff file
carries the same guidance in its Actionable Next Steps so a resumed session sees it without the original transcript.
Stay planning with `continue`, or implement by approving the plan or running `/mode build` (`/mode auto` for unattended
confirmation). The input placeholder switches to the same choice. VT Code never auto-switches modes from a blocked turn;
the user decides on the next turn. `build` and `auto` share the same tool catalog and safety gates — only confirmation
policy differs.

Shell commands in plan mode are validated against a read-only allow-list. Allowed patterns include:

- inspection base commands: `rg`, `ls`, `cat`, `sed`, `grep`, `find`, `head`, `tail`, `fd`, `tree`, `stat`, `file`,
  `which`, `jq`, and similar
- discovery commands must remain static: use literal `find` paths with quoted patterns, or prefer `rg --files`; dynamic
  `find` option/predicate expansion such as `$()`, backticks, variables, or brace expansion is rejected by the
  command-safety gate
- `cd` prefixes: `cd <dir> && <read-only command>` (changing directory mutates nothing)
- read-only subcommands:
  `git status|log|diff|show|blame|ls-files|rev-parse|describe|shortlog|grep|rev-list|ls-tree|cat-file|diff-tree|merge-base|range-diff|whatchanged|count-objects|var|version|help`,
  `cargo check|test|clippy|metadata|tree|nextest run`, `npm|pnpm|yarn test`
- git repository redirects: `git -C <dir> <read-only subcommand>` (only changes which repository is read; `-c` config
  injection stays rejected)
- git list-form subcommands: bare/flag-only `git tag`, `git branch`, `git remote`, `git reflog`, plus
  `git stash list|show` and `git worktree list` (operand forms such as `git tag v1`, `git branch -D x`, `git stash pop`,
  `git remote add` are rejected, as are upstream/description mutators like `git branch --set-upstream-to=<ref>`)
- version/help probes for any program: `cargo --version`, `python3 -V`, `bash --help` — every trailing word must be a
  probe flag (`-h`, `--help`, `-V`, `--version`)
- `&&` chains and `|` pipelines where every segment is itself read-only
- static `;` chains where every segment is independently read-only; literal-output `printf` is allowed as an
  inspection-output separator
- `2>&1` stderr merges (no file is written)

Rejected: file redirections (`>`, `>>`), command substitution (`$(...)`, backticks), dynamic `;` chains, in-place edits
(`sed -i`), and any chain with a mutating or unknown segment (`rm`, `mv`, `cargo build`, `git push`, arbitrary scripts).

During planning, the dispatch gate denies mutating tools. Plan remains read-only; the runtime alone persists validated
planning artifacts under `.vtcode/plans/`.

`task_tracker` is available for checklist state. Planning output should use `<proposed_plan>...</proposed_plan>` when
the agent is ready for user review.

Tracker updates use action-aware indices. Standard checklist item indices are positive and 1-based; the compatibility
form `index: 0` is reserved for checklist-level completion with `status: "completed"`. Planning workflow updates accept
positive flat indices or positive hierarchical `index_path` values such as `2.1`. Use `items` for bulk synchronization
rather than an item index. Bulk synchronization replaces the full checklist and its progress. Omitted metadata is
preserved only for descriptions that occur exactly once in both the old and new lists, including after reordering.
Matching ignores surrounding whitespace, including child indentation in the flat mirror after leaving planning mode.
Explicit metadata replaces individual fields; empty `files`/`verify` arrays and an empty `outcome` clear them.
Renamed or new steps need their own metadata. Duplicate descriptions with existing metadata require explicit
`files`, `outcome`, and `verify` to avoid transferring verification requirements to the wrong task. Send
complete descriptions with status prefixes or actual objects such as
`{"description":"Review links","status":"completed"}`. JSON-encoded update commands inside strings are rejected before
changing the tracker. To change one existing step while preserving its description and metadata, use
`{"action":"update","index_path":"1","status":"completed"}`.

Tracker lists and updates execute against live checklist state, including after approval or checklist replacement;
they must not replay earlier results from read-only caches or conversation history. Use the latest update result for
progress totals and the changed item. Request `action: "list"` only when the full current checklist is needed.
Reuse successful results rather than repeating unchanged updates. Repeat limits apply to consecutive successful
identical tracker calls; a different successful operation resets that streak, while failed calls do not.
Genuine request boundaries start a fresh tracker streak without deleting execution records;
internal recovery turns retain it. Repeated observations of the same checklist count as navigation,
including across output-budget changes. When verification is pending, the harness runs the missing
check through the normal execution pipeline. Otherwise one correction asks the agent to update the
indexed task; continued unchanged polling ends with a blocked handoff.
Identical indexed or bulk updates return `status: "unchanged"` with current totals and retain modification times
for identical artifacts. Planning updates still repair missing or stale mirrors and the embedded tracker.
Tracker artifacts use the shared private atomic writer (owner read/write on Unix); symlink redirects are rejected.
Reads and no-op updates do not rewrite identical mirrors.
All tracker artifacts are read before an update is published. Reported publication failures roll back earlier
writes; detected concurrent edits are preserved and rollback failures remain visible in the error.

Headless evaluator/replanner additions reconcile against one live tracker snapshot per batch.
Repeated descriptions and metadata already covered by the existing step preserve its status and checks;
omitted fields never erase existing metadata.
A unique unfinished placeholder can gain its missing metadata in place. Completed placeholders, ambiguous duplicate
descriptions, and different verification criteria are retained; new criteria become a separate pending item.
This reconciliation applies to harness-generated revisions, leaving the explicit tracker `add` action unchanged.

Successful tracker updates honor display mode: compact shows header plus the single current task (`» …`, first
`in_progress` leaf else first `pending`/`blocked`; header-only when all done) while expanded shows header plus truncated
tree (max 30 rows + `… N more`) so each task item is visible inline. The full hierarchical tree is always TODO-panel
body content when the user opens the panel. Parent rows show their branch and description; leaf status surfaces through
text styling only — done rows render struck-through, italic, and dimmed, the focused current row renders bold in the
theme `primary` accent, blocked rows use the `warning` token, and pending rows keep the default style. Descriptions
render inline markdown (code spans, emphasis, file paths); inline `-> files:` / `-> verify:` suffixes copied from plan
steps are stripped into structured fields. Files, outcomes, and verification commands remain structured tracker metadata
rather than extra visible rows.

## Usage

### Start With The Planning Agent

Set the default primary agent to `plan` when you want new sessions to start with the built-in planning agent:

```toml
default_primary_agent = "plan"
```

You can also press `Tab` on an empty idle composer to cycle to the `plan` primary agent.

### Use `/plan`

`/plan` starts or continues the planning workflow. Entering planning is a full switch to the built-in `plan` primary
agent: the session header badge shows Plan, and the prompt, tool catalog, and permissions match plan mode. `/plan off`
finishes planning and restores the previous execution agent (`build`/`auto`) when one was recorded.

While a turn is actively processing, `/plan` is dropped with a notice (mode switches are locked for the duration of a
turn). The automatic in-turn planning intent detection still engages on its own; only explicit `/plan` entry while busy
is deferred.

```text
/plan
```

### Enter From an Agent Suggestion

The agent can also propose entering the planning workflow on its own when it judges edits should be planned first. Under
an interactive policy a HITL confirmation prompt appears:

```text
Enter Planning workflow?

- Enter Planning workflow (Recommended) — enter read-only planning and research
- Continue without Planning workflow
```

- **Enter Planning workflow** — starts planning; read-only research begins and mutating tools stay disabled until you
  approve execution. The header badge switches to Plan immediately, and the plan primary agent is selected after the
  turn so prompt/tools match. Research continues in the same turn (the entry does not stop the turn). The runtime
  persists the validated plan after the final `<proposed_plan>` is emitted.
- **Continue without Planning workflow** — the agent proceeds without planning (mutating tools remain enabled).

This gate prevents the agent from silently switching into plan mode; you decide whether to plan before any edits begin.
Full-auto and skip-confirmations policies accept the suggestion directly, including in an interactive UI.

Mode-switch failures are recoverable and logged under the `vtcode.planning_workflow` target with a `switch_path` field
(`plan_entry`, `plan_approval`, `plan_exit`, `startup_plan_entry`). A failed plan-agent selection keeps Planning active
and the Plan header visible; a failed approval handoff keeps the approved plan and allows retry.

Execution agents such as `build`, `auto`, and `duck` can invoke the `start_planning` tool when a request is demanding,
ambiguous, or has multiple phases. The tool only presents the entry prompt; it does not silently change mode until you
confirm. Straightforward requests continue directly in the active execution agent. In a headless session without an
automatic execution policy, the suggestion is reported as pending and the turn stops safely; use `/plan` on the next
turn to confirm entry. Full-auto or skip-confirmations policies may accept the suggestion automatically.

### Intent Phrases

You can steer the workflow with short phrases instead of the review-gate UI:

- To **exit planning and present the plan** for approval, type `implement`, `approve`, `lgtm`, `ship it`,
  `yes`/`continue`/`go`/`start`, or select **Execute** / **Auto-accept** in the review gate. The plan is shown in an
  inline confirmation overlay (or a text prompt in non-interactive mode); the agent will not self-approve by editing the
  plan file and staying in plan mode.
- To **stay in planning**, type `stay in planning` (or revise the `<proposed_plan>` block). This overrides any exit
  phrase.
- To **cancel planning without implementation**, type `no`, `cancel`, or `abandon plan`, use `/plan off`, or switch away
  from the `plan` agent. VT Code emits a terminal `cancel` approval event and removes the active plan draft when the
  workflow is explicitly abandoned.

### Typical Workflow

1. Select the `plan` primary agent or run `/plan`.
2. Describe the goal and constraints.
3. Iterate on repository facts, risks, and open decisions.
4. Review the emitted `<proposed_plan>` block.
5. Switch to a build-oriented primary agent such as `build` or `auto` when you are ready to implement.

Approval resolves one typed execution target before leaving Planning. The normal current-context and fresh-context
choices target `build`; they do not inherit `auto` merely because planning began from `auto`. `auto` is selected only by
the explicit **Switch to Auto** choice or an explicitly configured full-auto policy. An explicit destination always wins
over the agent that was active when planning began, and read-only agents such as `duck` and `plan` are never selected to
execute an approved plan.

`build` and `auto` use the same write-capable tool catalog and runtime safety gates. The only intended difference is
confirmation policy: Build remains confirmation-aware, while Auto may continue unattended after that policy was
explicitly selected. Auto does not receive broader authority, larger blocked- call limits, or a bypass around command,
path, verification, budget, or recovery safeguards.

The approval overlay shows a compact synopsis so its choices remain visible, while the complete plan markdown is
appended to the scrollable TUI transcript and remains available from the persisted plan file. The queued implementation
turn revalidates the handoff, exits any stale planning gate, and refreshes the selected agent's permission snapshot
before tools are exposed. Mouse-wheel events outside the floating overlay are passed through to the transcript, so the
full plan can be reviewed without dismissing the approval gate. Approval also distills the plan's numbered and checkbox
steps into the `task_tracker` checklist; the implementation agent updates that checklist as work progresses.
Approved-plan execution receives a separate implementation safety budget so planning research does not leave the build
phase with only the ordinary short-turn allowance.

That implementation turn also receives one internal `+50` tool-loop allowance at initialization. The allowance is
clamped by the ordinary loop hard cap (for example, the default `40` becomes `90`, while `100` remains capped at `120`),
does not change the tool-call budget, and does not stack with manual loop extensions. A configured loop value of `0`
remains unlimited. Current and fresh-context approved-plan handoffs each initialize their own implementation turn, so
each gets the same one-time allowance.

The implementation request is scheduled as an explicit internal next-turn trigger after the handoff directive is
appended. It is not dependent on the bounded ordinary steering FIFO, and the synthetic user prompt is recorded once so
an approved plan cannot switch destination and then wait for another `continue` input. Current-context and fresh-context
approvals use the same handoff sequence: validate the persisted plan, persist and verify the task tracker, resolve
approval telemetry, leave Planning, select the destination, refresh permissions and tool catalogs, then enqueue exactly
one implementation turn. If that sequence cannot complete, the validated plan remains available for a resumable retry.

### Validated Approval Handoff

Approval is accepted only for a persisted plan that passes the artifact validator and has a persisted task tracker. The
canonical sections are `Summary`, `Implementation Steps`, `Test Cases and Validation`, and `Assumptions and Defaults`;
the documented short aliases `Steps`, `Validation`, and `Assumptions` are accepted case-insensitively. Plans may also
include optional `Expected Outcomes` and `Dependencies and Prerequisites` sections; the validator tolerates additional
sections and enforces only the canonical four. Every numbered implementation step must name a concrete file, symbol,
behavior, or other repository target and include a non-empty `verify:`/`verification:` command or check. Placeholder
tokens and unresolved `Next open decision` or `Open question` entries block approval. Invalid candidates are rejected
before persistence, so an existing valid draft is preserved. VT Code gives the model up to two repair requests; if the
repaired artifact is still invalid, planning remains active with the validation reasons visible. Once the bounded repair
attempts are exhausted, the turn ends blocked, retains the rejected draft and validator feedback, and waits for a revised
request instead of automatically continuing or reporting success.

Creating the `task_tracker` checklist is part of the approval gate. If the tracker tool is unavailable, fails, or does
not persist its tracker file, the planning workflow remains active and no write-capable execution turn is started. All
approval routes share the same typed handoff, including direct, queued, automatic, and fresh-context execution.

### Mid-Execution Replans

Execution agents may emit one `<proposed_plan>` block while implementing when the plan they are executing no longer
matches the repository (for example, the approved paths were renamed or the scope changed materially). The runtime
extracts that block from the response, validates and persists it exactly like a planning draft — overwriting the plan
file and refreshing its tracker sidecar — and publishes the normal `plan.delta` and `plan.approval.requested` events.

Routing follows the session state and confirmation policy:

- While an approved-plan execution turn is in flight, the revision continues automatically: the user already approved
  implementing this goal, so no second confirmation gate is shown. The transcript reports the revision, the persisted
  plan is updated, and the continuation implementation turn is scheduled with the session's existing confirmation policy
  preserved.
- Otherwise the ordinary approval routes apply: interactive sessions show the "Ready to code?" overlay with a notice
  that the plan was proposed during execution, full-auto or skip-confirmations policies continue without a prompt, and
  headless sessions wait for `approve` or `implement`.

An invalid revision is rejected with visible feedback, never overwrites the persisted plan, and never pushes
planning-repair directives into the execution turn; no continuation turn is scheduled in that case.
The rejected revision ends the turn blocked; a final rejection message does not count as successful implementation.

### Streaming-to-persistence handoff

In streaming mode, `<proposed_plan>` markup is removed from the live transcript so the plan is rendered by the approval
flow exactly once. The stream parser retains the plan body, including when tag boundaries are split across tokens, and
attaches one canonical `<proposed_plan>...</proposed_plan>` block to the completed response before response processing
runs. Completed provider content takes precedence over visible streamed prose; an existing recognized plan block is not
duplicated. This semantic handoff also occurs when output is suppressed while verification is pending, so the normal
response-processing path still extracts, validates, persists, and publishes approval events. Rendering suppression never
bypasses plan validation or persistence.

### Clarification Interviews

When the planning agent reaches a material ambiguity, or identifies an open decision before presenting a plan, it asks
through `request_user_input`. In an interactive session this opens an inline wizard with a selectable option list (and
an optional free-form note). The selected answer is returned to the same planning turn, recorded as the completed
interview, and supplied to the next model request so planning can continue with the user's choice.

Pressing `Esc` or `Ctrl-C` cancels the interview without submitting an answer; the planning agent may ask again when the
ambiguity still matters. A plan approval popup is not shown until any required clarification has completed.

`request_user_input` is optional in headless and other noninteractive runtimes. A permanent denial is recorded for the
planning session and suppresses repeated interview attempts. VT Code gives the model one bounded synthesis retry using
the repository evidence already gathered. If that retry does not produce a validated persisted plan, the session stays
in planning and shows a keep-planning message; it does not advertise implementation or emit approval events.

### Bounded synthesis recovery

After a research guard closes tools, one synthesis request and at most two corrective requests share a request-scoped
allowance. Empty content, private reasoning without final content, native tool-call violations, truncated output,
malformed plans, and provider failures all use the same allowance. Automatic continuations and session resume retain
consumed requests and cannot reopen research. Transport retries also consume this allowance. Private reasoning never
becomes a plan or final response.

The runtime retains the latest request, bounded public evidence, best structured draft, and validator report in the
canonical session event log. A repair targets that draft and its specific defects. Approval-only prose cannot replace
the draft or change readiness. Complete unwrapped plans still validate; approval requires successful artifact and
checkpoint persistence.

On exhaustion, a runtime-generated interview offers these choices without another model response:

| Choice | Result |
| --- | --- |
| Retry from retained evidence | One explicitly requested synthesis cycle; research stays closed. |
| Add clarification | Free text revises the retained plan and starts a bounded cycle. |
| Choose another model | The existing picker opens; synthesis waits for an explicit selection. |

No option submits automatically. Dismissal leaves planning idle and resumable. A genuine clarifying question may pause
before exhaustion. Headless or unavailable interviews produce a structured `planning.awaiting_input` diagnostic with
the same choices. Answers do not approve implementation. Cancellation, permissions, and hard execution budgets remain
in force. See the [recovery contract](../development/planning-recovery.md), [validator contract](../development/planning-validation.md),
and [provider audit](../development/planning-provider-audit.md).

## Plan Output Format

Planning output should stay decision-complete but sparse — treat it like a compact spec, not prose. Keep the whole
`<proposed_plan>` under ~1500 tokens; prefer `file:symbol` references over narrative. This bound exists because an
overly verbose plan is truncated at the model's output-token limit (cut off mid-plan) and must then be condensed and
re-emitted.

File references must be plain text or inline code, never markdown links or editor/IDE URIs — plans are read in terminals
and other non-hyperlink surfaces:

```markdown
Correct: `src/main.rs:42` or src/main.rs:42
Incorrect: [main.rs:42](vscode-file://vscode-app/.../workbench.html)
Incorrect: [main.rs](file:///Users/you/repo/src/main.rs#L42)
```

```markdown
Repository facts checked:

- [file:symbol or behaviour confirmed from the repo]
- [observed command output -> the insight it establishes]

Next open decision: [if any], otherwise: No remaining scope decisions.

<proposed_plan>

# [Task Title]

## Summary

[1-3 lines: goal, user impact, what changes / what does not]

## Scope

- In: [concrete surfaces/behaviours this plan changes]
- Out: [explicit non-goals; keep aligned with Assumptions when material]

## Implementation Steps

1. [Action] -> files: [path/to/file.rs] -> verify: [check]
2. [Action] -> files: [path/to/file.rs] -> verify: [check]

## Test Cases and Validation

- build/lint: [detected toolchain command]
- tests: [detected toolchain command]
- behaviour: [targeted check]

## Expected Outcomes

- [observable end state the implementation must produce]

## Dependencies and Prerequisites

- [required tooling, configuration, or prior work]

## Assumptions and Defaults

- [assumption or default chosen]
- [out-of-scope item intentionally not changed]

</proposed_plan>
```

`Scope` records in/out boundaries for the user and for later plan review. Scope lines are **plan context only**:
`task_tracker` items are distilled from `Implementation Steps` / `Steps`, never from `Scope`. The runtime validator
still requires the canonical four sections below and treats `Scope` as a documented section for new drafts (older plans
without it remain valid).

`Expected Outcomes` and `Dependencies and Prerequisites` are optional sections: add them when they are material to the
request — what observable end state the work must produce, and which tooling, configuration, or prior work must exist
before implementation — and omit them when nothing material exists. The validator tolerates additional sections; only
`Summary`, `Implementation Steps`, `Test Cases and Validation`, and `Assumptions and Defaults` are required.

Each comma-separated `verify:` entry must itself be a concrete command or observable check. Commas inside single or
double quotes stay inside one item, so a quoted shell pattern such as `rg -n 'a,b' README.md` remains a single check.
Build/test commands (`cargo nextest run -p vtcode`, `cargo check --locked`) and common inspection commands (`rg -n`,
`sed -n`, `grep -n`, `head`, `wc`) are all concrete when they are the command head of a `verify:` item. Command heads
that are also common English words (`file`, `sort`, `find`, `ls`, `wc`, `make`, …) must carry a flag or path-like
argument (`file src/main.rs`, `wc -l README.md`) — bare prose such as `file changes` is not a check. Prefer the simplest
check that expresses the same outcome — for docs changes, `grep -n 'symbol' path` or `sed -n '1,40p' path` are valid;
`verify: [run checks]` and `verify: [git diff --check]` are not.

Read-only reviews may use targeted Git history checks as verification: `git log -5 --oneline`, `git show --stat HEAD`,
`git diff HEAD~1 -- src/main.rs`, or `git blame src/main.rs`. Select a revision, path, or filter so the check provides
evidence for the step. Bare Git commands, mutating Git commands, and `git diff --check` do not verify a review outcome.
During planning recovery, the agent synthesizes from evidence already visible in the transcript. A single repeated read
among productive inspections does not force synthesis; the low-signal and repeated-navigation guards still bound genuine
loops.

`Next open decision` and `Open question` entries are explicit reopen markers for follow-up planning; use a resolved
statement such as `No remaining scope decisions` when none remain.

### Reasoning and Evidence

A plan is only as strong as the evidence behind it, and planning output should show the reasoning chain, not just the
conclusion:

- Ground every claim in a `file:symbol` reference or an observed read-only command output. Never speculate about code
  you have not opened.
- Pair each observed fact with its insight. A failing test line is evidence; the root cause, constraint, or bottleneck
  it establishes is the insight. Record the insight — the raw output belongs in the transcript, not the plan.
- Put load-bearing findings (a verified root cause, a hard constraint) in `Repository facts checked` or, when they
  define the work itself, in `Summary`.
- Make each step's `verify:` check the stated insight, so approval hands the implementation agent a falsifiable target
  instead of a description.

### Common Scenarios

The reasoning chain above applies whenever a request needs diagnosis before change. Three recurring cases:

**Troubleshooting a command failure.** A build, test, or runtime command fails. Run it read-only, capture the decisive
output lines, and isolate the offending code before proposing a fix:

```markdown
Repository facts checked:

- `cargo test parser::tests::parse_case` fails: `assertion failed: left == right` at src/parser.rs:120
- src/parser.rs:118 advances the cursor before the bounds check, so the final byte is read twice

<proposed_plan>

# Fix off-by-one in parser cursor advance

## Summary

`parse_case` reads the final byte twice and fails the round-trip test. The fix
hoists the bounds check above the cursor advance.

## Implementation Steps

1. Hoist bounds check above cursor advance -> files: [src/parser.rs] -> verify: [cargo test parser::tests::parse_case]
2. Add single-byte regression test -> files: [src/parser.rs] -> verify: [cargo test parser::tests::single_byte]

## Test Cases and Validation

- build/lint: `cargo check`; `cargo clippy --workspace --all-targets -- -D warnings`
- tests: `cargo nextest run`
- behaviour: the previously failing `parse_case` passes

## Expected Outcomes

- `parse_case` accepts single-byte input without double-reading; no other parser behaviour changes

## Assumptions and Defaults

- only the cursor advance changes; the grammar itself is out of scope

</proposed_plan>
```

**Optimizing a workflow.** A slow build, test, or check loop. Measure first so the numbers drive the plan, then verify
the win without breaking correctness:

- Repository facts: `cargo build --timings` puts 41% of build time in one crate; the check script rebuilds three crates
  the dev script does not.
- Insight: the bottleneck is that crate's feature set, not link time.
- Steps shape: trim the unused feature → re-time the build against the recorded baseline → run the full test gate. Each
  `verify:` is either the timing comparison or the correctness gate, so the optimization cannot ship as an unmeasured
  claim.

**Root-cause bug fix.** For a behavioural bug — for example, a fullscreen TUI transcript leaking into the CLI scrollback
after exit — the diagnosis-heavy plan maps onto the template section by section:

| Diagnosis plan part           | Template home                                                                    |
| ----------------------------- | -------------------------------------------------------------------------------- |
| Root cause (verified in code) | `Repository facts checked` — file:line evidence for the shutdown race            |
| Fix, in layers                | `Implementation Steps`, each with its own `verify:`                              |
| Verification and docs work    | `Test Cases and Validation`                                                      |
| Scope notes                   | `Assumptions and Defaults` (intentionally unchanged surface)                     |
| —                             | `Expected Outcomes`: "CLI scrollback is clean after a plain exit"                |
| —                             | `Dependencies and Prerequisites`: PTY harness available for the regression check |

### Research Scope

Research effort should scale with the request. The runtime gives planning a minimum per-turn tool-call budget of 120 and
a minimum loop budget of 60 when configured nonzero limits are lower; these floors are planning-specific and do not
change the conversation-turn retention limit. For a narrow or simple ask, a handful of targeted reads/searches (roughly
5-10) is usually enough before drafting `<proposed_plan>` — exhaustively enumerating the whole repository for a simple
request wastes the turn's tool-call and wall-clock budget and can exhaust it before a plan is produced. For a broad or
ambiguous ask, research proportionally more, but stop and draft as soon as the scope/decomposition/verification
decisions are closed.

The 120-call ceiling remains available for complex work; it is not a target. Planning also tracks low-signal navigation
separately from the hard ceiling. Six consecutive low-signal reads/searches, or ten total within one turn, schedule one
tool-free synthesis pass from the evidence already gathered. A productive navigation resets only the consecutive count.
Verification, mutation, recovery, and a new turn reset both counts. Shell inspection such as `rg`, `find`, `cat`, and
simple `sed -n` contributes to navigation accounting; compile, test, build, and clippy commands count as verification
progress. Successful reads can still drift into a loop, so twelve consecutive planning inspections containing even one
repeated semantic request schedule the same synthesis pass. Output controls such as `max_output_tokens` do not make an
otherwise identical inspection count as new research; twelve genuinely distinct inspections remain eligible for
continued research. The generic turn-balancer caps apply as well; in planning mode they converge on the same
plan-synthesis pass (with the `<proposed_plan>` contract) and consume the same once-per-turn slot. In execution mode,
three same-target listings — one tool (`ls`, `find`, or `fd`) rescanning the same search root across argument variations
— schedule the same single tool-free recovery pass; scans of distinct trees never group. Planning mode uses a higher
tripwire of five so three successful listings remain legitimate exploration under the dedicated 6/10 low-signal and
12-step synthesis guards. Execution mode also converges when diverse low-signal churn reaches twelve outcomes in one
window (a new query each time never repeats a family); the window resets on any mutation or verification, so churn
interrupted by real work does not accumulate. When a recovery pass is armed this way, the balancer names the dominant
churn family (for example `exec::inspection::ls::src ×3`) in the injected reason, and a pass that is already pending
from another guard or the blocked-tool fuse is never re-announced. A planning tool-free violation that still carries
plan-like salvage consumes one validation-repair budget and retries tool-free before the resumable handoff, and every
failed-closed turn budget (`tool_calls`, `tool_loop`, `session_calls`) is recorded as a `budget_exhausted` trajectory
event with its used/max counts so post-hoc diagnosis can tell which ceiling fired.

Tool previews shown to the model share a bounded per-turn budget (96 KiB in planning, 64 KiB execution); plan-mode
non-verification inspections default to a smaller per-result preview (~8 KiB when omitted) and explicit values are
clamped to ~16 KiB, while verification commands keep the full default. Once it is exhausted, further tool responses
arrive as metadata stubs without body content. In planning mode this schedules the same single tool-free synthesis pass
immediately — additional inspection cannot surface new evidence, so the turn converges on a plan from what was gathered
before exhaustion instead of retrying blind calls. Paged spool reads keep working after exhaustion through a small
per-page credit (bounded by the sequential spool-read cap), and non-spool inspections are rejected at admission while
verification commands, the interview, task bookkeeping, session polling, and plan-draft re-reads stay open.

## Review Gate

After a plan is ready, an interactive human-in-the-loop (HITL) confirmation popup presents a bounded, decision-ready
synopsis (summary plus numbered steps) and a decision gate. The complete markdown remains available in the persisted
plan file and runtime plan events; summary and step entries keep their full text and wrap to the modal width, while the
step count is bounded with an explicit `… and N more plan steps` overflow row rather than silently clipped.

Approval options:

- **Yes, implement this plan** — execute in the current context as `build` while preserving the session's existing
  confirmation policy.
- **Yes, clear context and implement** — execute in a fresh `build` thread while preserving the approved plan and task
  tracker. The subtitle reports the pre-reset context usage (for example, `Fresh thread. Context: 7% used.`). This is
  recommended after long research sessions.
- **Yes, switch to Auto and implement** — execute in the current context as `auto` with unattended confirmations. The
  same command, path, verification, blocked-call, budget, and recovery gates remain active.
- **No, stay in Plan mode** — return to planning and revise the plan.

`Ctrl+G` in the approval popup opens the persisted plan file in your configured external editor (`tools.editor`). After
you save and close the editor, VT Code re-reads and re-validates the saved markdown and shows the approval popup again
with the updated synopsis; approving then builds the task tracker from your edited plan. If the edited draft fails
validation, the previous plan stays active and a warning explains why.

The existing manual confirmation policy remains attached to the Build approval paths. The Auto choice explicitly selects
the unattended confirmation policy; ordinary approval does not infer it from the source agent. A fresh handoff clears
only transient transcript, continuation, cache-lineage, recovery, and tool-budget state; the plan file, task tracker,
working tree, configuration, provider, permissions, and aggregate usage remain intact. The UI shows
`Preparing fresh execution thread...`, `Restoring approved plan...`, and `Starting build...` while the handoff is active
and guards input and mode switches until it completes.

The confirmation policy is explicit handoff state and is not inferred from the destination agent's name. Textual
approvals use the same policy already selected by the session.

The approved execution turn ends with a concise summary of the outcome, changed files, verification performed, and
remaining blockers. In interactive sessions this appears alongside the final response. In headless sessions, a plan that
requires confirmation remains pending and the run stops safely after emitting the plan; send `approve` or `implement` on
a later turn to continue. Existing full-auto or skip-confirmation policies may continue automatically without an
interactive prompt.

### Runtime Events

All clients can reconstruct the approval lifecycle from the authoritative `ThreadEvent` stream. A plan turn emits
`plan.delta` and the completed plan item, then `plan.approval.requested` with the producing turn and plan file. The
terminal decision is emitted as `plan.approval.resolved` with one of `execute`, `fresh_context`, `switch_build`,
`switch_auto`, `revise`, `cancel`, or `auto_accept`, plus an `automatic` flag. The event is emitted once only after the
validated plan and task tracker are ready to hand off; a failed handoff keeps the plan pending for retry. A successful
fresh handoff then emits `context.reset` with the trigger, plan-preserved status, previous context usage, and
tool-budget reset status. The Open Responses bridge forwards this as `vtcode.context_reset`.

## Budget Exhaustion

If the configured planning tool-loop limit is reached, the runloop stops research and enters the tool-free recovery
path. It asks for one compact decision-ready plan from the evidence already gathered rather than ending with only a
loop-limit message. After synthesis, review the plan and choose the current-context implementation or the fresh-thread
handoff. Session budget and wall-clock hard caps remain enforced; when those caps prevent synthesis, the draft and
research are preserved for a later approval or revision command.

During implementation, tool results remain causally attached to the assistant tool-call batch that produced them.
Recovery and system directives are appended only after the complete batch, so a provider never receives a directive
between the assistant call and its results. Request assembly may create a repaired, request-only view when older history
contains split, orphaned, duplicate, or missing results; the durable session history is not rewritten by this repair.

If the session tool-call limit is reached during an active turn, VT Code shows the current primary agent and asks
whether to grant more calls. A grant retries the pending call in the same turn, emits `session_tool_limit_increased`,
and preserves the granted session fuse across runtime limit refreshes and the next turn. A denial remains an explicit
denial. The analogous tool-loop prompt emits `tool_loop_limit_increased` and keeps tools enabled for the current agent;
it does not enter tool-free recovery. Only the first tool-loop grant in a session prompts: after one manual grant, later
tool-loop limit hits auto-grant the remaining increment without a prompt (a denial never latches that preauthorization,
so the prompt returns if you previously denied). Full-auto auto-grants regardless, and the hard cap still applies.
Transient bridge submissions, including mode-switch input, are deferred or ignored while the prompt owns input and are
not interpreted as a denial.

## Plan File Persistence

The runtime-owned draft is the single source of truth and lives on disk under `.vtcode/plans/<plan>.md`, not only in
chat history. The planning agent never writes this file itself. A candidate is validated before the plan file, sidecar
tracker, global tracker, or approval events are created or updated. Invalid or partial inline plans from normal or
tool-free recovery are discarded for approval and cannot overwrite an existing valid draft; the existing malformed file
is preserved until a later valid synthesis repairs it. Only a validated `<proposed_plan>` is extracted and written
during tool-free recovery. If no valid draft exists, the recovery message tells the user to keep planning rather than
offering `implement`.

## Best Practices

1. Be specific about files, functions, constraints, and desired behaviour.
2. Ask the agent to state trade-offs before implementation begins.
3. Ask the agent to record expected outcomes and any prerequisites when they affect implementation order or risk.
4. Keep the planning agent read-oriented and switch to `build`, `auto`, or `review` for the next phase.

## See Also

- [Command reference](../user-guide/commands.md)
- [Subagents and primary agents](../user-guide/subagents.md)
- [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md)
