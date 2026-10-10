# Agent Loop Contract

VT Code keeps its existing harness-first runtime, but its external loop contract now lines up more closely with
SDK-style agent runtimes.

This guide describes the public lifecycle semantics shared by interactive runs, `vtcode exec`, harness logs, and Open
Responses extension events.

## Message and Event Mapping

VT Code does not expose Claude-specific SDK structs. The canonical stream stays `vtcode_exec_events::ThreadEvent`.

### Tracker-aware and blocked-end auto-continuation

The harness continues instead of ending the turn and nudging the user to resume in two cases: when `task_tracker` still
has incomplete steps, and when a turn ends on a recoverable block — in every mode, even with an empty or absent tracker:

- Tracker scope: a workspace checklist drives continuation only after a successful tracker create/update/add for the
  current request, explicit continuation, or approved-plan handoff. Ordinary inspections and work-intent words
  do not adopt an unrelated checklist. A new user request resets this adoption boundary; internal follow-ups retain it.
  Successful adoption is latched when tool results are processed, before output or history compaction, and survives
  internal continuations. The headless runner ignores bootstrap adoption and only owns scaffolds created for its run.
- In-turn: status-only assistant text (including budget/tool-loop/recovery recaps) is forced to continue unless it is a
  true user handoff (trailing question / interview ask) or a hard permission/policy/safety/credentials/
  **verification-pending** handoff. Mid-text `?` and optional-offer closers are not handoffs while tracker work remains.
  Planning remains terminal for tracker in-turn continue. Verification-pending recaps (`verification is still pending` /
  `unverified assistant responses`) are terminal in-turn so continuation cannot race past the anti-blind gate.
- Cross-turn: the session loop queues the next turn automatically in two shapes, bounded by
  `[agent.harness.continuation].cross_turn_turns` (default 32; progress-resets when any tracker step completes). `0`
  disables cross-turn auto-queue. After a **Completed** turn with incomplete tracker steps it queues the next tracker
  implementation turn. After a **recoverable Blocked** turn — with or without tracker steps — it queues a bounded
  blocked-end resume ("The previous turn ended on a recoverable block: …"). Recoverable means the production
  blocked-reason constants only (turn / tool / tool-loop budgets, per-result preview limits, safety caps, blocked-tool
  fuse and tool-call limits, tool-free recovery). Pending verification, including exhaustion of the tool-call budget,
  uses the separate verifier-first recovery path and its verification turn/failure limits, preserving the pending gate.
  A pending-verification recap does not veto that budget-only retry; genuine policy or user-decision handoffs still do.
  Provider refusals, true handoffs, and unknown reasons never auto-queue.
  Successful auto-queue never prints “Type `continue`”.
- Exhausted-path UX: when tracker/plan/blocked-end auto-queue is eligible but cannot resume (queue full or
  `cross_turn_turns` exhausted), the harness prints **one** info line and skips the generic blocked handoff nudge stack
  / blocked TUI placeholder for that recoverable budget end. With incomplete tracker steps the line covers the
  tracker/plan resume; blocked ends without tracker work name the blocked-end resume directly
  (`Blocked-end auto-continue could not resume automatically.` or `Blocked-end auto-continue budget exhausted.`, each
  ending “Type `continue` to retry the request.”). True handoffs, verification escalation, and unknown blocked reasons
  still use the normal blocked handoff.
- Resume: restored sessions use the same current-request adoption gate before probing workspace tracker state or
  auto-queuing one continuation turn. Restoring an informational session does not adopt an unrelated checklist.
- Plan mode: while planning is active and no validated plan is ready for approval, **recoverable blocked** planning ends
  (budget / safety-cap / tool-free recovery, including blocked-tool fuse trips and turns ending without a
  harness-visible final assistant response) auto-queue another planning turn (same `cross_turn_turns` budget).
  Deterministic empty-turn fallbacks cannot self-loop: after 2 consecutive empty fallback turns the gate closes and the
  user must `continue` manually. Ordinary completed planning turns are not auto-continued — they may be interview or
  approval handoffs. Planning never auto-approves or auto-implements. Plan-mode and tracker auto-continue share the
  `cross_turn_turns` episode budget; a planning episode can exhaust it for later tracker auto-continue until a genuine
  user turn resets the episode.
- Kill-switch: `[agent.harness.continuation].auto_continue_tracker = false`.

