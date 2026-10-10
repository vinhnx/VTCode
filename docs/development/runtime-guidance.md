# Runtime Guidance and Project Instructions

VT Code has two distinct prompt sources:

| Source                    | Loaded from                                                   | Purpose                                                                                                   | Trust boundary                                     |
| ------------------------- | ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Compiled runtime guidance | `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs`  | Small, universal user-facing behavior included in Default, Minimal, Lightweight, and Specialized profiles | Part of the application runtime                    |
| Project instruction map   | User/workspace `AGENTS.md`, `CLAUDE.md`, and `.vtcode/rules/` | Project conventions, local architecture, and maintainer workflows                                         | User-controlled context, never a security boundary |

The compiled section is deterministic, cached with the static profile, and kept below its approximate 650-token cap. It
must not read, embed, or generate content from repository instruction files. Profile-specific operating details remain
in the prompt builder; correctness-critical behavior belongs in runtime policy, schemas, tests, or lints.

Repository claims must follow observed code and current metadata. Exact versions and capabilities should be omitted
when that evidence is unavailable, rather than filled in from model memory. This rule shares the existing runtime
guidance presence and size tests across prompt profiles; it adds no automatic metadata scans or provider requests.

## Planning contract ownership

The `# PLANNING WORKFLOW (READ-ONLY)` mode section owns the canonical research budget, step-quality, output,
and runtime-owned persistence rules from `prompts/system/constants.rs`. Both interactive and headless composition
retain that section at Default and Minimal tool-guidance densities. Active Tools supplies tool-specific inspection,
read-only, and tracker reminders without repeating the planning output contract. Section replacement recognizes
both `#` and `##` headings, so updating Active Tools preserves the planning contract even without Environment addenda.

Plan markup, validation, persistence, and approval controls continue to use the existing runtime contract.

## User-facing progress contract

The compiled guidance tells the model that its text between tool calls is what the user reads. It says in one sentence
what it will do before starting, updates only on findings, direction changes, or blockers, and finishes with the outcome
first, then what changed, what it checked, and anything the user must do. Structured tool-call events remain the
authoritative status signal. These are user-facing updates, not a transcript of every call.

The UI reports runtime phases automatically. Compiled guidance tells the model to avoid echoing those labels or
inventing percentages, leaving its updates for findings and decisions. The runtime guidance presence and size tests
cover this rule in every prompt profile.

Compact transcript mode may collapse successful command bodies while retaining complete output in Transcript Review. The
model must not rerun commands merely to reveal hidden output; material findings belong in a visible progress update or
the final reply. The provider-neutral collapsed-output disclosure reinforces this after each affected tool result.

Empty searches are evidence of absence within the queried scope, so retries should ask a new question or change scope.
For optional tooling, inspect the declared project/CI commands and check availability once; report an unavailable check
as skipped and continue. A failure from an available checker still needs to be resolved.

Frame work as hypothesis, compare observation, on mismatch inspect evidence and revise hypothesis before retrying;
never retry an unchanged approach. A mismatch (non-zero exit, empty search, patch context-mismatch, verifier fail,
repeated evidence) requires a targeted re-read or spool page before the next mutation.

Verify edits in place. When a checker fails and a baseline comparison is useful, read the baseline files separately
instead of stashing and restoring workspace edits. Report the failing check and baseline comparison separately; an
ordinary filtering pipeline's exit status does not establish verifier success. VT Code enables fail-closed `pipefail`
for static verifier pipelines whose remaining stages are independently read-only. Filtering is preserved; only a
terminal exit 0 for the whole pipeline clears verification. No matches, filter errors, or SIGPIPE keep the gate pending.
Pure `head`/`tail` pipelines retain the standalone verifier rewrite. Dynamic syntax, mutating tails, and `;`/`||` joins
do not qualify, and all normal command safety, permissions, and budgets still apply.
Normalization includes separate `args` and keeps a matching `raw_command` alias synchronized. Conflicting command
aliases cannot establish verifier success; run the intended verifier with one consistent command spelling.

Run checkers standalone, cap previews with `max_output_tokens`, and use the structured exit code. Appending
`; echo $?` masks the checker status and does not establish verification. A completed command containing a
recognizable checker in an unverified shell sequence gets one diagnostic per turn, even for docs-only work
with no pending gate. This feedback grants no permissions or repair edits and never clears or arms verification;
normal mutation accounting still applies. Running calls retain diagnostic-only session identity; only matching
terminal polling results queue the notice. Session cleanup or loss discards that identity without granting repair
edits. Rejected, cancelled, and still-running calls do not produce this notice.

