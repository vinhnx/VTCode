<!-- markdownlint-disable MD013 -->

# Agent-loop/tool recovery investigation, 2026-10-06

Session: `.vtcode/sessions/session-vtcode-20261006T035519Z_302954-43619/events.jsonl`.
The worktree was clean before implementation. No README or owner TODO changes are part of this fix.

## Reconstructed sequence

1. Events 253-257: user approves the README plan; implementation begins.
2. Events 271, 279, and 444: baseline/check commands use pipes, redirects, and semicolon joins. These do not expose trustworthy verifier status. The ordinary execution turn later exhausts its 600-second wall-clock budget (636-640). That budget remains unchanged.
3. Event 644: an internal tracker continuation starts with pending verification retained. A lint plus `; echo` at 693 also masks lint status.
4. Events 719-721: README pagination crosses its six-read cap. Events 739-745 and 779-781 retry the exhausted path, while other calls succeed in between.
5. Events 766 and 814: `python3 scripts/check_markdown.py 2>&1 | grep ...` and `| grep ... | head -20` run as mutation-classified verifier attempts. Their exit status belongs to grep/head, so they neither clear verification nor grant the normal failed-verifier repair window. Event 796 shows the still-pending gate.
6. Events 821-831: two more capped reads push the blocked total to 7/6, despite a consecutive streak of only 2/3. The remaining Git diff is skipped. `turn_1620.json`, conversation items 230 and 233, preserve these exact counters and the tool-free recovery directive. Event 832 is the quoted incomplete recovery summary.
7. Events 838-895: user `continue` starts a fresh turn and reads current README content.
8. Events 898-900: apply_patch incorrectly expects `MCP Servers` rows with `docs/development/...` links; the already-read file instead has `[MCP](./docs/guides/mcp-integration.md)` rows. Exact context rejection is correct and atomic. The error is typed ContextMismatch, recoverable, non-retryable without changed input.
9. Events 904-913: one fresh bounded sed read succeeds, and a corrected table-header patch applies. Events 919-930 show a later read-cap rejection followed by a successful targeted deletion patch from retained context. The mismatch recovery already worked in this session; the new fix closes its previously uncovered identical-read-cap boundary.
10. The later turn reaches the ordinary 32-call work budget (1190). Further continuation and a final scoped README commit occur later in the archive. These safety budgets are not raised.

Trajectory corroboration: `.vtcode/logs/trajectory.jsonl:141` lists eager apply_patch in the wire tool catalog; line 472 records the failed README patch. No evidence of routing to the wrong tool was found.

## Findings and changes

| problem | evidence/session event or file:line | regression commit if found | fix | verification |
| --- | --- | --- | --- | --- |
| A path-local read limit escalated into the global total fuse despite productive intervening calls | Events 721, 743, 745, 781, 827, 829; checkpoint turn_1620 items 230/233; read_guard.rs:469 | 18e516a620 made path-cap rejection local but left generic total-fuse accounting; the fuse itself predates that change | Typed ReadCapBlocked charges the existing consecutive fuse, without accumulating global total-fuse debt. All capped reads remain rejected, and uninterrupted retry loops still stop. | Runtime continuation and consecutive-retry regression; existing blocked-call/fuse tests |
| Filter status hid the verifier result and repair opportunity | Events 766, 796, 814, 818; tool_intent/activity.rs:622 and command_policy.rs:137 | c1aec0d32 introduced conservative compound-status classification; dca67aeb3 added Markdown verifier recognition but retained the filter limitation. This is an older gap, not an identified new performance regression. | Static verifier pipelines with independently read-only tails retain their filtering under fail-closed pipefail. Terminal 0 verifies all stages; checker/filter failure grants the existing bounded repair window. Pure truncator pipelines retain their standalone rewrite. | Actual checker exits 7/0, grep no-match 1 and error 2; runloop failure-to-success continuation; parser and command-policy negatives |
| Fresh patch recovery could be intercepted by the identical-slice guard | read_guard.rs:429: recovery was reserved only after the family guard before this fix; observed mismatch at event 900 correctly recovered at 908/913 | 37e11dfaf introduced the one-read allowance only at the path cap | Reserve the existing one-read canonical-path allowance before either read cap. Both counters advance; repeated mismatches/aliases cannot replenish it. Rebuild from current complete lines instead of replaying the failed patch. | Cached stale-content refresh at both caps followed by successful mutation; batch admission, alias, symlink escape, and effective-range tests |
| Tool routing suspected | Wire catalog trajectory line 141; apply_patch invocation 898 and correctly typed failure 900 | No routing regression established in reviewed execution-facade extraction commits | Preserve canonical tool routing, direct patch surface, permissions, sandbox checks, and atomic exact-context matching. Add idempotent normalization at the shared pipe/PTY preparation entrypoint (registry/executors.rs:314) so direct registry calls also preserve truthful status. | Existing patch/public routing and execution-kernel tests; real direct-registry checker/filter exit assertions |

