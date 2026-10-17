# Source inspection before editing

VT Code recommends inspecting relevant current source before editing, including
documentation, callers, tests, and conventions when needed. Inspection is advisory
by default; matching cannot prove understanding. Use a bounded edit, refresh after
a mismatch or stale evidence, then verify. Clearing or compaction alone does not
require rereading every touched file in advisory mode.

```toml
[agent.harness.inspection]
mode = "warn" # default; "enforce" blocks missing exposure, "off" disables this guard
```

The six-mutation **Verification checkpoint** remains independent. Successful
completion after any code change also requires [fresh verification](completion-verification.md). Run an
appropriate build, test, lint, or documentation check after a bounded change.
In strict mode, passing verification never authorizes an unseen edit, including
a repair after a failed verifier. Advisory mode permits missing exposure without
changing the independent verification, sandbox, approval, or command policies.

## Workflow and quick reference

| Operation | Strict-mode required inspection |
| --- | --- |
| Successful completion after code changes | Fresh terminal verifier exit zero after the last edit; required tracker checks pass |
| Local patch | Original context/removal lines and resolved anchor |
| Insertion or append | Adjacent source at the insertion boundary |
| Literal edit | Source selected by the replacement matcher |
| Whole-file overwrite or delete | Complete existing text |
| New file | Runtime confirmation that the target is absent |
| Pure move/copy | Valid source/destination identities; full inspection of overwritten destinations |
| Cleared, summarized, resumed, externally changed, or evicted source | One targeted refresh of the required ranges |

Read results, positioned `code_search` snippets, and supported single-file verbatim
shell reads (`cat file`, `sed -n 'start,endp' file`) can provide evidence. Only
complete returned source lines count. Filenames, summaries, truncation markers,
internal filesystem reads, and source inside a proposed patch do not.

The bounded session-local ledger stores canonical paths, content fingerprints,
line coverage, and retained read/patch dependencies outside the prompt. Successful
structured edits map surviving coverage and credit authored text. They do not
expose the rest of the file. Clearing a supporting source result or patch removes
its dependent authorization. A resumed session starts without this ledger;
summary text cannot recreate it. Cached read reuse can establish fresh exposure
only when returned lines still match current source and the new call/result pair
is retained. Cleared-result stubs never preserve mutation support, even when
clearing retains tool arguments.

Enforcement reconciles the ledger against normalized request history after local
result clearing. Hard inspection uses local clearing even on providers offering
native tool-result clearing. Canonical session history remains unchanged.

A rejected edit returns `source_inspection_required`, the path, reason, missing
range, and bounded `next_read_args`. This shares the existing canonical-path
stale-patch recovery allowance: one uncached read of at most 200 lines per path
and user turn, even at the existing read cap. Aliases and repeated rejections do
not refill the allowance or increase ordinary limits.

Advisory inspection logs unavailable source (including binary text decoding)
without blocking an otherwise authorized delete, move, or copy. Strict mode
requires readable retained text for existing files. A patch deletion of an
already-absent file remains a successful skipped deletion in every mode.
External-change conflicts are checked under mutation leases before disk-based
patch matching or preview planning, preserving the conflict resolution workflow.
Successful native transfers and deletions refresh tracked descendants in the
conflict monitor, so the agent's own writes do not appear as external changes.
Advisory recursive deletion can unlink symlinks without following them. Recursive
copy still resolves every source and destination entry inside the workspace.

## Shell commands and host integrations

Use guarded file tools for authored edits. In strict mode, unclassified potential
shell writes, in-place editors, interpreter code, and mutating verifier flags are
rejected; advisory mode logs inspection warnings.
A verifier label does not authorize file-writing redirects, explicit compiler or
report output paths, or inline interpreter code. Python and `uv` verification
classification follows the actual execution target, not later argument text.
Safe stream plumbing such as `2>&1` remains supported.
Ordinary command approval does not exempt them from inspection. Existing execution
policy and sandbox checks still apply.

To run a formatter or generator yourself, submit the complete command through
shell mode, for example `!cargo fmt --all`. This trusted user-input route creates
an exact single-use inspection exemption. Approving a model-authored shell call
does not create one. Fallback calls do not inherit it.

An embedding host may scope an explicitly selected mechanical command with
`ToolRegistry::trusted_user_command_exemption(&args)`, then dispatch those
arguments through the normal tool pipeline. The returned guard removes an
unconsumed exemption on drop. The exemption binds the complete normalized
invocation and working directory; model tool arguments and ordinary approval
cannot create it. Existing execution policy and sandbox checks still apply.

## Validation

Focused deterministic regressions live in `tools::source_inspection::tests`.
They exercise authorization and actual file contents independently. Fixture read
counts, source token estimates, and elapsed time are local evidence; they are not
live-provider performance results.

## Literal matching and mismatch recovery

`edit_file` selects a unique exact occurrence first. If none exists, it accepts a
unique sequence of complete lines matching after trimming line-edge whitespace.
Internal whitespace and Unicode punctuation remain significant. Duplicate matches
at the selected level are rejected; use anchored `apply_patch` to disambiguate.
The same byte and line range drives inspection, mutation, and coverage mapping.
Only the selected bytes change; unrelated line endings and final-newline state
are preserved. Whitespace fallback uses the selected region's CRLF convention.
Exact replacement uses the supplied replacement bytes. A no-op is a valid match.
Successful literal edits report modified paths to runtime mutation tracking;
conflicts and no-ops do not report a changed path.

Literal errors include optional `edit_failure` diagnostics with `no_match` or
`ambiguous_match`, a canonical path, and at most five candidate line ranges.
Reliable candidates provide small surrounding `next_read_args`. Without a match,
`next_search_args` scopes a literal search to the path; read a targeted range at a
returned location. Errors include no unrelated head/tail source previews. Literal
and patch mismatches share one uncached read of at most 200 lines per canonical
path per user turn; repeated failures and aliases cannot refill it. No automatic
read, rewrite, or unchanged retry occurs. Full persisted tool results remain
separate from the bounded model-facing context.

The exact-first approach follows the inspected Codex
[matcher at f87ba04ec3cd386a92e775405b252a310b40bb0d](https://github.com/openai/codex/blob/f87ba04ec3cd386a92e775405b252a310b40bb0d/codex-rs/apply-patch/src/seek_sequence.rs)
(Apache-2.0, copyright 2025 OpenAI). The literal matcher is independently
implemented and stricter about ambiguity and internal whitespace. Existing
`apply_patch` matching, mutation leases, conflict checks, and rollback remain
independent of this literal matching policy.
