# Goal loop contract

The `/goal` loop drives one persistent thread objective across turns on both
frontends from shared state. The state machine lives in `GoalTracker`
(`vtcode-memory`); the wire contract is the boxed `goal.updated` event
(`vtcode-exec-events`, schema 0.29.0) carrying `Some(GoalSnapshot)` after a
mutation and `None` on clear; the slash grammar (`GoalSlashCommand`) and the
`goal/execute` RPC live in `vtcode-desktop-protocol`.

## Ownership and persistence

The runloop owns one loop-lived `SharedGoalTracker`, restored from
`<session>/goal/goal.json` at session setup and published through the host
stop-handle slot next to the runloop's own emitter. Restoring demotes any
in-flight goal to paused, so a reloaded goal never resumes driving itself;
clearing removes the snapshot so it can never resurrect. All mutations persist
synchronously and emit through the runloop emitter, keeping the canonical log,
live stream, and desktop restore seeding in agreement.

## Command flow

CLI input parses to `SlashCommandOutcome::GoalCommand` and applies the pure
`apply_goal_command` core: `Set` replaces any goal and submits the objective
as a turn, successful `Resume` submits a continue prompt, and `Pause`, `Clear`,
`Status`, and `Edit` render without starting turns. Desktop executes the same
core through quiet `goal/execute` RPC — never as turn text, which would wedge
awaiting a terminal event a non-turn command never emits. Responses refresh
the composer bar from the returned snapshot, surface read-only output as
transcript notices, and submit follow-up turns normally.

## Surfaces

The CLI prints a one-line indicator (`[i] Pursuing goal: …` / `[i] Paused
goal: …`) at every turn boundary while a goal is active or paused; terminal
states stay quiet because their transitions already printed. The desktop
composer bar renders the same states live above the draft input.

## Turn boundary and auto-continue

Every turn boundary flushes elapsed time, folds session tokens into the
high-water mark, persists, and — while Active — emits for live surfaces. An
Active goal then auto-continues Completed turns through the existing synthetic
follow-up queue under its tracker-owned counter against the shared cross-turn
budget (every Set and successful Resume restores a full budget on all
frontends). A final message whose first line is exactly `GOAL ACHIEVED:`
completes an Active goal on completed or blocked turns — same trust class as
the gate's other final-text classifiers, and a spurious claim completes
visibly and can be re-set in one command.
Completed-only continuation keeps hard budget stops idle; pauses, clears,
stops, pending input, refusals, verification blocks, and planning mode idle
the loop instead. Setting or resuming a goal resets its budget.

## Model tools

Session setup registers `get_goal` (read-only snapshot) and `update_goal`
(`complete` after verifying against evidence, `block` for genuine external
blockers, `pause` only on explicit user request) with the live tracker and
runloop emitter, so model-driven mutations persist and stream exactly like
user-driven ones. Both are `Allow`: session-local, visible, recoverable.
The `GOAL ACHIEVED:` first-line claim remains as a zero-tool fallback.

## Non-goals (v1)

Model-created goals (no `create_goal`; the user sets goals).
