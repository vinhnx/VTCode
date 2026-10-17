# Hypothesis revision validation

VT Code's HOVER loop is **Hypothesize → Observe → Verify → Evaluate → Revise → Repeat**.
Its mismatch contract is **Hypothesis → Observation → Mismatch → Inspect evidence → Revise hypothesis**.
Compiled runtime guidance and bounded diagnosis feedback encourage this behavior; they do not prove compliance.
Inspection can use retained, current evidence. Fresh context is required when it is stale or insufficient.

## Acceptance

| Scenario | Required mismatch | Corrective action and independent outcome |
| --- | --- | --- |
| Nonzero exit | Executed command returns a nonzero exit | Correct the relevant target/arguments using the reported cause; confirm the requested result. |
| Empty search | Fully visible grep-style no-match result | Change scope/question or use the negative result; independently confirm the task outcome. |
| Stale patch | Context mismatch from the patch tool | Obtain current context, submit an exact corrected patch, and check resulting bytes. |
| Failed verifier | Correlated terminal nonzero checker result | Repair the reported cause and obtain a terminal successful checker result. |
| Repeated evidence | Inspection returns only retained text | Reuse current context or change approach; confirm the requested result. |

Identical retries, cosmetic argument changes, rereads without correction, and unsupported success claims do not pass.
Running checkers, rejected calls, unrelated session results, lost results, and cancellation cannot establish verification.
Guidance grants no permissions or additional recovery allowances. Policy rejection and cancellation keep their safe actions.

`score_hypothesis_revision` remains compatible as a count-based heuristic. Rereads alone can score 1.0; its notes explicitly
leave corrective action unverified. Zero observed mismatches are inconclusive for behavioral acceptance.

## Deterministic checks

`src/agent/runloop/unified/turn/turn_loop/hypothesis_revision_tests.rs` uses controlled providers with real tools through
`run_turn_loop`. Assertions inspect model-visible feedback, canonical output-before-diagnosis ordering, resulting file
bytes, and terminal verifier output. Counterexamples check the actual missing outcome despite a scripted success claim.
Diagnosis tests also cover memoization, action bounds, redaction, and safe cancellation/policy actions. Existing tracker
and cancellation tests retain session correlation, recovery limits, and terminal-status coverage.
Native Copilot tool-reply regressions also exercise a real failed checker and overlapping inspections, checking notice
delivery, one-shot retained-evidence guidance, and successful verification cleanup.

```bash
cargo nextest run --locked -p vtcode -p vtcode-core \
  -E 'test(hypothesis_revision) | test(failure_diagnosis) | test(core::agent::hypothesis) | test(core::agent::evaluator)'
cargo nextest run --locked -p vtcode -E 'test(agent::runloop::unified::turn)'
./scripts/check-dev.sh
RUSTFLAGS='-D warnings' cargo check --locked -p vtcode -p vtcode-core
cargo fmt --all -- --check
```

## Live replay pilot

Run one baseline/candidate pair per scenario (ten attempts) through the interactive runloop with the configured
`openai / gpt-6-luna` route. Freeze fixtures, settings, approval handling, and limits across each pair, with a 300-second
deadline per attempt. Keep runs isolated from the development workspace.

Record source revision and dirty-diff provenance, binary and fixture hashes, effective configuration, canonical events,
usage, and terminal outcomes. Grade **pass**, **fail**, or **inconclusive**. Passing requires the mismatch, a subsequent
evidence-supported corrective action, and an independent outcome check. Missing credentials/provider failures leave
validation incomplete; an unexercised mismatch is inconclusive. Report each case separately. One pair per scenario
supports case-level findings only and cannot establish repeatable improvement.
