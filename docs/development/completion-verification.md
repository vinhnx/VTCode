# Fresh verification before successful completion

Interactive and headless runs require fresh verification after successful code
mutations, including changes below the six-edit checkpoint. The checkpoint still
controls mutation pacing and retains its bounded repair allowance after failure.
Prose-only and dedicated planning writes, rejected edits, and known no-op diffs
retain their exemptions.
Exemptions check requested paths and the effective targets reported by file
operations, so prose/planning symlinks to code still require verification.
Planning exemptions require every target to be a planning artifact. Build and dependency inputs
such as `CMakeLists.txt` and `requirements.txt` require verification.

Shell accounting exempts an inspection only after a complete syntax-aware proof
that every command, including nested substitutions, is nonmutating. Unknown or
malformed syntax, dynamic executables/options, mutating options, and file writes
retain debt. Descriptor plumbing and literal quoted operator text do not create
debt. Literal `*`/`?` path globs such as `docs/development/*.md` also preserve evidence
when used as operands of the supported read commands (`rg`, `grep`, `egrep`,
`fgrep`, `ls`, `cat`, `head`, `tail`, `wc`). Their literal directory prefix
prevents expansion into options. Character classes, bare globs, variable-based paths,
substitutions, dynamic executable names, mutating options, and output-file redirects retain
debt. This accounting exemption grants no planning or execution permission.
Concrete changed-file evidence always creates shell mutation debt. This
proof affects freshness and mutation pacing only; admission and permissions
remain unchanged.

Verification evidence belongs to runtime state in the existing tool registry,
separate from source inspection. Each verifier has a launch identity, mutation
generation, session identity while running, and observed terminal verdict. A
check launched before a later edit cannot verify that edit. Active mutating
sessions must settle before completion; their settlement invalidates checks run
while they were active. Internal turns and prompt compaction retain runtime
verdicts. Restored history can establish debt, but never restores live ownership
of archived process handles or credits historical passing output as a new verdict.
The newest evidence covering each required checker and its running session identities
are protected from optional-check eviction. Superseded chain variants become optional;
the registry retains up to 32 optional checker entries in addition to pinned entries,
and reapplies that bound when sessions settle or requirements are released.
A newer attempt for the same command supersedes older evidence;
retention never makes a check fresh after another code edit.

Qualifying model-run checks can satisfy completion. Adopted tracker verification
commands remain mandatory; a different passing checker cannot replace them.
Requirements and observed checks match using the execution kernel's pipeline
normalization, while dispatch retains the original required command for policy
checks. Eligible declared custom checks count as verification rather than
invalidating their own verdict as mutations; command admission remains unchanged.
The interactive checkpoint shares this classification, including yielded custom
checks and the bounded repair allowance after failure.
Recovery directives and blocked handoffs select the next missing tracker command
before a configured or detected project verifier. Partial success names the next
required check, so a passing generic build does not cause repeated recovery when
a different command remains unsatisfied. Required commands may run standalone or
as unchanged segments of a statically proven pure `&&` chain. A terminal exit zero
credits those segments; changed arguments, directory changes, dynamic syntax,
mutating tails, pipelines, negation, subshells, `;`, and `||` do not gain segment credit. Nonzero or
running chains remain unverified. Preserve verification metadata when updating task status.
Recognized `npx --loglevel=<level>` options change logging only: a Markdown lint
check remains verification, while `--fix` still counts as a mutation.
The repository's `python3 scripts/check_docs_links.py` is also a verifier, so a
successful link check does not create new mutation debt of its own.
Native terminal adapters may supply empty argv and environment containers.
Nonempty launch context still cannot receive command-only verification credit.
Otherwise the harness uses the existing configured verifier override and project
detection. Disabling tracker continuation does not disable changed-code
verification. An empty verification list never proves changed code verified.

Generated `verify:` commands must exit zero on success. A `grep`/`rg` absence
check returning exit one is useful inspection evidence but cannot satisfy a
mandatory completion requirement; invalid grep options returning exit two also
remain failures. Choose a verifier with a zero-on-success contract when writing
a plan. Test absence assertions with both present and absent fixtures; a reversed
predicate can otherwise make the desired state fail its own required check.
Existing tracker requirements are never repaired or replaced automatically.
Indexed tracker updates require `status`, including updates to `outcome`,
`verify`, or notes; retain the current status when changing only metadata.
Required inspections and custom checks bypass both result-cache boundaries,
including entries cached before tracker adoption, so actual execution supplies
the verdict. Passing current-generation checks remain reusable as verification
evidence without executing them again merely for a completion summary.

A yielded command has no verdict. Interactive and headless verification resume an
owned session covering the requested check in the current mutation generation before
starting another command, including a check inside a qualifying `&&` chain.
It follows that session through existing wait facilities, under the command
execution ceiling,
and checks cancellation between waits. Required commands run sequentially and
stop at the first unsuccessful result. Only observed terminal exit zero can
produce passing evidence. Nonzero exit is a failed check; denial, lost results,
timeout, and cancellation remain unverified. Unavailable sessions are cleaned up
through existing termination/release facilities. Output retained for verification
is bounded. Partial success names the next missing requirement without announcing
a cleared gate or replenishing reminder/cross-turn recovery budgets. Unchanged
tracker updates, repeated summaries, and replay do not restore those budgets.
Exhaustion produces a blocked handoff with the failing command, observed terminal
exit status, and bounded failure tail even below the consecutive-failure limit.

