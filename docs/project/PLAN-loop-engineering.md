# PLAN Loop Engineering

> Loop engineering = running the VT Code harness repeatedly, with isolation and
> durable state, so long-horizon work makes monotonic progress.

This document describes the loop-engineering substrate that lets a *plan*
(generated once) drive many *execution steps* without losing context or
colliding with itself. It pairs with
[`agent-capability-composition.md`](../guides/agent-capability-composition.md):
a loop is where the "keep getting closer to completion" invariant is exercised
at scale.

## Principles

1. **Worktree isolation.** Each spawned sub-agent with `isolation = "worktree"`
   gets its own git worktree under `.vtcode/worktrees/`. File mutations stay in
   the child's working tree until explicitly merged, so parallel loop runs
   never collide on the working tree.
2. **Propose / verify separation.** `SubagentController::verify_proposed_change()`
   spawns a read-only verifier that re-reads affected files and approves or
   rejects the change. The verifier shares no context with the proposer.
3. **Loop state persistence.** `vtcode-core/src/loop_state.rs` captures the
   durable state a loop scheduler reads on resume: current step index, last
   artifact path, and status. State lives under `.vtcode/state/loop-{id}.json`.
4. **Cost guardrails.** Long loops accrue spend; `SessionBudget`
   (`vtcode-core/src/llm/usage_cost.rs`) pauses or escalates at thresholds so a
   loop cannot run unbounded.
5. **Progress over time.** The `ProgressLedger`
   (`vtcode-session-store/src/progress.rs`) and `ProgressMonitor`
   (`vtcode-core/src/core/agent/progress_monitor.rs`) give the loop an external,
   compaction-safe signal of goal progress and detect stalls.

## Lifecycle

```
plan ──► loop scheduler ──► spawn child (worktree)
                        └──► child proposes change
                              └──► verifier approves/rejects
                                    └──► merge or retry
                                          └──► record progress, next step
```

The scheduler reads `LoopRunState` on resume to know where execution left off,
and reads the `ProgressLedger` to decide whether the loop is actually
advancing or should escalate.

## Cross-references

- Harness invariants: [`docs/harness/ARCHITECTURAL_INVARIANTS.md`](../harness/ARCHITECTURAL_INVARIANTS.md)
- Agent loop contract: [`docs/guides/agent-loop-contract.md`](../guides/agent-loop-contract.md)
- Capability composition: [`docs/guides/agent-capability-composition.md`](../guides/agent-capability-composition.md)
