# Plan Mode Reliability Design

## Summary

Plan mode must reliably turn read-only repository discovery into a validated, persisted plan and then hand approved work
to a write-capable primary agent. The visible workflow identity remains `plan`, but its behavior becomes
discussion-first like `duck`: clarify material ambiguity early, inspect only the evidence needed for a decision, and
synthesize a compact plan instead of continuing open-ended research.

This change also fixes the concrete failure captured in turns 985-987. A valid shell verification using
`target/release/vtcode` was rejected, the model then tried to persist the plan through shell redirection, and the
read-only gate correctly denied that mutation. No canonical plan or task tracker was published, so later `continue`
messages had nothing approval-ready to execute.

## Goals

- Make plan discovery discussion-first and evidence-driven without changing the active workflow identity from `plan` to
  `duck`.
- Ask the user only when a material decision cannot be resolved from the repository or request.
- Persist completed plans exclusively through the canonical `<proposed_plan>` validation and persistence path.
- Accept safe workspace-relative executable paths in verification commands.
- Give actionable, bounded validation repair feedback that does not echo untrusted plan content into a system message.
- Preserve the validated approval boundary and reliably select `build` or `auto` for execution.
- Raise the plan-mode tool-loop floor from 40 to 60 while retaining existing wall-clock, tool-call, blocked-call,
  recovery, and hard-cap safeguards.

## Non-Goals

- Do not make `duck` the active agent or create a delegated duck child thread.
- Do not weaken plan mode's read-only command or filesystem boundary.
- Do not allow shell redirection, `apply_patch`, or arbitrary repository writes during planning.
- Do not bypass plan validation, task-tracker creation, or user approval.
- Do not change the ordinary build-mode tool-loop default of 40.
- Do not add a new top-level harness or planning configuration subsystem.

## Architecture

### Discussion-first planning persona

The built-in `plan` and `duck` prompts will share a small discussion-first guidance fragment owned by `vtcode-config`.
The shared guidance requires the agent to identify ambiguity, ask focused questions, compare relevant options, and stop
researching when existing evidence supports a decision. Plan-specific guidance then adds repository grounding, the
canonical plan shape, and the read-only workflow contract.

The plan prompt must explicitly say that the model never writes the plan file with shell or file-editing tools. It emits
exactly one `<proposed_plan>` block; the runtime validates it, writes the plan and tracker artifacts, and opens the
approval gate. This removes the ambiguity that caused the model to attempt `cat > .vtcode/plans/...` in turn 985.

The runtime planning guidance will repeat this boundary because primary-agent prompts are user-overridable. The compiled
system contract, not only the built-in prompt, must tell every planning agent how canonical persistence works.

### Read-only discovery tools

Plan mode retains its current plan-specific permissions: direct read/search tools, `request_user_input`, and read-only
`exec_command` calls remain available, while the planning dispatch gate remains authoritative for shell mutation
detection.

The read-only shell classifier will recognize literal-output `printf` commands used only to separate inspection output,
including chains where every command is independently read-only. Redirection, command substitution, dynamic shell
syntax, environment mutation, and mixed read/write chains remain denied. Adversarial tests will cover `printf` combined
with redirection, substitution, and a destructive neighbor command.

### Plan validation and repair

Verification parsing will recognize a workspace-relative executable token when it contains a path separator and every
character is from a conservative path alphabet. For example, these are commands:

- `target/release/vtcode --version`
- `scripts/perf/compare.sh before after`
- `./scripts/check-dev.sh --changed`

URLs, shell expressions, assignments without a following command, and prose remain invalid. Validation is syntactic and
does not require the path to exist, because a plan may create the referenced script in an earlier step.

For bracketed verification lists, validation will report the one-based ordinal of each invalid list entry using
validator-owned text, such as `verification item 2 must include a concrete command or check`. It will not copy the
rejected command into the system repair directive. The single bounded repair pass remains unchanged.

### Canonical persistence and approval

The data flow remains:

1. The model explores through read-only tools and optionally calls `request_user_input`.
2. The model emits one `<proposed_plan>`.
3. Response processing extracts and validates the plan.
4. Canonical persistence writes the plan, its sidecar tracker, and the workspace task tracker.
5. Only after all artifacts are readable does the runtime emit plan-ready and approval-requested events.
6. Approval finishes planning, restores permissions, selects a write-capable primary agent, and begins the
   implementation turn.

The existing execution-agent resolver remains authoritative. A prior write-capable `build` or `auto` agent is restored
when possible. A read-only `plan` or `duck` agent is never used for implementation; resolution falls back to the
configured write-capable default and then built-in `build`/`auto`. Tests will cover current-context and fresh-context
approval plus explicit Build/Auto legacy selections.

### Planning budgets

`PLANNING_WORKFLOW_MIN_TOOL_LOOPS` increases from 40 to 60. An explicit zero remains unlimited. Nonzero user values
below 60, including the workspace's current value of 20, resolve to 60 only while planning is active. Ordinary turns
retain their configured value, and the planning extension hard cap stays 240.

The supplied failure used 29 tool-loop batches, so this budget increase is headroom rather than the root-cause fix. The
discussion-first prompt and existing tool-free synthesis recovery remain responsible for preventing the extra capacity
from becoming unbounded research.

## Error Handling and Safety

- Invalid plans are never persisted over an existing valid draft.
- A failed repair ends the turn with the rejected draft and validator-owned reasons visible; it never advertises that
  implementation can start.
- Missing plan or tracker artifacts keep planning active and prevent a mode transition.
- A denied or unavailable interview falls back to the existing bounded plain text/synthesis path.
- Read-only shell classification stays fail-closed for unknown or dynamic syntax.
- Approval does not enable mutations until plan and tracker persistence have succeeded and a write-capable primary agent
  has been selected.

## Testing

Implementation follows test-driven development. Regression coverage will include:

- The turn-985 verification shape validates, including a bracket list whose second command starts with an environment
  assignment followed by `target/release/vtcode`.
- Unsafe relative-path lookalikes and shell expressions remain invalid.
- Repair feedback identifies an invalid verification-list ordinal without echoing its content.
- The compiled plan prompt says to emit `<proposed_plan>` and forbids manual plan-file persistence.
- `printf`-separated read-only inspection chains are admitted, while redirection, substitution, and destructive chains
  are denied.
- Configured loop limits of 20 resolve to 60 in planning and remain 20 outside planning; zero remains unlimited.
- A checkpoint-shaped streamed plan is validated, persisted with both trackers, emits approval events, and hands
  execution to `build` or `auto`.
- Existing planning, tool-policy, task-tracker, and approval tests remain green under `cargo nextest run`.

## Documentation and Module Guidance

Update the planning workflow guide and configuration quick references with the discussion-first behavior, canonical
persistence boundary, relative command validation, and 60-loop planning floor. Update compiled runtime guidance where
the model-facing contract is defined. After implementation, use the `audit-module-agents` skill to determine whether
`src/AGENTS.md`, `vtcode-core/AGENTS.md`, or `vtcode-config/AGENTS.md` needs a concise invariant update.

## Expected Files

- `crates/codegen/vtcode-config/src/subagents.rs`
- `crates/codegen/vtcode-config/src/constants/tool_limits.rs`
- `crates/codegen/vtcode-core/src/prompts/system.rs`
- `crates/codegen/vtcode-core/src/prompts/runtime_contract.rs`
- `crates/codegen/vtcode-core/src/tools/handlers/planning_workflow/artifacts.rs`
- `crates/codegen/vtcode-core/src/tools/handlers/planning_workflow/mod.rs`
- Read-only command-classification tests in the existing tool-intent module
- Planning response/handoff tests under `src/agent/runloop/unified/`
- `docs/guides/planning-workflow.md`
- `docs/config/TOOLS_CONFIG.md`
- `docs/config/CONFIG_FIELD_REFERENCE.md`

No updater or launch-time implementation files are part of this change; their current uncommitted edits belong to the
earlier session and must be preserved.