If no verifier is available or recovery budgets are exhausted, changes remain in
place and the run returns the existing blocked/failure outcome with unverified
status. Report the command, observed evidence, and blocker. A passing build
establishes a build result; behavioral correctness still needs an appropriate
regression or exercised user flow. Automatic coverage selection, browser checks,
and fingerprints for external workspace changes are outside this policy.

Source-repair flags such as `cargo clippy --fix`, `ruff check --fix`, `--fix-only`, and suppression-writing options
create mutation debt. Run a separate checker after repairs; a declared tracker requirement cannot turn a repair into
verification. Ruff invocations carrying explicit repair flags remain conservative even when combined with `--diff`.
Requirements are preserved for explicit correction rather than silently rewritten.

## Implementation boundaries

The registry owns freshness and required-check evidence.
`core/agent/runner/verification.rs` owns headless command execution, bounded
waiting, cancellation, and cleanup; its private tests exercise the runner method.
The interactive session's `session_loop_runner/verification_recovery.rs` owns
cross-turn recovery selection and queue admission. The session loop retains
rendering and Exit/continue control flow.
Request projection regressions live in `llm_request/request_builder/tests.rs`;
its private `history_projection.rs` retires expired turn restrictions and older
execution handoffs, and compresses identical older tracker observations. Current
restrictions, user messages, distinct tracker states, and tool-call/result IDs are
preserved. This projection never rewrites durable history.
Verification, checker-session, and recovery suites live under
`tool_outcomes/helpers/tests/`, sharing private parent fixtures.

Interactive tool-bearing responses share bounded verification recovery: repeated
tracker observations, six inspections, or six batches without advancing the next
required check schedule the missing verifier through the existing tool pipeline.
Passing unrelated checks does not reset this progress window. Real edits, changed tracker
state, and advancement to another required command reset the progress window; a harness
attempt leaves a bounded diagnostic/repair window after failure. Existing cancellation,
permissions, failure limits, and execution budgets still apply. Without pending
verification, repeated unchanged tracker polls get one tool-enabled correction;
continued polling ends blocked rather than automatically queuing the same work.

Recovery attempts count only after the follow-up queue accepts them. A full
queue publishes no directive and preserves prior attempts and failure evidence;
it cannot reset the episode or refill recovery budgets. The accepted directive
is appended after queue admission, preserving existing queued user input order.

## Research provenance

The lifecycle distinction was checked against OpenAI Codex at commit
`d292f721242ef355ce5a38da90e0e33edc4790e6`,
[`codex-rs/core/tests/suite/unified_exec.rs`](https://github.com/openai/codex/blob/d292f721242ef355ce5a38da90e0e33edc4790e6/codex-rs/core/tests/suite/unified_exec.rs#L2380).
Its tests distinguish running session identities from terminal exits. This change
uses VT Code's existing execution facilities; no upstream implementation was
copied. Upstream is Apache-2.0, copyright 2025 OpenAI.
Selective retention was also compared with
[`process_manager.rs`](https://github.com/openai/codex/blob/d292f721242ef355ce5a38da90e0e33edc4790e6/codex-rs/core/src/unified_exec/process_manager.rs#L1750)
and its
[`process_manager_tests.rs`](https://github.com/openai/codex/blob/d292f721242ef355ce5a38da90e0e33edc4790e6/codex-rs/core/src/unified_exec/process_manager_tests.rs#L544).
Upstream protects recent process entries during bounded pruning. VT Code applies
selective retention to mandatory verifier evidence under its own completion
policy; no upstream implementation was copied.
[OpenAI's goals guidance](https://developers.openai.com/cookbook/examples/codex/using_goals_in_codex)
supports explicit completion evidence and blocked conditions. These sources inform
the policy and do not establish a measured VT Code reliability improvement.

The conservative syntax proof was compared with
[`codex-rs/shell-command/src/bash.rs`](https://github.com/openai/codex/blob/f0c5077c34ec7a76b4ad9134835925e6ae3d72ec/codex-rs/shell-command/src/bash.rs)
at commit `f0c5077c34ec7a76b4ad9134835925e6ae3d72ec` (Apache-2.0, copyright
2025 OpenAI). VT Code reuses its own command predicates and explicitly walks
supported control flow and substitutions; no upstream code was copied.
The saved `session-vtcode-20261015T142625Z_988722-36010` failure supplied the
inspection loops, stale cached `rg` verdict, and invalid mandatory `grep` cases
for deterministic regressions, following
[OpenAI's trace-to-regression workflow](https://developers.openai.com/cookbook/examples/agents_sdk/agent_improvement_loop).

Session `session-vtcode-20261015T155522Z_593497-38366` supplies the literal-path-glob
regression: `rg -n "MD013|line_length" docs/development/*.md | head` created
spurious debt during README-only work and activated a reversed required check.
The deterministic replay preserves generation zero and does not rewrite that
requirement. Code-debt coverage separately ensures the same inspection preserves
current passing evidence, while adversarial glob and write variants retain debt.
