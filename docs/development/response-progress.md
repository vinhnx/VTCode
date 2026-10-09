# Response progress and latency

The inline and fullscreen interfaces show one temporary progress row as soon as a request is accepted. The row
follows actual work: initialization, context preparation, checkpoint saving, model wait, processing, response reception,
retry backoff, permission checks, approval waits, and admitted tool execution. Individual tool rows and queued prompts
keep their existing presentation. Progress appears once: the footer uses the same phase label and elapsed seconds
only when the current layout cannot show the transcript progress row. Git status stays static even when the branch
name contains an activity word such as `blocked`. While that row is visible, the footer retains
the configured status-line content: Git/worktree context in `auto` mode, custom command output in `command` mode,
and no configured text in `hidden` mode. The existing `ui.status_line` settings, clock visibility, command refresh
interval/timeout, automatic fallback for a missing command, and live configuration reload remain effective.
Configured output is retained by source, so text such as `Running custom dashboard` is not discarded as tool activity.
The footer reserves persistent regions before fitting optional content: mode, configured right-side content, configured
context, then hints. Each region is truncated independently by terminal columns. Loading phases and elapsed seconds
cannot push out the mode or move the right-side content. When the transcript row cannot fit, loading uses a bounded
24-column optional footer slot after context. Copy notifications and shell hints retain
their footer presentation when space permits. Background activity stays out of the bottom line while anything is
busy (transcript loading row, foreground command, or in-flight turn): the transcript loading row carries a static
`· N bg` suffix and the header carries a short `• N bg` / `✓ N done` badge, so the composer line never hides/shows
as turn status, the fallback budget, or the foreground-command counter toggles. The `Running N background tasks...`
copy and drawer hint return to the bottom line (clickable, width-deterministic) only once everything is idle. The
foreground-PTY `Ctrl+B background` hint never appears in the bottom line while busy; background a running command
with `Ctrl+B`, empty-Enter (`/jobs`), or `/subprocesses` — the header suggestions line keeps showing the shortcut
while a foreground command runs.

`ProgressOperation`, `ProgressPhase`, and `ProgressUpdate` live in `vtcode-commons::ui_protocol`. The UI accepts a new
operation identity, updates only that active identity, and clears only the matching operation. Finished identities
remain superseded so delayed updates cannot resurrect an old row. `InlineHandle::begin_progress` returns an owning
guard; `transfer` and `resume_progress` retain the acceptance clock when the interaction loop hands off a model turn.
Guard drop clears progress on completion, preparation failure, provider failure, cancellation, and session handoff.
The UI deduplicates phase updates against the displayed operation. Copilot runtime requests can temporarily show
tool execution or approval waits; when they settle, the request caller restores model progress before more text.
Keyboard and paste handlers only emit input events. They do not create provisional operations: a submission can
be consumed by a local command, overlay, or focused process before the runtime accepts a request.

Progress is presentation state. It does not grant permissions, set `ActivityState`, change input ownership, or enter
conversation history, exports, canonical events, or provider requests. The transcript widget reserves one row outside
the scroll, link, and selection body. Modals and other overlays clip it through the existing render order. When progress
consumes the entire transcript allocation, transcript selection uses an empty area; overlay selections retain their
own viewport. Elapsed seconds update on shared ticks; approval waits stay static. Animated phases reuse `ShimmerState`
and `tui-shimmer`
with the existing 33 ms frame interval and two-second sweep. Reduced motion and screen-reader policy retain static
labels. Animation requests a redraw without invalidating transcript reflow.

## Measurements

Enable the existing debug diagnostics for `vtcode.response_latency`. Measurements use `Instant` and log operation,
step, and attempt identifiers with durations; they never include prompts, credentials, response bodies, or provider
error strings. Relevant observations are:

| Observation | Meaning |
| --- | --- |
| `accepted_to_feedback_ms` | Acceptance to the final frame containing the progress row or footer fallback |
| `checkpoint_ms` | Awaited checkpoint preparation, including durable publication and retention |
| `preparation_ms` | Outer turn preparation before the execution clock starts |
| `accepted_to_prepared_ms` | Includes earlier interaction-loop preparation |
| `request_assembly_ms` | Provider request construction |
| `dispatch_to_first_event_ms` | Provider dispatch to the first observed provider activity |
| `dispatch_to_visible_output_ms` | Provider dispatch to the first nonempty visible text emission |
| `accepted_to_visible_output_ms` | Includes preparation before the execution clock |