Navigation compares complete, positioned text returned by `code_search` and simple `sed -n` ranges, including
spool pages. Different queries or ranges that return only previously seen lines count toward the existing
low-signal recovery guards. A new line, changed content, or a missing page remains productive; truncated,
unpositioned, running, failed, and cancelled results cannot establish repeated evidence. Shell ranges require a
terminal exit code of zero before their output is fingerprinted. The bounded fingerprint ledger
is progress tracking, not a cache or a permission decision. Successful workspace edits or context compaction
invalidate it.

After six complete searches concentrate on one file, one coaching notice encourages the next concrete action.
Distinct queries stay available and this notice does not count them as redundant. Proven repeated evidence gets
its own once-per-turn notice after the tool response, allowing edits and verification before the unchanged
recovery guards intervene. Neither notice grants verification credit or repair edits. Reuse context at a cap;
do not copy the same file to another path to evade it.

If tool-free synthesis fails after a call, loop, or wall-clock budget is exhausted, the final handoff and blocked
outcome retain the exact budget cause and limit, including failed planning synthesis. Best-effort prose,
planning state, and gathered evidence remain
available for continuation. A successful model synthesis keeps its normal completion status; this diagnostic
does not widen any budget, grant permissions, or clear verification.

Proven standalone grep/rg no-match results appear as completed empty searches, retaining exit code 1 and
their deterministic diagnosis. Invalid arguments, diagnostics, truncated output, and ambiguous shell sequences
remain failures. This presentation does not change execution success, retry accounting, or verification credit.
Streamed calls closed before dispatch retain terminal lifecycle records with a cancelled invocation outcome;
timeline output labels them `not_executed`. Their closure records are distinct from executed failures, and
cancelled verification remains unverified. Existing session archives remain unchanged.

A per-file read cap rejects that path while other reads, edits, and verification remain available. Intermittent
path-cap rejections do not consume the global blocked-call total; consecutive retries still trip the existing fuse.
After a typed patch context mismatch, one bounded uncached read per canonical path per user turn can pass either
read cap. Rebuild the patch from complete current lines; unchanged failed patches must not be retried. Repeated
mismatches and symlink aliases cannot replenish the allowance.

Reuse successful reads and saved checker diagnostics. Read only missing or changed ranges, and rerun checks after
changes or unresolved failures. Standalone `markdownlint-cli2` checks, including explicit `npx` invocations, count as
verification when they do not request `--fix`. The default `python3 scripts/check_markdown.py` invocation also counts;
its `--fix`, `--list`, and help forms do not. Availability probes such as `command -v npx` remain inspection. Unknown
scripts stay subject to planning and execution policy.

Indexed tracker updates send the model checklist totals and the changed item. Explicit list calls retain the complete
checklist; persistence and UI events keep the full original result. This avoids repeatedly sending unchanged task
details.

The navigation tracker counts empty successful read-only search pipelines as low-signal results even when a final filter
such as `head` or `sed` returns exit 0. It uses the existing planning/execution convergence limits; it does not change
command permissions or treat pipeline success as verification. Quiet existence probes, non-empty results, unfinished
captures, and hidden or spooled output do not count as empty evidence.

Standalone grep-style searches with exit 1 and fully visible empty output use deterministic no-match diagnosis without a
model request or diagnosis-budget charge. Compound commands, redirected diagnostics, and hidden output retain the
ordinary failure path because the search's exit status is not established.

Orientation uses topic searches over code, memory, and logs followed by matching read ranges. Repository memory can grow
large; concatenating the full corpus for each task consumes context and forces unnecessary spool reads. This guidance
ships in every prompt profile and does not impose another output quota.

## Continuity and long-running work

### Runtime observability and cancellation

`AgentRuntime` keeps `ThreadEvent` authoritative while tracing bounded diagnostics for model latency, turn latency,
input/output token usage, output and reasoning byte counts, tool-call count, finish reason, cancellation, and timeouts.
These fields are diagnostic only and must not be persisted as a parallel lifecycle schema. A provider error or timeout
fails the turn; an interrupted stream is never reported as successful completion. Open tool calls are closed with a
terminal failed status, while partial streamed text remains partial and is not promoted to a successful final answer.
Follow-up steering received during streaming is applied to the live history at the next turn-loop iteration boundary, or
retained for the next turn when the turn ends first.