Harness-generated tracker, plan, blocked-end, verification, and background continuations preserve the existing episode
budgets; their prompt text must never reset counters as if it were a new user request. Without progress, the configured
cross-turn limit stops automatic retries even when steps stay blocked. Only a completed-count high-water mark within the
current user request resets the tracker retry budget; decreasing counts or recreating the checklist does not count as
progress. A fresh user request starts a new progress episode. Tool-free recovery applies only to its turn. A fresh turn
supersedes expired recovery guidance while retaining history and enforcing current planning, safety, verification, and
permission checks. Cleared source context can be recovered with targeted reads. Restoration guidance is appended once
per recovery episode, including planning, preflight, blocked-tool, loop-budget, navigation, and empty-response synthesis
paths; historical preview-exhaustion directives are also superseded. A later restriction requires a new restoration;
already superseded restrictions do not add redundant messages on subsequent turns.

Shared classifiers live in `vtcode_core::core::agent::completion` (`tracker_final_text_is_safety_handoff`,
`tracker_final_text_requires_user_input`, `recoverable_status_recap_phrasing`) so the binary runloop, outer queue, and
AgentRunner status path cannot drift on session/production vocabulary.

The closest concept mapping is:

| Agent SDK concept            | VT Code event                                                 |
| ---------------------------- | ------------------------------------------------------------- |
| `SystemMessage(init)`        | `thread.started`                                              |
| `AssistantMessage`           | `item.*` with `agent_message`, `reasoning`, `tool_invocation` |
| Tool-result `UserMessage`    | `item.*` with `tool_output` or `command_execution`            |
| `StreamEvent`                | `item.updated` plus Open Responses stream events              |
| `ResultMessage`              | `thread.completed`                                            |
| `compact_boundary`           | `thread.compact_boundary`                                     |
| Fresh plan execution handoff | `context.reset`                                               |

`turn.started`, `turn.completed`, `turn.failed`, and `turn.blocked` remain VT Code turn wrappers around the inner item
lifecycle. `turn.blocked` is emitted alongside `turn.failed` for blocked turns with streak/total/caps/last-tool counters
so UI layers get a first-class signal instead of inferring it. Harness `TurnBlocked`, `BlockedRecoveryStarted`, and
`BlockedRecoveryFinished` item events cover the recovery lifecycle.

### Approved-plan handoff boundary

Plan approval produces one immutable execution target containing the destination (`build` or `auto`), confirmation
policy, and current/fresh context. Normal approval targets Build; Auto is selected only by an explicit Auto choice or an
explicit full-auto policy. The source agent is never silently restored. Build and Auto share the same tool catalog,
command/path/verification gates, blocked- call fuse, budgets, and recovery limits; Auto changes confirmation behavior
only.

The handoff validates the persisted plan, persists and verifies its task tracker, emits one `plan.approval.resolved`
event, exits Planning, refreshes the selected agent's permissions and tool catalog, applies the context choice, and
queues exactly one implementation turn. Fresh and current execution use this same boundary. A failed handoff keeps the
validated plan available for a resumable retry, and queued mode input cannot replace the selected destination while the
transition is active.

### Tool-result ordering and bounded request repair

An assistant message with tool calls is one protocol batch. Every matching tool result is sent immediately after that
assistant message and before any intervening system, user, or assistant message; batch result order is retained.
Deferred prompt-injection warnings and recovery directives are appended only after all results in the batch. The warning
is rendered to the UI as soon as a probe flags output, but its model-facing message is queued and deduplicated so it
cannot split the provider batch.

Before a provider request, VT Code keeps a borrowed history view when this invariant already holds. Otherwise it builds
an idempotent request-only view that drops orphaned, causally early, and duplicate results, adds bounded cancellation
results for missing calls, and groups split results after their assistant call. Durable session history is unchanged. If
the provider returns the specific unmatched-tool-result `400`, VT Code retries once only when this wire view changes; a
no-op or repeated failure uses the existing resumable fail-closed handoff rather than issuing repeated identical
requests.

### Turn reliability and diagnostics

The runtime records the canonical `turn.*` lifecycle events exactly once for a turn. Model latency, output/reasoning
byte counts, tool-call count, finish reason, and reported input/output token counts are emitted as structured diagnostic
fields in runtime tracing; these diagnostics do not introduce a second event contract. Provider errors and timeouts emit
`turn.failed` through the owning run loop and never produce a successful completion.

Cancellation is fail-closed: an interrupted stream returns a cancelled finish reason, partial assistant text is not
treated as a successful final answer, and tool calls that were opened but did not reach a terminal provider response are
completed as failed. A timeout applies to the provider stream acquisition and is reported as a failure; it does not
silently convert an empty or partial stream into success. Steering follow-ups are applied to the live history at the
next turn-loop iteration boundary (after the current tool-call batch); when the turn ends before that boundary, they
remain queued for the next turn.

