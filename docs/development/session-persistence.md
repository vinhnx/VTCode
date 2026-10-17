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
