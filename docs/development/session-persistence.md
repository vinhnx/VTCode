# Session Event Persistence

VT Code stores the authoritative `vtcode_exec_events::ThreadEvent` stream for interactive and exec sessions at:

```text
<workspace>/.vtcode/sessions/<session_id>/events.jsonl
```

The session directory also contains the manifest, turn index, and derived artifacts. The canonical event sink uses
bounded, non-blocking handoffs to a dedicated blocking drain. Filesystem I/O does not run on the async executor. If a
bounded queue fills, persistence fails closed and the run cannot be reported as successful; accepted events are drained
before the failure is returned. A run may be reported as successful only after its terminal `thread.completed` event and
the canonical drain have completed successfully.

`agent.harness.event_log_path` and exec `--events` are explicit compatibility exports. They are optional, independently
bounded, and may report drops or write failures without changing the canonical event contract. No global the user state
directory's `sessions` harness file is created by default. ATIF and Open Responses exports configured by the interactive
harness are derived under `<session>/derived/`.

Retention defaults to 50 sessions or 30 days. It operates after session closure/startup on a blocking task, preserves
active/current sessions, skips symlinks, and removes only validated direct children of the sessions root.

## Concurrent sessions

Several `vtcode` processes may run in one workspace at once (for example one
per terminal pane), each with its own session. The stores are built for this:

- Event log: one session, one writer. The runloop's event sink opens the log
  with `open_exclusive`, which fails closed with `SessionBusy` when another
  process already holds the session — resume the session in its own terminal
  or fork with `--session-id <suffix>` instead. Readers (`session-store
  inspect`, retention scans) use the shared `open`, which degrades
  gracefully and never fails on a live writer.
- Checkpoints: each session owns disjoint `turn_{key}_{n}.json` records plus
  a per-session `rewind-{key}.lock` held for the turn duration. Same-session
  overlap (a second turn or rewind) is refused; other sessions proceed
  concurrently. Pre-upgrade `turn_{n}.json` records keep loading through a
  legacy fallback. `revert --turn N` resolves legacy records first and fails
  closed naming `--session` when several sessions recorded the same turn.
- Checkpoints retention serializes content reclamation (`delete_sessions` +
  `collect_garbage`) on a best-effort `rewind.lock` mutex: a process that
  finds cleanup running elsewhere skips its round and retries later.
- Trajectory rotation names files `trajectory-{ts}-pid{pid}.jsonl` (plus a
  counter on repeats) so concurrent startups never clobber each other's
  rotated logs.

Still shared across sessions by design: the `current_task.md` mirror and
`current_blocked.md` pointer are last-writer-wins (both write atomically;
the tracker bails instead of clobbering on races, and blocked-handoff
clears are session-scoped), and explicitly started servers (`webmcp serve`,
the Anthropic API shim) fail fast with a clear bind error when their
configured port is taken.

Focused checks:

```bash
cargo nextest run --locked -p vtcode-memory -E 'test(session_store) or test(session_exclusivity)'
cargo nextest run --locked -p vtcode-core -E 'test(snapshots) or test(checkpoint_concurrency) or test(trajectory)'
```

## Peer-session messaging

Concurrent sessions in one workspace can ask each other questions through a
file mailbox under each session store (`mailbox/*.json`, acknowledged mail
archived under `mailbox/read/`). The agent tools are `ask_session` (send a
question to a live peer), `answer_session` (reply naming the question's
`message_id`; the reply target resolves from the asker's inbox entry), and
`list_sessions` (live peers only). Sends go to flock-live sessions only and
fail closed otherwise; delivery is at-least-once (`ack` after the receiving
turn admits the note to history, redelivery on failure) with TTL clamped to
60..86400 s (default 3600 s) and expired mail moved aside at drain time.

Each turn drains the inbox during turn preparation (`support::drain_peer_messages`,
fail-open so a sick mailbox never breaks the turn), injects a bounded
`Peer session messages` system note with an `answer_session` reply hint, and
emits correlated `SessionMessageReceived` / `SessionMessageExpired` harness
events (the `SessionMessageSent` counterpart is derived from the tool output
at record time; all three link via `correlation_id`). `session-store list`
marks live sessions so users can see messaging peers.

Focused checks:

```bash
cargo nextest run --locked -p vtcode-memory -E 'test(mailbox) or test(live_sessions)'
cargo nextest run --locked -p vtcode-core -E 'test(session_messaging) or test(session_message_sends)'
cargo nextest run --locked -p vtcode -E 'test(turn_notes) or test(list_row_marks_live)'
```

## Memory-envelope file replacement

The compaction memory envelope is a derived continuity artifact. Its synchronous and asynchronous writers use the
shared private-file policy: create an exclusive temporary file beside the destination, write and sync its contents,
then rename it over the existing file. The async entry point runs filesystem work on a blocking worker. Windows uses
the same replacement operation; the destination is never deleted first. Temporary files are cleaned after failed
publication, and failures retain the last committed destination.

Envelope writes reject symlinked destinations and unsafe parent paths, preserve the pretty-printed JSON format, and
use private file permissions on Unix. Bare relative file names remain valid. The canonical `ThreadEvent` stream and
session retention policy retain their existing ownership.

Focused checks:

```bash
cargo nextest run --locked -p vtcode-core -p vtcode-commons -E 'test(memory_envelope) or test(private_file_writer)'
```

Windows CI includes these persistence tests, including a locked-destination failure regression. Native Windows
execution is required to verify that platform-specific failure path; a macOS or Linux run excludes it.

## Audit packs

`vtcode session-store pack <session>` produces a digest-verified snapshot of a session's store: every file (canonical
log, manifest, turn index, derived views) with its byte count and SHA-256, plus a manifest counter snapshot. Packs
default to `<session>/derived/audit-pack.json`; `--output` overrides the destination. `--verify <pack-file>` re-digests
the current session contents and reports per-file `MODIFIED`/`MISSING`/`NEW` status, failing with a non-zero exit when
listed artifacts no longer match. Files added after the pack are reported as `NEW` but do not fail verification; the
pack pins a point-in-time, it does not forbid later writes. Pack entry paths are validated on load, so a hand-edited
pack cannot point verification outside the session directory.

Verification:

```text
cargo nextest run -p vtcode-memory
cargo nextest run -p vtcode-core -E 'test(/session_store_sink|event/)'
cargo nextest run -p vtcode -E 'test(/harness/)'
```
