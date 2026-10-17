# Durable local matrix orchestration

Select the opt-in `coordinator` primary agent with `/agent`, or set `default_primary_agent = "coordinator"`.
Normal Build sessions keep their existing direct execution and selective delegation behavior. The coordinator
defines tasks and makes decisions; scheduler-owned workers execute instructions, edit files, and verify results.
The coordinator may delegate read-only discovery while the matrix is idle. Active execution uses only workers
assigned by the scheduler. Matrix workers cannot delegate further.

## Define and control a matrix

Call `matrix` with `action: "create"` and an explicit `spec`. Creation durably records the specification without
launching work. `start` validates the Git workspace and freezes it before dispatch. A specification has a stable `id`,
a `tasks` list in declaration order, and a `resources` map of named pools with positive integer capacities.

```json
{
  "action": "create",
  "spec": {
    "id": "workspace-checks",
    "resources": {"cargo-target": 1, "device": 1},
    "tasks": [
      {
        "id": "repair",
        "instructions": "Repair the requested configuration and summarize changed files.",
        "dependencies": [],
        "workspace": ".",
        "access": "write",
        "checks": ["cargo check --locked"],
        "inputs": [],
        "resources": {"cargo-target": 1},
        "timeout_secs": 600,
        "replay_safe": false
      },
      {
        "id": "ui-checks",
        "instructions": "Inspect the UI behavior after repair and report findings.",
        "dependencies": ["repair"],
        "workspace": ".",
        "access": "read",
        "checks": ["cargo nextest run --locked -p vtcode-ui"],
        "resources": {"cargo-target": 1},
        "timeout_secs": 900
      }
    ]
  }
}
```

Every task needs a unique stable `id`, nonempty instructions, workspace-relative directory, `read` or `write`
access, at least one nonempty verification command, and a positive `timeout_secs`. `dependencies`, `inputs`, and
task resource requirements default to empty; `replay_safe` defaults to false. Declare untracked files consumed
by execution or checks in `inputs`, relative to the task's `workspace`. Paths must remain inside the session workspace,
including after symlink resolution. Duplicate IDs, unknown dependencies/resources, cycles, impossible resource
quantities, invalid paths,
and missing checks fail before dispatch.

| Action | Meaning |
| --- | --- |
| `create` | Persist `spec`; launch no workers |
| `start` | Freeze and execute `matrix_id` |
| `status` | Read task, resource-wait, attempt, failure, and verification projections |
| `pause` | Stop new dispatch while current work finishes |
| `resume` | Reconcile durable state and process ownership before dispatch |
| `retry` | Coordinator requests another attempt for `task_id` after a decision |
| `cancel` | Stop owned work and terminally cancel the matrix |
| `report` | Worker submits outcome and canonical `evidence_ids` for its runtime-owned assignment |

Cancellation prevents continuation. A cancelled matrix cannot resume; subsequent execution needs a new matrix.
User input takes precedence over completion notifications. Tracker and agent views project canonical events;
they do not independently own matrix status.

## Resource reservations and workspace access

The worker limit is `min(subagents.max_concurrent, 5)`; the existing default remains three. Ready tasks are
considered in declaration order. The scheduler skips tasks with unavailable resources so independent work can
progress. It reserves a worker slot, workspace lease, and all required named resources atomically before launch.
Failed launches unwind reservations after cleanup is confirmed.

Readers may overlap. A writer excludes every other matrix task across the shared workspace, regardless of the
task's subdirectory. Named resource pools additionally serialize shared Cargo targets, devices, or other declared
resources. This accounting covers scheduler-owned work in one local session. It does not reserve resources used
by another session or unrelated processes.

Workers inherit the session sandbox, approvals, and budgets. The coordinator's narrow tool catalog does not
remove capabilities authorized for workers. Scheduler-issued checks and command polling use the runner's
effective-permission gate as well as registry admission; full-auto grants cannot override an explicit deny.
Delegated children use their own agent specification rather than
inheriting the coordinator primary role. Execution under a read lease requires a read-only worker specification;
a write-capable explorer override is rejected before execution. Resource reservations last until both the worker
and its owned commands stop. Timeout and cancellation terminate owned work; uncertain cleanup blocks resource reuse.

## Execution and final verification

Execution and verification are separate phases. A dependency becomes ready when prerequisite instructions have
finished successfully; this does not claim that final verification has passed. After execution, workers run every
declared check against the final workspace.

The runtime fingerprints tracked source content plus declared untracked inputs before verification, including
file change stamps so restoring content also invalidates earlier checks. Gitlinks include the recorded submodule
commit, initialized submodule HEAD, and tracked working-tree content, including nested submodules. Uninitialized
gitlinks remain valid inventory entries. Declared symlink inputs include both
the link and its resolved file content, with workspace containment checked before reading. Source and declared
input changes invalidate that generation, and v1 reruns the complete final verification phase after owned work
stops. Permission denials, exhausted budgets, and failed checks remain coordinator decisions. Success requires durable,
attempt-owned evidence that the exact declared commands completed successfully against the current generation.
Worker prose, a child's `Completed` status, unrelated exit-zero commands, missing checks, cancelled commands,
and evidence from an earlier generation cannot establish success.