Request segment fingerprints include the provider route, model, context capacity, effective reasoning tag,
tool/parallel/cache capabilities, and current tool catalog epoch. Repeated requests reuse immutable prompt and ordered
tool bytes; capability changes invalidate the segment identity. Shell profile, environment, and harness limits belong to
the frozen segment prefix. Provider cache routing keys include the fingerprint where the transport supports explicit
keys. Local fingerprint reuse is not proof of a provider cache hit: use returned usage cache-hit metrics to measure that
separately for each provider.

Tool documentation density is resolved separately from tool authorization. Both request paths use Minimal guidance at
32,000 context tokens or below, or when the Default prompt exceeds the configured system-prompt token or monetary
budget; otherwise they retain Default guidance. Missing pricing does not imply cheap execution. Parallel-call hints
require the active provider's parallel-tool capability. Both profiles preserve inspection, verification, and
terminal-owned WebMCP permission guidance.

The runtime keeps prompt additions small and cache-stable while preserving the newest working context. Automatic
compaction applies the shared configured trigger ratio (90% by default) to the effective provider/session budget and
uses a continuity tail target of approximately 20,000 estimated tokens. It retains complete user/assistant/tool protocol
groups verbatim, removes an incomplete trailing tool call, and summarizes only the older prefix. Unless an explicit
harness threshold is configured, the effective hard threshold is the resolved model capacity, bounded by the provider
route and a positive `context.max_context_tokens` safety ceiling, minus the next request's output reservation. The
default safety ceiling is zero (automatic). Known request output limits take precedence; otherwise 4,096 tokens are
reserved. Explicit thresholds may lower this boundary but cannot bypass it. Prompt and tool overhead count toward
pressure; per-turn tracing records the context denominator. A derived soft boundary marks compaction pending for the
next outer turn boundary; the effective prompt threshold compacts before the next model request. Provider-native
compaction results are normalized through the same tail rules, with local fallback when the provider does not return a
usable tail.

Long-running command sessions have an explicit `wait` action. A wait deadline returns a bounded in-progress result
without killing the process, so the model does not need to spend repeated turns issuing 30-second polls. Full command
output is written to the tool-output spool; responses expose only a bounded preview and its spool metadata. A spool
reference is emitted only after its file is open and has not reported a write failure. Completed references include
`spool_state`, the exact byte count, and a SHA-256 digest and are reopened only after descriptor-relative containment,
regular-file identity, length, and digest validation. Pending live references permit only bounded, explicitly unverified
reads. Exited sessions with an unfinished spool retain the session and defer the reference until a later wait can safely
observe the complete file.

Background execution is owned by `ExecSessionManager`, the single registry for pipe and PTY sessions. `exec_command`
accepts `background: true`, and `Ctrl+B` promotes the active foreground session into the same registry without replacing
its `session_id`. The launch response keeps the bounded preview and provides the lifecycle state, child PID when
available, and reusable wait/continuation arguments. At most three live background processes are reserved per VT Code
runtime; the fourth launch fails before spawning and existing sessions are not evicted. Exited background metadata
remains inspectable until explicit close, while runtime shutdown closes all process groups and descendants. The runtime
continues to emit the existing `ThreadEvent` item lifecycle events rather than introducing a parallel background-process
event contract.

Ordinary pipe launches use null stdin (EOF); `exec_command.stdin: true` explicitly keeps a writable pipe open.
PTY input remains available. This prevents pathless searches such as `rg pattern` from waiting on an unused stdin
pipe. Public `write_stdin` supports bounded `inspect`, process-group `terminate`, and record-releasing `close`
actions as well as write/poll/wait. Cleanup remains available while edits await verification and uses the existing
bounded control-call budget and permission checks.

A running verifier response never clears the verification gate. The turn tracks its session identity and accepts
only its terminal exit status; unrelated session completions cannot verify edits. At the assistant text cap, enabled
autonomous verification runs promptly or waits for the existing verifier instead of spending reminder rounds.
Fresh internal turns reuse an exact matching live verifier owned by the current runtime. Exhausted execution budgets
produce a blocked handoff without announcing a verifier that cannot run. Only observed non-zero verifier exits
consume the consecutive-failure budget; rejected calls and still-running results do not.