The pipefail setup is a narrowly recognized status-only shell builtin, not a general command-policy exception. Every executable stage still needs an allow match; explicit deny rules also evaluate the setup itself. Dynamic syntax, mutating tails, unsafe redirections, backgrounding, semicolon joins, and OR joins cannot acquire verified status.

Reviewed recent relevant history: 0703c6abd, 1df35c9a9, 1388169fa, c59a0a38a, da07a9e3d, dca67aeb3, 37bbc3181, 37e11dfaf, 18e516a620, d63ada473, and c1aec0d32. No new regression was established in the latest performance or routing extractions. Commit attribution above identifies the source of each older behavior or incomplete recovery boundary, not a proven deployment hash for the archived binary.

## Validation

Initial focused run: 16 passed, 2 failed. The real execution case exposed the inserted shell setup needing transparent status-only policy handling; the fuse assertion needed to recognize active recovery after the pending flag was drained. Both were corrected, with additional negative policy coverage.
Second focused run: 25 passed, 7592 excluded.
Broad affected suite: 734 passed, 6884 excluded, no failures after fixes stabilized. This covers tool outcomes, tool intent, command policy, execution normalization, patch recovery, apply_patch routing, and compiled prompt/golden/size tests. The broad run exposed one obsolete filter expectation, the direct-registry normalization gap, and prompt-size pressure; the expectation was updated, the shared launch entrypoint fixed, and guidance shortened without raising prompt budgets.

The initial implementation's affected-suite command was:

```sh
cargo nextest run --locked -p vtcode-core -p vtcode --no-fail-fast --status-level fail -E 'test(tool_outcomes) | test(tools::tool_intent) | test(tools::command_policy) | test(tools::registry::execution_kernel) | test(tools::registry::patch_recovery) | test(apply_patch) | test(filtered_verifier) | test(prompts::system) | test(prompts::runtime_guidance) | test(session_tool_catalog) | test(piped_verifier)'
```

Changed Markdown files and this report: pinned markdownlint-cli2 0.23.3 passed. git diff --check passed.
`./scripts/check-dev.sh --quiet` passed formatting, all-targets/all-features Clippy with warnings denied, compilation, and shell lint. The script did not run tests; the separate affected nextest suite above did.
Warnings-denied locked compilation passed: `env RUSTFLAGS="-D warnings" cargo check --locked -p vtcode -p vtcode-core`.
Windows and non-POSIX shell execution have not been exercised here; unsupported pipefail shells fail closed. This is an integration-test reproduction of the loop/tool boundaries, not a rerun of the archived live provider session.

## Follow-up guideline review

Applied the Rust, code-review, risk-first testing, and module-guidance audit skills. Reviewed the complete diff, shared shell parser, command construction, permission policy, read-cap admission/finalization, and patch-recovery ownership. Two confirmed issues were found in the new verifier handling, both high severity because they could establish verification from a different or unsafe invocation:

| severity | confirmed issue and evidence | fix | regression coverage |
| --- | --- | --- | --- |
| High | Updating only `command` left `raw_command` overriding the rewrite in `prepare_exec_command`. The real registry reproduction reported exit 0 for a checker that exited 7. | Keep a matching raw alias synchronized with the rewritten command; conflicting aliases cannot acquire verifier status. Preflight and launch now call the same argument normalizer. | Actual registry checker exits 7/0 with and without the raw alias; model-dispatch failure/repair/success with the alias; conflicting-alias admission and clearance negatives. |
| High | Validation omitted separate `args`; `cargo check \| sort` with `args: ["-o", "changed.txt"]` could be treated as verification or a read. A failed complete-sequence proof also fell back to the original read-only intent. | Share complete command extraction across read-only and verifier classification, validate the quoted suffix, include it exactly once in rewrites, and fail closed after an unproved shell sequence. | Safe appended filter/truncator arguments, mutating suffixes with/without an explicit pipefail prefix, malformed args, idempotence, planning admission, read-only intent, and preservation of the pending gate even for a claimed exit 0. |