Failure-like tool results include hard failures, timeouts, and successful process responses with a non-zero exit status.
Non-zero results retain their stdout, stderr, exit status, partial output, and spool evidence, but count as failures for
metrics, batch summaries, and recovery diagnosis; they do not create a successful read-only signature. Low-signal
grep/no-match behavior remains unchanged. The existing model-facing tool response may include a bounded `diagnosis`
object:

```json
{"diagnosis":{"observed":"...","likely_cause":"...","next_action":"..."}}
```

Diagnosis uses only the already bounded tool preview and never reopens a spool. When routing permits, a tool-free
lightweight model request returns the same three bounded fields as strict JSON. Provider failure, timeout, or malformed
output falls back to deterministic evidence-only guidance. Policy, authentication, permission, circuit-breaker, sandbox,
resource, and preflight failures always use deterministic guidance, so diagnosis cannot recommend bypassing safeguards.
The UI renders an `Info` block and emits an existing `ReasoningItem` with stage `"diagnosis"` after the failed
`ToolOutput`; this remains visible when native reasoning is hidden. Provider-native reasoning continues to follow its
existing capability and display settings, and raw chain-of-thought is never exposed.

Deterministic execution-failure `next_action` values append hypothesis-revision framing from
`core::agent::hypothesis` (`classify_mismatch` → `revision_guidance`): each mismatch kind names its
inspect-and-revise step, so retries revise the hypothesis instead of repeating unchanged.

`ToolInvocationItem.outcome` remains the authoritative invocation result. Each runloop additionally emits exactly one
terminal `ToolLatencyRecorded` harness observation per executed invocation, carrying total duration, attempt count, and
the canonical error category when the invocation failed or recovered. Legacy retry/recovered event variants remain
readable, but new executions do not emit redundant per-retry terminal observations.

Parallel read-only calls consume any existing streamed invocation ID and emit the same canonical invocation and output
completion events as serial calls. Completion belongs to the execution future, including when the group is drained after
interruption, so turn teardown cannot label an executed call as unexecuted.

Read-only cache and history reuse also consume the streamed invocation ID and complete its invocation/output items
with the retained result, exit status, and spool reference. Reuse does not charge a fresh execution or fabricate a
latency observation. Pre-execution rejections use the same item identity but retain their failed status. Teardown closes
only calls that remain undispatched, so a cached result cannot later become a false cancellation in the archive.

When the product collapses or bounds a tool result, every provider/model receives the fixed disclosure after the
tool-result user message:
<!-- markdownlint-disable-next-line MD013 -->
`Only you see that command's output — the user's terminal shows at most a few lines of it. If the user needs to read any of it, put it in your reply.`
Anthropic wire routes whose selected provider/model capability supports it use `clear_at: "next_user_message"` and the
required beta; unsupported Anthropic models and gateways promote the same text to their top-level system prompt. Other
providers map it to their native system, history, instructions, or transcript representation without the Anthropic-only
`clear_at` field. One typed marker remains in canonical history for provider switching and replay.

### User-facing progress updates

The model-facing runtime contract is intentionally separate from provider native reasoning. The model states in one
sentence what it will do, updates only on findings, direction changes, or blockers, and ends with the outcome first,
then what changed, what it checked, and anything the user must do. Structured tool-call events are the authoritative
status signal. It must not narrate every tool call. When compact UI hides successful output, it should summarize
material findings in those visible updates or the final reply instead of rerunning commands solely to display output;
complete evidence remains available through Transcript Review.

## Terminal Thread Result

VT Code now emits `thread.completed` at the end of a session or exec run.

Fields:

- `thread_id`: stable event-stream thread identifier
- `session_id`: stable VT Code session identifier
- `subtype`: `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution`, or `cancelled`
- `outcome_code`: VT Code-specific terminal code
- `result`: final assistant summary text on successful completion only
- `stop_reason`: provider stop reason when available
- `usage`: aggregate token usage for the full thread
- `total_cost_usd`: aggregate estimated cost when pricing metadata exists
- `num_turns`: total turn count

For `vtcode exec`, `outcome_code` comes from `TaskOutcome::code()`. Interactive sessions preserve the corresponding VT
Code session end semantics.

## Canonical event persistence

Interactive and exec runs share one authoritative persistence contract:

| Concern          | Contract                                                                                                                                                   |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Event type       | `vtcode_exec_events::ThreadEvent` is the only runtime event contract.                                                                                      |
| Canonical path   | `<workspace>/.vtcode/sessions/<session_id>/events.jsonl`, with `manifest.json` and derived artifacts beside it.                                            |
| Ordering         | One dispatch gate feeds canonical persistence and optional exporters in the same order.                                                                    |
| Backpressure     | Canonical events use bounded non-blocking handoffs to a blocking I/O drain; queue saturation fails closed, and accepted events are never silently dropped. |
| Shutdown         | The terminal `thread.completed` event is emitted first, then exporters finish and canonical persistence drains before success is reported.                 |
| Lifecycle status | Sessions become `active` at `thread.started`/`turn.started`; only `thread.completed` makes the manifest terminal.                                          |
| Retention        | Closed sessions use the 50-session/30-day defaults. Active sessions, the current session, symlinks, and unrelated files are preserved.                     |