`report` does not let a worker select another task or attempt. Assignment identity comes from runtime state.
An accepted report ends the execution attempt; verification still runs separately. Evidence references are optional
and must be canonical event IDs, rather than command run IDs or invented references.
Duplicate or stale reports are rejected or reconciled against the recorded attempt, and results are persisted
before completion becomes visible. Reopening the canonical sink retains command-event identities, so replaying
checkpoints does not publish historical checks as newly executed commands.

## Restart reconciliation and retries

Assignments are persisted before launch through the acknowledged event drain. Results are persisted before
completion publication. Canonical matrix checkpoints survive event-log cap rewrites so explicit session resume
can replay the lifecycle rather than guessing from summaries.

Resume reconciles durable results and process ownership before replacements are dispatched. A PID alone is
insufficient proof of ownership. Ambiguous surviving work fails closed: resources remain blocked pending a
coordinator decision or confirmed cleanup. There is no background restart service or remote worker support.
Restored active or cleanup-uncertain matrices continue to exclude ordinary discovery delegation, including
when reconciliation cannot resume dispatch. Session bootstrap awaits this reconciliation before exposing
delegation; a failed replay keeps admission closed. Matrix completion wakes the idle interaction loop without
requiring user input, and matrix-only follow-ups do not latch the ordinary background completion queue.

An internal driver failure stops that driver's owned work without turning it into user cancellation. After the
cause is resolved, explicit `resume` or `retry` reconciles durable attempts and cleanup before continuing.
Uncertain cleanup still blocks reuse. Only user cancellation permanently prevents continuation of that matrix.

An interrupted or timed-out task can retry automatically once only when `replay_safe: true` was explicitly
declared and cleanup is confirmed. This flag asserts that repeating the task's effects is safe; it does not bypass
approvals or budgets. Failed checks, permission denials, budget exhaustion, uncertain cleanup, and tasks without
safe replay require coordinator decisions. Explicit `retry` reruns task instructions for repairs, then starts a
fresh complete verification phase; wait for current owned work to stop first. Retrying cannot reuse successful
evidence from a different attempt or workspace generation.

Verification reruns count against the existing tool-loop budget. Exhaustion stops further checks and requires a
coordinator decision; changing a workspace cannot create an unlimited verification loop.

## Development verification

Run the focused acceptance suites with the tracked nextest configuration and confirm that tests matched:

```sh
cargo nextest run --locked --config-file .github/nextest.toml \
  -p vtcode -p vtcode-core -p vtcode-config -p vtcode-memory -p vtcode-llm \
  -p vtcode-exec-events -p vtcode-utility-tool-specs \
  -E 'test(matrix) | test(coordinator) | test(subagent_restart_at_cap) | test(build_child_config) | test(simultaneous_spawns)'
```

| Acceptance area | Regression coverage |
| --- | --- |
| Admission and leases | Concurrent ordinary spawns, the five-worker bound, atomic named resources, exclusive writes, dependencies, and independent progress |
| Durable barriers | Rejected assignment writes launch nothing; rejected launch-intent writes retain prepared work; rejected result writes publish no completion |
| Recovery | Bootstrap blocks discovery before control calls; replay failures fail closed; repaired internal errors can resume; prepared launches roll back and ambiguous survivors block reuse |
| Canonical history | Reopened sinks preserve command identities and timestamps, including after log-cap eviction retains only checkpoints |
| Final verification | Exact owned commands, complete checks, current generations, symlink target changes, submodule HEAD and nested tracked changes, cancelled commands, and rejected unrelated evidence |
| Decisions and permissions | One safe automatic retry, denied unsafe retries, pause/cancel, coordinator restrictions, inherited shell denies despite full-auto grants, allowed-command controls, and exhausted budgets |
| Idle delivery | Matrix readiness returns control to orchestration without ordinary background events; user input wins; matrix-only follow-ups leave later background completion scheduling available |
| End-to-end runtime | Asymmetric mocked tasks with shared resources, a writer, a timeout, and generation changes; real command exit and cleanup behavior |

The persistence-boundary fixture injects failures before acknowledgements and resumes through a fresh controller
using the canonical drain. A lost result acknowledgement deliberately leaves a launched attempt uncertain even
when the test worker stopped: summaries and in-memory observations cannot establish durable cleanup.

After focused tests, run the affected-crate suites, `./scripts/check-dev.sh`, locked warnings-denied checks,
formatting, Markdown, and diff checks. A live-provider exercise must be explicitly bounded by the existing turn,
cost, and timeout settings; deterministic mocked tests remain the repeatable acceptance fixtures.

See the [sub-agent quick reference](../user-guide/subagents.md#matrix-quick-reference) and
[agent loop contract](../guides/agent-loop-contract.md) for the surrounding runtime contracts.
