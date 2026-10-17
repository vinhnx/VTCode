# PLAN Loop Engineering

> Loop engineering = running the VT Code harness repeatedly, with isolation and durable state, so long-horizon work
> makes monotonic progress.

This document describes the loop-engineering substrate that lets a _plan_ (generated once) drive many _execution steps_
without losing context or colliding with itself. It pairs with
[`agent-capability-composition.md`](../guides/agent-capability-composition.md): a loop is where the "keep getting closer
to completion" invariant is exercised at scale.

## Principles

1. **Worktree isolation.** Each spawned sub-agent with `isolation = "worktree"` gets its own git worktree under
   `.vtcode/worktrees/`. File mutations stay in the child's working tree until explicitly merged, so parallel loop runs
   never collide on the working tree.
2. **Propose / verify separation.** `SubagentController::verify_proposed_change()` spawns a read-only verifier that
   re-reads affected files and approves or rejects the change. The verifier shares no context with the proposer.
3. **Loop state persistence.** `crates/codegen/vtcode-core/src/loop_state.rs` captures the durable state a loop
   scheduler reads on resume: current step index, last artifact path, and status. State lives under
   `.vtcode/state/loop-{id}.json`.
4. **Cost guardrails.** Long loops accrue spend; `SessionBudget` (`crates/codegen/vtcode-core/src/llm/usage_cost.rs`)
   pauses or escalates at thresholds so a loop cannot run unbounded.
5. **Progress over time.** The `ProgressLedger` (`crates/codegen/vtcode-memory/src/progress.rs`) and `ProgressMonitor`
   (`crates/codegen/vtcode-core/src/core/agent/progress_monitor.rs`) give the loop an external, compaction-safe signal
   of goal progress and detect stalls.
6. **Evidence-bounded self-improvement.** Evaluator observations are bounded task-scoped notes with a claim, scope,
   evidence, and falsifier. Replanning must preserve each scope and add each falsifier to the tracker; notes are not
   promoted into global beliefs or persistent memory automatically.

The guidance follows Bennett,
[_The Optimal Choice of Hypothesis Is the Weakest, Not the Shortest_](https://arxiv.org/abs/2301.12987): choose the
weakest sufficient hypothesis, preserve its scope, and keep a concrete falsifier close to the evidence. A local
observation should not become a general rule merely because it is concise or reusable.

## Lifecycle

```text
plan ──► loop scheduler ──► spawn child (worktree)
                        └──► child proposes change
                              └──► verifier approves/rejects
                                    └──► merge or retry
                                          └──► record progress, next step
```

The scheduler reads `LoopRunState` on resume to know where execution left off, and reads the `ProgressLedger` to decide
whether the loop is actually advancing or should escalate.

## Evidence-bounded replanning

The evaluator may emit at most eight notes. Each note is normalized and validated before it is rendered into
`current_evaluation.md`. A replan receives the validated notes, repeats every scope verbatim, and creates tracker
verification steps from the falsifiers. The evaluation artifact and execution contract are the task-local record; the
loop does not write these observations to global beliefs or cross-session memory as a side effect.

## Cross-references

- Harness invariants: [`docs/harness/ARCHITECTURAL_INVARIANTS.md`](../harness/ARCHITECTURAL_INVARIANTS.md)
- Agent loop contract: [`docs/guides/agent-loop-contract.md`](../guides/agent-loop-contract.md)
- Capability composition: [`docs/guides/agent-capability-composition.md`](../guides/agent-capability-composition.md)