`agent.harness.event_log_path` and exec `--events` are explicit compatibility exports. They do not replace the canonical
store, and no global The user state directory's `sessions` harness artifact is created by default. ATIF and Open
Responses files, when enabled by the interactive harness, are derived under the canonical session's `derived/`
directory. Historical global artifacts are left untouched.

## Compaction Boundary

Whenever VT Code compacts history itself or via a provider-native compaction path, it emits `thread.compact_boundary`.

Fields:

- `thread_id`
- `trigger`: `manual` or `auto`
- `mode`: `local` or `provider`
- `original_message_count`
- `compacted_message_count`
- `history_artifact_path`: optional archived history path
- `previous_segment_id` and `new_segment_id`: optional cache segment transition
- `previous_prefix_hash` and `new_prefix_hash`: optional immutable prompt-prefix hashes
- `previous_catalog_hash` and `new_catalog_hash`: optional ordered tool-catalog hashes

Each request segment freezes one system prompt, instruction digest, and deterministically ordered tool catalog. Ordinary
turns only append messages. Compaction, instruction changes, catalog expansion, or primary model/provider/mode changes
start one new segment; the immutable event archive is retained and local compaction seeds the new segment with its
summary and continuity tail.

This is emitted for manual `/compact` flows, for automatic compaction, and for automatic local fallback compaction. When
Open Responses is enabled, VT Code surfaces these as VT Code custom extension events without changing the core Open
Responses response model.

## Fresh Plan Execution Context

Selecting “Yes, clear context and implement” is a plan-to-build handoff inside the same user session. The runtime
preserves the approved plan, task tracker, working tree, configuration, permissions, authentication, and aggregate
usage. It clears only the live transcript and other transient continuation, recovery, cache-lineage, request-segment,
and tool-budget state, then starts a normal build turn with a compact handoff directive.

The successful handoff emits `context.reset` with `trigger`, `plan_preserved`, `previous_context_usage_percent`, and
`tool_budget_reset`. The event is also written to the normal JSONL/log stream and forwarded by the Open Responses bridge
as `vtcode.context_reset`.

### Unified auto-compaction

Auto-compaction is **on by default** (`agent.harness.auto_compaction_enabled`, default `true`) and is **unified across
both runloops**: the core `AgentRunner` loop and the binary unified runloop both delegate to the shared
`vtcode_core::compaction` orchestrator (`auto_compact_messages`) rather than maintaining separate compaction logic. It
fires at the effective session ceiling: resolved model capacity bounded by the provider route and a positive
`context.max_context_tokens` safety ceiling, with room reserved for the next response. The default ceiling is zero
(automatic); known request output limits are reserved, falling back to 4,096 tokens when unavailable. An explicit
`agent.harness.auto_compaction_threshold_tokens` can lower this trigger but cannot bypass either the safety ceiling or
output reservation. Disabling normal auto-compaction does not disable the single bounded recovery compaction used after
a provider rejects a follow-up that follows successful tool output; that safety path preserves the current request and
completed tool results, and blocks truthfully if it cannot reduce the context.

To preserve conversational continuity, every compacted history keeps:

- a **continuity tail** — approximately 20,000 estimated tokens of the newest complete user/assistant/tool protocol
  groups retained verbatim. Incomplete trailing tool calls are dropped, and an oversized individual message is
  represented by a bounded preview/spool reference;
- the structured **session memory envelope** injected at the boundary (see Resume and fork continuity).

The runloop derives a soft scheduling boundary from the effective prompt threshold. Reaching it marks compaction pending
and defers the work to the next outer turn boundary. The effective prompt threshold compacts before the next model
request. No hidden summary model call is issued in the middle of an active tool loop.

If a transient provider failure follows successful tool execution, the unified runloop first compacts only the older
prefix, preserving the current request, tool outputs, and memory envelope. It emits one canonical
`thread.compact_boundary`, then permits one bounded `ToolEnabledRetry` so a required edit or verification can still run
without repeating completed read-only exploration. A second failure produces a blocked, resumable handoff; the harness
never reports successful completion without confirmation.

### Blocked-turn recovery