Managed background subprocesses publish a terminal completion notification after their `BackgroundRecord` has been
persisted as `Stopped` or `Error`; user-launched background exec sessions publish the same terminal signal directly from
the shared exec-session watcher. Before publishing, the watcher makes a bounded attempt to drain and retain final
output, and pruning retains an exited background session until that delivery finishes; the run loop then refreshes Local
Agents immediately. The local agent state and transcript can therefore show terminal results without a
`/subprocesses refresh` or explicit `write_stdin` poll. Clean exits are `Stopped`, spontaneous non-zero exits are
`Error`, and user-requested managed or raw exec-session stops remain `Stopped`. When the main interaction loop is idle,
it appends one bounded authoritative completion note and schedules at most one follow-up reasoning turn. A completion
that arrives during an active model/tool turn is deferred to the next safe boundary, and newer user input always takes
priority; direct user commands do not fabricate an unrelated autonomous turn. The explicit `wait` action remains
available when a caller needs a synchronous observation. The canonical completion record is emitted as
`background_subprocess_completed` in event schema 0.16.0.

Exec-session lookup failures are typed `ResourceNotFound` errors with the existing debug metadata code
`exec_session_not_found`. They use deterministic recovery without a model-diagnosis call or circuit-breaker charge.
Wait, poll, and inspect propagate missing-session/output errors immediately instead of returning empty success or
spending the full wait deadline. Recover the exact ID from the original response and reuse recorded completion output. A
missing handle does not prove a command failed; rerun only when fresh execution is needed. The pending-verification gate
still requires a successful verifier when its result was lost. If the running verifier's session ID is known, a missing
unrelated session cannot discard that ID or grant verifier repair edits. Session identity uses the same normalization as
the execution tools for completion, cleanup, and loss recovery.

Cross-turn resume hint body is transient, not universal guidance: when a turn ends with a live foreground session, the
next turn start injects a bounded `Exec session resume:` hint via `append_transient_turn_notes` (same path for normal
next-turn and session restore/resume). The hint carries at most 4 session lines (160 bytes per command, <1 KiB
single-session) plus a pre-filled `write_stdin` wait, and the runtime never auto-executes the wait. Turn-end
`turn.completed` (schema 0.16.0) and `SnapshotTurnDiagnostics` both record `in_progress_exec_sessions` (bounded to 4)
for ATIF correlation, including retained background sessions that remain live for asynchronous work. This hint body
stays out of `runtime_guidance.rs` so the universal section is not taxed on turns with no live session; per-tool
`guidelines.rs` `write_stdin` guidance carries only a one-line pointer that the hint may appear.

Provider-facing tool previews are bounded per result (up to 64 KiB execution, 96 KiB planning), with larger output
retained in the spool and session viewer. Each result retains a bounded preview and outcome/control metadata; earlier
output volume never exhausts later visibility or disables tools. History compaction bounds accumulated context. Legacy
preview-exhaustion markers are retained for diagnostic/replay compatibility, without gating new inspections. Diagnostics
also report requested, admitted, and derived unadmitted tool-call counts so budget or policy rejections cannot disappear
from turn accounting. Read-only results reused by same-turn caches, cross-turn target caches, or bounded history replay
all increment the same reuse counter. Request assembly also collapses legacy duplicate output-disclosure notices to one
current marker.

Interactive follow-ups are durable steering intents. Each queued intent has a UUID, the session envelope stores at most
16 pending intents and a 64-ID applied window, and the intent is acknowledged only after its tagged user message is
durably checkpointed. Recovery compares IDs in the envelope with tagged history, not just instruction text, so duplicate
text remains meaningful. Delivery is mid-turn: the tagged user message is appended to live history at the next turn-loop
iteration boundary, right after the current tool-call batch.

Even when `.vtcode/prompts/system.md` replaces the static base prompt, the compiled section is reattached after prompt
layers are resolved. This keeps the universal baseline present without treating workspace prompt content as a security
boundary.

The dynamic instruction pipeline remains enabled by default. It discovers user and workspace sources in precedence
order, loads nested files for the active directory, applies path-scoped rules and exclusions, and appends the resulting
project appendix separately from the compiled base prompt. `AGENTS.md` files therefore remain useful maintainer maps
without becoming an implicit source of universal VT Code behavior.

When changing this boundary, run:

```bash
cargo nextest run -p vtcode-core
cargo check --locked
./scripts/check-dev.sh --changed
```

Release archives are independently allowlisted to contain the binary, man page, and shell completions only. They must
never include `AGENTS.md` or other workspace guidance.

Debug traces emit span completion records with timings instead of repetitive enter/exit records on each poll.
Skill-reference extraction removes Markdown delimiters, ignores external links, and sorts deduplicated validation
errors. PTY decoding handles large valid chunks and retains only an incomplete UTF-8 suffix across reads; chunk size
alone is not a Unicode error.