The new text rewrite is private; the existing public argument normalizer remains the shared API. Unchanged normalization retains the borrowed path without another payload clone. The read-cap continuation test uses structured reads, and the POSIX checker/filter execution test is Unix-only. Stale return-value documentation, verifier test naming, and a grammar typo were corrected. No new production dependencies, safety-limit changes, or prompt-budget increases were introduced. Both affected module guidance files remain accurate and under 30 lines.

Reproduction before correction: 3 tests failed, including the actual masked exit. First correction run: 5 passed, 1 failed; the remaining failure exposed the read-only fallback. After fixing that boundary, the expanded registry/runloop/prompt suite passed 1167 tests, with 6454 excluded, including after API/allocation/test-portability cleanup. The same complete command extraction now also gates planning, caching, and parallel admission. The new planning test initially failed compilation due to a missing parent-module qualification; that test reference was corrected.

Final expanded suite: **1168 passed, 6454 excluded**, no failures. Command:

```sh
cargo nextest run --locked -p vtcode-core -p vtcode --no-fail-fast --status-level fail -E 'test(tool_outcomes) | test(tools::tool_intent) | test(tools::command_args) | test(tools::command_policy) | test(tools::registry) | test(prompts::system) | test(prompts::runtime_guidance) | test(session_tool_catalog) | test(piped_verifier)'
```

Final `./scripts/check-dev.sh --quiet` passed formatting, all-targets/all-features Clippy with warnings denied, compilation, and shell lint. Final `env RUSTFLAGS="-D warnings" cargo check --locked -p vtcode -p vtcode-core` passed. Changed Markdown and diff checks passed. Windows/non-POSIX execution and a live-provider session replay remain outside this validation scope.

## Revalidation, 2026-10-09

Reconstructed the archive and checkpoint again against checkout `527717af1` (0.175.1).
The production fix is already present in `6b4886dd9`; no additional production changes or duplicate regressions were needed.
Reviewed subsequent diffs in the read guards, patch recovery, verifier classification, execution kernel/facade, and launch preparation. No reintroduction of the reported failure was found.

The exact checkpoint evidence remains `.vtcode/checkpoints/turn_1620.json`, conversation indices 230 and 233: the total fuse tripped at 7/6 with a consecutive streak of only 2/3. Events 766 and 814 used filtering pipelines, while event 693 used a status-masking semicolon/echo tail. The checker reported real README lint failures; safe pipeline handling must preserve those failures and grant the bounded repair window, rather than falsely clear verification.

The patch at event 898 supplied rows absent from the file. Its atomic context rejection at 900 was correct. Events 908/913 prove fresh-read-to-corrected-patch recovery, and 919/930 prove an edit from retained context after a read-cap rejection. The session later reports scoped verification and commits the README work as `b85747827` (event 1350). The quoted stop was an intermediate blocked pass, not the final state of the archive.

Fresh checks:

- Focused locked nextest: **32 passed, 7,691 excluded, zero failures**. Covers path caps, intermittent versus consecutive rejections, stale cached context, recovery-read reservation at both caps, checker/filter exit status, and successful loop continuation.
- Broader locked nextest: **1,246 passed, 6,477 excluded, zero failures**. Covers tool outcomes, tool intent, command arguments/policy, registry/routing, apply_patch, and compiled prompt presence/budget tests.
- `./scripts/check-dev.sh --quiet`: passed formatting, all-targets/all-features Clippy with warnings denied, compilation, and shell lint. Tests ran separately above.
- `env RUSTFLAGS="-D warnings" cargo check --locked -p vtcode -p vtcode-core`: passed.
- Pinned markdownlint-cli2 0.23.3 on this report and `git diff --check`: passed.

```sh
cargo nextest run --locked -p vtcode -p vtcode-core --no-fail-fast --status-level fail -E 'test(intermittent_path_cap) | test(filtered_verifier) | test(piped_verifier) | test(patch_context_mismatch) | test(patch_recovery) | test(repeated_paginated_sed) | test(blocked_tool) | test(verifier_pipeline)'
cargo nextest run --locked -p vtcode -p vtcode-core --no-fail-fast --status-level fail -E 'test(tool_outcomes) | test(tools::tool_intent) | test(tools::command_args) | test(tools::command_policy) | test(tools::registry) | test(apply_patch) | test(prompts::system) | test(prompts::runtime_guidance) | test(session_tool_catalog) | test(piped_verifier)'
```

Limits, permissions, containment, exact patch matching, and terminal-success requirements are unchanged. The archived executable's deployment SHA remains unknown. These checks reproduce the tooling/runloop boundaries locally; they do not establish live-provider convergence or Windows/non-POSIX behavior.