Every blocked turn publishes one non-empty deterministic assistant response through normal conversation history, the
renderer, and `ThreadEvent::ItemCompleted` with `AgentMessage`, then emits the corresponding `turn.failed` event plus a
first-class `turn.blocked` event with fuse counters. The turn result remains `Blocked`; publishing the handoff does not
convert it to success or emit `turn.completed`. The TUI surfaces the block via a `Blocked` header badge,
`Blocked • continue to retry…` footer hint, transcript banner, and `ActionRequired` terminal title;
`ActivityState::Blocked`/`Recovery` drive those states while input stays enabled. Blocked-turn spool outputs are pinned
until the blocker resolves so `continue`/`--resume` can still read them.

Blocked responses are reason-specific. A pending-verification response explains that inspection-only checks, link
checks, and `git diff --check` do not clear the anti-blind checkpoint and directs the operator to run
`cargo check --locked` or the relevant `cargo nextest run`. The gate trips after 6 consecutive successful mutations;
docs-only prose edits stay allowed while pending and never increment the counter. Before blocking, the harness grants
bounded autonomous recovery without manual `continue`: each in-turn text-response cap-hit (2 consecutive texts) consumes
one of the configured in-turn attempts (default 2) that reset the streak and inject a project-aware directive naming the
exact detected verifier (`default_verifier_for_workspace`: `Cargo.toml` → `cargo check --locked`, `go.mod` →
`go test ./...`, `package.json` scripts → `npm test`/`npm run check`/`lint`/`build`, pytest markers → `pytest -q`,
`Makefile` → `make test`, `justfile` → `just test`; standalone or pure-`&&` chain, `max_output_tokens` for truncation;
`[agent.harness.verification].default_verifier_override` wins when set and valid). For documentation-only work,
recovery prefers the latest recorded failed checker for the current request, such as README Markdown lint, over the
workspace build. Running checkers retain their exec-session identity through read-only polls/waits until a matching
terminal result arrives; unrelated sessions and superseded checkers cannot supply that verdict. Rejected or cancelled
polls retain identity, while terminal completion, lost sessions, and successful cleanup retire it. Code or unknown
mutations, uncompleted/rejected checks, and commands with launch context that command-only replay cannot preserve
(including directory aliases, environment, shell/login, and sandbox overrides) fall back to workspace detection.
The same resolver drives manual resume, in-turn verification, and cross-turn recovery. When the directive budget is
exhausted, the harness runs that verifier itself once per turn through the normal tool pipeline (admission, permissions,
budget, and gate accounting identical to a model-run verifier; kill-switch
`[agent.harness.verification].auto_execute = false`): exit 0 clears the gate and the turn continues, a non-zero exit
grants the fix-up window with the failure in history, and a denied/unresolvable command falls through to the manual
handoff. A completion claim ("done", "complete", "all tests pass") while pending skips the directive rounds and jumps
straight to harness verification — asserting success without evidence is the moment that needs evidence most. Tool-free
recovery synthesis bypasses verification accounting entirely (its texts cannot verify by design; recovery budgets and
the generic cap, which still refuses unverified completion, govern it). Consecutive harness failures escalate: after
`[agent.harness.verification].max_consecutive_failures` (default 3) the harness stops executing and writes a handoff
carrying the failing command and its output tail. If the turn still blocks on verification, the session loop schedules
up to the configured cross-turn recovery turns (default 2; skipped once escalated) as system directive + queued
follow-up with no blocked handoff and input staying enabled, before writing `.vtcode/tasks/current_blocked.md` and
requiring manual `continue`. Tracker progress restores the verification turn budget without clearing the failure
escalation counter, so long-running work that keeps completing tracker steps gets fresh bounded recovery for new edits
while a never-passing suite still escalates. The exhausted handoff names the `attempt/max` recovery count and the exact
verifier to re-run standalone, and the transcript banner leads with that verifier-first step instead of the generic
`continue` nudge. A user-typed `continue` after a verification stall resumes verifier-first with the detected project
command instead of the generic conclude-oriented recovery; stalled follow-up treats a stall reason naming the pending
gate as verification-stalled even when the snapshot was lost across compaction or model switch. A context-capacity
response explains that bounded compaction could not reduce the request, retains completed tool outputs, and directs the
operator to resume after reducing context or switching models. Other blocked reasons use a generic retry handoff.
Existing recovery text is reused when it was already published, so the assistant item is never duplicated.

