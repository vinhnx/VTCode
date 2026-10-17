# Execution explanations

`/explain` projects retained canonical events without a model call. Terminal output, WebMCP, and offline reports share
`vtcode_memory::explanation::ExplanationModel` and digest-verified evidence references.

| Command | Result |
| --- | --- |
| `/explain` | Latest task, five sections, at most 20 logical lines |
| `/explain --scope session` | All retained tasks |
| `/explain --details` | Public facts in Transcript Review |
| `/explain diagram` | Width-aware recorded execution order and file relationships |
| `/explain --web` | Existing live bridge route, or an offline report when no bridge is active |
| `/explain --export html` | Private standalone report under the session's `derived/explain/` directory |

Summary sections are Goal, Changes, Decisions, Verification, and Review first. Outcome, failures, completeness, and
omitted counts stay visible. Terminal wrapping can increase physical rows. Evidence opens retained event captures in
the existing review surface. In `/explain --details`, Alt+click a wrapped fact to inspect its canonical evidence;
ordinary click and drag retain text selection. Browser navigation uses a typed UI channel and cannot submit prompts or run commands.

## Canonical contract

Event schema `0.17.0` adds optional task/turn/actor context, input origin, timestamps, command activity, and public
decisions. Idle user requests start tasks; corrections, approval handoffs, retries, and continuations retain their root
goal. Legacy records decode without these fields and display uncertainty.

The existing shell classifier supplies command activity. The projection streams retained events and reduces stable
item identities. Lifecycle updates and output aliases do not multiply actions. Completed file-tool results supply file
attribution and captured bounded diffs. Distinct file count differs from edit-operation count. Current Git state is not
proof of agent ownership and is not used for attribution. Detailed, live, and offline views show a separate bounded
current Git diff against HEAD, including staged and unstaged tracked changes. Untracked files are excluded. The
snapshot records its capture time, truncation, and unavailable state. Its read-only runtime adapter disables external
diff, text conversion, clean/process filters, and filesystem monitor commands; capture times out after two seconds.

Verification requires terminal completion and exit code zero. Pending processes or absent exit codes remain
unconfirmed. Later recorded mutation attempts, including pending and failed operations, conservatively invalidate
freshness. Review signals identify inspection priorities, not proven
bugs or coverage: security, permissions, execution, authentication, persistence, schema, and dependencies rank high;
API-like changes, large diffs, repeated failures, and absent fresh verification rank medium.

Optional `record_decision` uses normal tool discovery. Bounds: 240-character summary, 1,000-character public rationale,
three alternatives, and eight current-task canonical item IDs. Runtime supplies identity and timestamps. Evidence IDs
are checked through the ordered canonical persistence queue. Rationale is agent-reported; private reasoning does not
stand in for a recorded decision. Approvals and plan evolution remain separate facts.

## Queries and reports

Ordered request/reply barriers on the blocking persistence actor flush a consistent retained range without closing the
session or blocking rendering. Queries bypass the live replay buffer. Projections share a bounded in-memory cache
(two scopes, each capped at 32 MiB of serialized facts); canonical appends invalidate it. The blocking actor creates
browser pages from shared projections without copying the full model for every page. Oversized projections remain
uncached. No explanation database is persisted.

Evidence references contain session, offset, length, and digest. Changed or evicted bytes expire references. Malformed,
unknown, legacy, evicted, incomplete diffs, and unavailable evidence are distinct from no activity.

Authenticated WebMCP operations are `explanation.get`, `explanation.evidence`, and `explanation.navigate`. Existing
adapters default to unsupported; status advertises `explanations_available`. Collections and evidence are paged under
existing bridge byte limits. Reconnects re-query canonical state. Reports embed redacted evidence, escape untrusted
text, and use no network dependencies. Writes use private symlink-safe helpers. Browser-opening failure leaves a usable
artifact path. Timing, ancestry, usage, and cost appear only when recorded.
Native delegation results preserve public status, child session, and parent identity as `DelegatedAgentStatus` canonical
harness observations before tool-output compaction. Reports provide readable timeline, plan, decisions, checks, review
signals, ancestry, usage, native SVG relationships, and local evidence links without scripts or network requests.

## Validation

Focused `cargo nextest run --locked` coverage exercises task boundaries, continuations, duplicate lifecycles, failed
and no-op edits, stale verification, per-file ranking, ordered barriers, expired evidence, navigation without prompts,
legacy input, redaction, hostile HTML, and 10,000 retained events. Run affected Rust suites and the WebMCP app's tests,
typecheck, and build, then the development gate, formatting, warnings-denied checks, and `git diff --check`.
