scan for large and monolith files and module and plan deduplication and refactor and extract reusable components.

===

prioritized refactor plan (docs/development/refactor-scan-2026-10-04.md) and full Rust file inventory (docs/development/refactor-scan-2026-10-04.csv).

===

Investigate and fix the agent-loop/tooling regression shown in this session:

`/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/sessions/session-vtcode-20261006T035519Z_302954-43619`

Observed failure:

> Reads of README.md exhausted the per-file cap (6) this turn, and a verifier was run behind a filtering pipe so it did not clear the verification gate; retries then tripped the blocked-call fuse. Tools are disabled for this pass.

The agent appears to stop before completing the task after:

- hitting the per-file read cap,
- failing to satisfy the verification gate because the verifier ran through a pipe/filter,
- retrying until the blocked-call fuse disables tools.

Also investigate this `apply_patch` failure:

> Tool 'apply_patch' failed: Execution failed: Patch context mismatch in 'README.md': use complete current lines and preserve internal…

Tasks:

1. Reconstruct the failing agent/tool-call sequence from the session and identify the exact root causes.
2. Check recent commits for regressions affecting the agent loop, read limits, verification gate, blocked-call fuse, `apply_patch`, retry/recovery logic, and tool routing.
3. Fix the underlying behavior rather than only handling this specific session.
4. Ensure reaching a per-file cap cannot leave the agent stuck when it already has enough context to continue.
5. Ensure valid verification commands still satisfy the verification gate when output is piped or filtered, where safe.
6. Improve recovery from `apply_patch` context mismatch. Refresh stale file context or retry with current complete lines instead of repeatedly failing with the same patch.
7. Prevent repeated recoverable failures from unnecessarily tripping the blocked-call fuse.
8. Preserve existing safety limits and avoid weakening protections just to make this session pass.

Keep the fix surgical, KISS, and DRY. Follow existing VT Code conventions.

Verify with focused tests that reproduce:

- per-file read-cap exhaustion,
- verifier commands with pipes/filters,
- stale `apply_patch` context,
- repeated recoverable tool failures,
- successful continuation of the agent loop after recovery.

Also run the relevant existing checks and inspect recent commits/diffs to identify which change introduced the regression.

Report:
`problem | evidence/session event or file:line | regression commit if found | fix | verification`

===

# Targeted Transcript Review Timing Probe

## Summary

Add a temporary, uncommitted test in `crates/codegen/vtcode-ui/src/tui/core_tui/app/session/transcript_review/tests.rs`. Measure open, append refresh, and search costs before choosing an optimization. No public API changes or production dependencies.

## Probe

- Use deterministic Tool captures with unique IDs, short indexed lines, and sparse `alpha` matches. Fix review width/height at 80×24 and use default Rich mode.
- Build fixtures, append captures, clone comparison states, and perform assertions outside timed intervals. Use `Instant`, `black_box`, and `eprintln!`.
- Run one warm-up and three measured repetitions with fresh viewer states; report each sample and median.

| Case | Fixture                              | Measurements                                                                |
| ---- | ------------------------------------ | --------------------------------------------------------------------------- |
| 1    | 64 captures × 200 lines              | `open_focused`; refresh after adding a two-line tail capture                |
| 2    | One capture × 20,000 lines           | `open_focused`                                                              |
| 3    | Case 2 with committed `alpha` search | Append refresh; isolated incremental search versus full `recompute_matches` |

For case 3, preserve the large cached capture and append a new tail capture containing one non-match and one match. Prepare equivalent states immediately after `refresh_messages`; compare incremental and full search with identical prefix lowercase caches. Also report end-to-end refresh separately.

Use direct test fixtures for the appended 65th capture, avoiding FIFO eviction as a timing confound. Assert the intended capture and row counts.

## Attribution and Decision Gates

- Separately measure source collection, all `build_cached_block` calls, and wrapping-only work over the same plain-text payload. Keep these diagnostic passes separate from open timing.
- If case 1 dominates, compare **one × 12,800** against **64 × 200**, using identical total text. Tool-only fixtures cannot establish a Core reflow bottleneck; add a corresponding Core comparison before making that claim.
- Case 2 consistently above **500ms debug**, with wrapping accounting for most of the cost: proceed to a separate increment that lazily wraps only `ReviewSourceKind::Tool`, behind `CachedToolOutputBlock`, retaining the Core path. Re-run this probe plus scroll, search, evidence-link, copy/export, and resize regressions.
- Confirmed block-count overhead: target the measured refresh/source/reflow stage. Do not throttle follow behavior unless repeated refresh work is the demonstrated bottleneck.
- All cases comfortably within budget: make no structural change. Reject this freeze hypothesis; close the lag report only if its original reproduction also succeeds.
- Near-threshold or noisy results: repeat the affected case in release. Do not infer release timing from a fixed debug multiplier.

## Run and Cleanup

```
cargo nextest run --locked -p vtcode-ui --lib \
  -E 'test(transcript_review_timing_probe)' -- --nocapture
```

Assert exact expected search rows and equality between incremental and full results. Timing values are observations, not pass/fail assertions.

Remove only the temporary probe afterward. Check the scoped diff and final worktree status, preserving the existing unrelated edit. Report timings, attribution, and the selected next step; commit nothing.

===

https://developers.openai.com/api/docs/guides/decisions

===

For VT Code, I'd treat this as a desirable agent-loop behavior:
Hypothesis → Observation → Mismatch → Inspect evidence → Revise hypothesis

===

https://github.com/astral-sh/astral-html

===

Switching to codegen-units = 1 made the uv build ~50% faster, cut peak memory by 67%, and reduced binary size by 17%.

I was really surprised by this... Increasing the number of codegen units typically _improves_ build times.

This is amplified by PGO: building the instrumented binary become >90% faster. (The final, non-instrumented build also got faster, but less dramatically so.)

According to Codex, by decreasing to codegen-units = 1, we sacrificed parallelism within each crate, but substantially reduced the total work especially in the fat-LTO step, because we end up with less "intermediate code, profiling data, and retained function bodies to process".

https://github.com/astral-sh/uv/pull/22303