A lost verification result must not deadlock the anti-blind gate. While the checkpoint is pending, a verifier-level
`Failure`/`Timeout` (for example, the exec session ended before the verifier's output was captured, reported by a
`write_stdin` or non-run `unified_exec` session follow-up as a missing session) grants the same bounded fix-up window
as a genuine failed verifier and surfaces a "Verification result lost" directive; the gate still only clears on a
successful standalone verifier re-run. Pure `head`/`tail` truncator pipelines are elided at execution into the
standalone verifier (capped via `max_output_tokens`), so their exit status is the verifier's own and a success clears
the gate like any standalone run. Static pipelines with independently read-only filters instead preserve filtering and
enable fail-closed `pipefail`; only terminal exit 0 clears the gate, and a non-zero exit grants the existing bounded
repair window. All admission, permission, command safety, and execution budgets remain enforced. Remaining non-
rewritable sequences (such as `;` joins) queue a one-shot "piped verifier did not clear the gate" directive, because
the pipeline's exit status belongs to the tail command and the model would otherwise read the silence as verified. The
`turn.blocked` event also populates `last_tool`, `consecutive_cap`, and `total_cap` from the blocked-tool-call fuse
when it tripped, and transcript block reasons are truncated (~600 chars) with a pointer to the handoff file, which
retains the full reason.

The assistant text-response safety cap distinguishes finished work from a stalled loop. When the cap fires after the
harness promoted the latest commentary to a final answer and the anti-blind checkpoint is clear (and planning is not
active), the turn ends `Completed` — the model had concluded and the loop merely re-prompted past its answer. The cap
ends `Blocked` only when no final answer could be preserved or the verification gate is still pending.

The fork/branch history builder (`build_summarized_fork_history`) deliberately omits the continuity tail and produces a
minimal resume artifact (envelope + summary + retained users only).

### Long-running command waits and durable steering

`write_stdin` and `unified_exec` accept an explicit `action: "wait"`. A wait blocks until the process exits or its
requested deadline expires, then returns one bounded result. A deadline does not terminate an in-progress process; the
returned session ID is reusable for a later wait. Wait time is excluded from the ordinary per-turn harness wall-clock
budget, while cancellation, shutdown, safety policy, and the configured long-running-command ceiling remain active.

Non-draining inspection returns the latest bounded output snapshot. A sliding head/tail preview, including a same-length
update or a reset after another reader drains output, replaces the previous snapshot. Draining waits append new chunks.

Missing sessions fail promptly, including during output inspection; they are not successful empty captures. Typed lookup
errors preserve the distinction from permission or process failures and carry deterministic recovery guidance. Reuse the
original response or recorded completion output before considering a fresh command; an absent handle alone does not
establish execution failure.

Managed background subprocess completion and user-launched background exec completion are delivered independently of
that explicit wait. The controller persists managed terminal state before publishing its bounded payload; the raw exec
watcher publishes the user-session terminal signal after confirmed exit. The run loop can therefore trust the status and
exit code without a manual poll. It drains these events only at a safe boundary: active turns defer them, queued user
input takes precedence, and an idle loop schedules at most one follow-up reasoning turn. Direct commands update state
and transcript without creating an unrelated autonomous turn. The canonical `ThreadEvent` item is
`background_subprocess_completed` (event schema 0.16.0).

When a turn ends while a foreground `run-*` exec session is still running, the next turn start injects a bounded resume
hint (`Exec session resume:`, at most 4 sessions newest first, per-command display truncated to 160 bytes,
single-session hint under 1 KiB) with a pre-filled
`write_stdin {"session_id", "action": "wait", "wait_timeout_seconds": 600}`. The hint flows through the same turn loop
for normal next-turn and session restore/resume, so a compacted session needs zero identity reconstruction.
`wait`/`inspect` stay exempt from the per-turn tool-call budget. Turn-end
`SnapshotTurnDiagnostics.in_progress_exec_sessions` and `turn.completed.in_progress_exec_sessions` (schema 0.16.0,
bounded to 4) record all live ids, including retained background sessions; the resume hint uses the foreground subset.
See invariant #22 in `docs/harness/ARCHITECTURAL_INVARIANTS.md` and the agent-facing settle shape in
`docs/harness/AGENT_LEGIBILITY_GUIDE.md`.

Session input is policy-checked: each submitted line is evaluated against the same PTY deny list that guards session
creation, so a denied interactive program cannot be launched by typing into an already-running session. The
`unified_exec` `action: "code"` path is likewise subject to the command policy through its interpreter program
(`python3`/`node`); it cannot bypass a policy that excludes those programs.

Command responses never expose unbounded accumulated `raw_output`. They return a bounded preview plus total bytes,
truncation state, exit state, and a spool path when the output file is available. `spool_complete` is false while an
active session is still writing and the path is a readable partial snapshot. When an exited session is still draining,
the path is withheld and `spool_pending` is set until a later wait completes the output spool. Producer-marked spooled
command responses reuse the command preview policy and cap the model-visible preview at the smaller of the requested
budget and 6 KiB. Inspection commands preserve head and tail context; verification and mutation commands preserve the
tail. The complete spool, byte counts, reference metadata, and failure or recovery diagnostics remain available without
reopening the spool while the response is constructed. Completed references carry `spool_state = "completed"`,
`spooled_bytes`, and `spool_sha256`; consumers validate all three against a descriptor-relative, no-follow open before
replay. Pending live spools are append-only and may be read only through the bounded, explicitly unverified pending
path. Legacy references without integrity metadata remain deserializable but are not replayable as completed output.