Feedback is observed once per operation after overlays are painted. A covered row, suppressed footer, empty area,
or footer containing only an ellipsis does not establish visible feedback.

Completion-only streaming responses emit a visible-output observation after sanitized fallback rendering. Already
streamed text does not emit a duplicate completion observation, and suppressed or blank output does not count.
Hidden reasoning is provider activity, not visible text. Non-streaming requests record response availability as their
first event; visible-text emission diagnostics currently cover streaming requests. Terminal paint timing can be
measured separately with a PTY fixture. These observations do not alter execution budgets, retry limits, or timeout
clocks. Compare rebuilt release binaries in three paired runs and report provider wait separately.

## Checkpoint preparation

The prompt worker acquires the existing exclusive rewind lock, captures content, serializes JSON, and durably
publishes the checkpoint and navigation record on the blocking pool. Its lease remains held until the turn settles.
Independent dirty-worktree inspection starts before checkpoint preparation and is awaited before request dispatch.
If checkpointing fails, drain that worker and retain the existing prompt restoration path. Cancellation interrupts
preparation waits before tools are admitted. The worker retains ownership through safe checkpoint publication and
lease release even when the caller stops waiting; cancelled preparation never grants a tool execution lease.

Hot retention discovers retired records before parsing live checkpoint JSON. With no retired records, it skips live
record parsing, content-store opening, and garbage collection. Reclamation still checks every live reference before
deleting retired sessions, and corrupt metadata defers cleanup. Explicit full maintenance also collects old orphaned
content when no retired records exist. Existing resource, catalog, connection, and request-prefix caches retain their
current ownership and invalidation rules.

## Cancellation and exit

First Ctrl+C cancels the current task; a second press within one second exits. The TUI callback publishes exit
immediately, including during initialization and preparation. Exit is irreversible within a session. A fresh
submission may clear handled cancellation; it cannot clear an exit request. Cancelled provider follow-ups end as
Cancelled rather than entering recovery, fallback synthesis, or automatic planning/tracker continuation. Completed
output, task state, queued user messages, and composer drafts remain available. Fresh user input is required to resume.
During initialization, first cancellation pauses the remaining initialization work; submitting fresh input resumes it.
Preparation assembles transient system notes without changing conversation history. Notes enter history only after
the cancellable preparation wait succeeds, so interrupted preparation cannot leave them in later requests or archives.

Exit cleanup uses one 1.5-second session deadline and the existing 500 ms runtime shutdown allowance. Independent
persistence, hooks, child cleanup, and TUI finalization run concurrently under the remaining deadline. Cancellation
and exit skip LLM-backed memory finalization. Completed canonical persistence errors, code-change snapshots, and saved
archive identifiers are retained independently when other maintenance times out. Archive identifiers are published
after the atomic write, before later hooks. Terminal restoration and the exit summary always run after maintenance.
Terminal event draining uses an atomic timed read in the maintained crossterm fork, with a 20 ms deadline and 128-event
maximum. Registry crossterm builds use a bounded 10 ms poll to collect pending terminal replies; they avoid a subsequent
blocking read because registry crossterm has no atomic timed-read API.

Content-free `vtcode.shutdown` diagnostics record exit acceptance, teardown, persistence, TUI closure, terminal
restoration, postamble completion, and runtime shutdown/process return. Durations use monotonic clocks; no prompts,
responses, or credentials are recorded. The PTY regression script checks cooked-mode restoration, one complete
postamble, child cleanup, and accepted-exit-to-shell-return latency with continuing events and stalled hooks:

```sh
python3 scripts/tests/test_exit_latency.py target/debug/vtcode --baseline /path/to/rebuilt/baseline --runs 3
```

The baseline is observational; the candidate must return within two seconds. Use binaries built with the same
profile and record their provenance. The fixture uses isolated temporary configuration and makes no provider calls.