Independent read-only calls may fan out after hook rewrites, argument-aware intent classification, command hardening,
and preflight all succeed. Direct file/search reads and simple allow-listed inspection commands are eligible. Identical
semantic calls, mutations, compound or dynamic shell commands, PTY/session actions, polling, and stdin writes remain
sequential. Both runloops honor `max_parallel_tool_calls` and trace the configured limit, admitted calls, group count,
parallel-group count, and maximum group size.

Provider-visible tool previews are bounded independently for each result. The registry respects the requested output
token limit, capped at 64 KiB for execution or 96 KiB for planning; the provider-history boundary applies the same
fallback ceiling. Large output remains in the spool and current-session viewer, with a bounded preview and
outcome/control metadata. Targeted reads, spool pages, and verifier diagnostics remain visible regardless of prior
output volume. History compaction controls accumulated context; preview size never blocks inspections or forces a
tool-free synthesis pass. Legacy `preview_budget_exhausted` markers remain readable for diagnostics but do not change
current tool availability. Replacing a result does not double-count truncation diagnostics.

Blocker live pointers are cleared only by the session that created them; archived blocker files remain self-contained,
append a durable resolution marker before pointer cleanup, and do not claim ownership of the workspace-global task
tracker. An ordinary user exit after a completed non-fallback turn is reported as successful thread completion; an exit
that terminates active work remains cancellation. Starting a new session also preserves the last turn's blocked,
aborted, fallback, or cancelled outcome instead of reporting success for unconfirmed work. A failed approved-plan
summary remains a failure even when its final response is substantive. A thread budget limit takes precedence over both
the last turn outcome and a clean exit.

After a tool-call or wall-clock budget is exhausted, compact rejection stubs direct the model to synthesize from
existing evidence. They do not suggest another tool or narrower scope; the batch still receives exactly one synthesis
directive and the existing tool-free recovery pass. Control-plane waits retain their tool-call budget exemption.

Turn-balancer recovery resets navigation/repetition evidence only. It preserves the anti-blind mutation count,
`verification_pending`, and any failed-verifier fix allowance; only a truthful successful verifier (including a validated
pipefail pipeline) clears that checkpoint.

Workspace-aware tool responses and execution summaries render paths inside the active workspace relative to that
workspace (for example, `.vtcode/tasks/current_task.md`). Paths outside the workspace keep their absolute form so
diagnostics do not hide external locations. The same display rule is used for generated planning artifacts and handles
canonical workspace paths reached through symlinks; planning lifecycle events reuse the same display value, and it does
not change the path used for I/O.

Steering follow-ups are persisted as UUID-tagged intents. The schema-v3 session envelope keeps at most 16 pending
intents and a 64-ID applied window. Recovery replays only pending IDs absent from both the applied window and tagged
user history; the public `FollowUpInput(String)` message shape remains unchanged. Intents are applied to live history at
the next turn-loop iteration boundary (after the current tool-call batch) and acknowledged only after the post-turn
history checkpoint succeeds.

## Budget and Limits

`agent.harness.max_budget_usd` is the shared budget setting for interactive and exec sessions.

- VT Code estimates cost from aggregate usage via `ModelResolver::estimate_cost`.
- If pricing metadata is unavailable for the active model, VT Code does not enforce the budget.
- In that case `total_cost_usd` stays `null` and VT Code emits one warning.

Turn limits still surface through `thread.completed.subtype = "error_max_turns"`.

## Hooks

VT Code now supports `hooks.lifecycle.pre_compact`.

`pre_compact` runs before VT Code records a compaction boundary. Its payload includes:

- `session_id`
- `cwd`
- `hook_event_name = "PreCompact"`
- `trigger`
- `mode`
- `original_message_count`
- `compacted_message_count`
- `history_artifact_path`
- `transcript_path`

`session_start` with source `compact` remains supported for compatibility, but `pre_compact` is the first-class hook for
compaction-aware automation.

## Orient Phase

Every session should begin by gathering orientation context from external artifacts. This follows the long-running
harness pattern: the agent reads the progress ledger, harness artifacts, loop memory, and git log to understand the
current state before acting.

The orient phase produces an `OrientationContext` (see `crates/codegen/vtcode-core/src/core/agent/bootstrap.rs`) that
includes:

- Progress ledger summary (goal, completion ratio, confidence, stall status)
- Harness artifact summaries (spec, contract, sprint contract, evaluation, outcome verification)
- Recent git log (last 5 commits)
- Loop memory notes and decisions from previous iterations
- Handoff context from a previous agent, if any

This context is injected as a `[Orientation Context]` section in the system prompt, using summaries and references
rather than full content to keep the context lean.

## Handoff Protocol

When one agent hands off to another, it produces a `HandoffRequest` (see
`crates/codegen/vtcode-core/src/core/agent/handoff.rs`) that includes:

- **State summary**: what was accomplished, what remains
- **Boundary status**: explicit list of features/deliverables with Done/InProgress/NotStarted/Blocked status
- **Modified files**: files changed in this session
- **Test results**: last test run outcome with actual output
- **Open decisions**: unresolved questions for the next agent
- **Known issues**: bugs, limitations, tech debt the next agent should know
- **Next actions**: recommended next steps
- **Task context**: the original task description

The handoff prompt is rendered as a structured markdown section that the next agent can parse without re-exploring the
codebase. This prevents the "inheriting a collaborator's mess" problem: the boundary status makes explicit what is done
vs. what was left incomplete.

## Related Controls

These VT Code settings line up with common agent-loop controls:

- Tool allow and deny rules: `[permissions].allow`, `[permissions].deny`, tool policy config
- Permission policy: workspace trust, human-in-the-loop settings, granular agent rules, and full automation allow-lists
- Effort: provider/model reasoning settings
- Tool discovery: MCP and tool catalog flows
- Resume and fork continuity: session archives, thread bootstrap, and compaction envelopes

## Context Reset

Context reset is a context engineering technique **distinct from compaction**. While compaction preserves conversational
continuity within the same task, context reset deliberately discards conversation history so a fresh agent can reorient
from durable artifacts only.

### When It Triggers

Configured via `agent.harness.context_reset_mode`:

| Mode            | Trigger                                                   | Use Case                                      |
| --------------- | --------------------------------------------------------- | --------------------------------------------- |
| `off` (default) | Never                                                     | Normal operation                              |
| `on_stall`      | `context_reset_stall_threshold` consecutive stalled turns | Long-horizon tasks where the agent gets stuck |
| `on_compaction` | After every auto-compaction                               | Clear noise accumulated before compaction     |

### What Happens

When a reset triggers:

1. A versioned private transition manifest is atomically written to `.vtcode/tasks/current_transition.json`. It records
   thread/turn identity, transition kind, local trigger metadata, checkpoint paths, and references to the existing
   compaction/reset events. Legacy `current_context_reset.md` files remain readable and are migrated in memory.
2. The next session starts with **only** `OrientationContext` — no conversation history is carried forward.
3. The orient phase reads the manifest and prepends a `### Context Reset` banner: "This session starts from a clean
   context. Reorient from the artifacts below."
4. The manifest is consumed only after orientation and reset application both succeed. Parse or orientation failure
   leaves it intact for retry.

The public `context.reset` event keeps its existing schema. Stall/compaction resets use the public `Unknown` trigger
while the real local trigger remains in the private manifest; compaction and reset therefore retain distinct semantics.

### Artifacts That Survive a Reset

All durable artifacts persist across a reset:

- Progress ledger (`crates/codegen/vtcode-memory/src/progress.rs`)
- Harness artifacts (spec, contract, feature list, evaluation, sprint contract)
- Loop memory (notes, decisions)
- Git log and working tree state
- Compaction summary

The comparison with compaction is summarised in [Context Reset](#context-reset).

## Loop Engineering Additions

The subagent layer now supports loop-engineering primitives:

- **Worktree isolation**: set `isolation = "worktree"` on an agent spec to run the child in a git worktree under
  `.vtcode/worktrees/`. The child's file mutations stay in its own working tree until explicitly merged.
- **Propose/verify separation**: `SubagentController::verify_proposed_change()` spawns a read-only verifier sub-agent
  that re-reads affected files and approves or rejects the change. The verifier has no shared context with the proposer.
- **Loop run state**: `crates/codegen/vtcode-core/src/loop_state.rs` persists step index, cumulative cost, and status to
  `.vtcode/state/loop-<id>.json` so a scheduler can resume across invocations.
- **Loop memory**: `crates/codegen/vtcode-core/src/loop_memory.rs` provides an append-only store for agent notes and
  decisions in `.vtcode/state/notes.md` and `decisions.md`.
- **Cost guardrails**: `CostBudget` in `loop_state.rs` tracks token/cost/step limits and reports `BudgetStatus`
  (Ok/TokenLimitReached/CostLimitReached/StepLimitReached).

See [Loop Engineering](../loop-engineering.md) for the full design.
