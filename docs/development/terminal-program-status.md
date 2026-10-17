# Terminal program status

VT Code can report its activity and child tasks through the
[Program Status Protocol (OSC 7501)](https://www.superlogical.com/rex/docs/build/program-status).
See also Mitchell Hashimoto's rationale in
[A Terminal Protocol for Program Status (OSC 7501)](https://mitchellh.com/writing/program-status-osc7501):
the program that knows its state should report it over the pty, so inboxes and
terminals do not have to guess from window-title spinners or screen contents,
or integrate with per-inbox socket APIs.
Reporting is disabled by default. Enable it in any normal configuration layer:

```toml
[ui.program_status]
enabled = true
```

Valid live configuration reloads apply this setting. Disabling clears only the current session's owned subtree.
Reports go to the interactive TUI's stderr terminal stream, only when stdin, stdout, and stderr are terminals.
Headless, ACP, JSON, and redirected output remain silent. No terminal feature query or additional input reader is used.

## Adoption and existing telemetry

Keep reporting opt-in while terminal compatibility is being established. Conforming terminals ignore unknown OSC
sequences, but wire tests do not prove how a terminal, recorder, or multiplexer handles them. Terminal names and
`TERM` values alone do not establish support.

The adapter projects current UI/runtime facts. It does not write another history file or feed ATIF or the trajectory
logger. Keep these responsibilities distinct:

| Surface | Responsibility |
| --- | --- |
| `ThreadEvent` and canonical `events.jsonl` | Authoritative runtime event history |
| ATIF | Structured trajectory export for evaluation and interchange |
| Trajectory logger | Routing, tool, and diagnostic records |
| OSC 7501 | Bounded current terminal state with generic labels |

Do not read telemetry files or parse display strings to drive status. Actual interaction ownership belongs to wait
guards; execution outcomes come from runtime evidence. OSC 7501 is the machine-readable authority for external
inboxes; the window title (including Braille spinners such as `⠋`) remains human-only and must never be treated
as a status API. Delivery caches and restoration metadata are temporary
terminal bookkeeping, not another execution state machine. Keep desktop notification preferences independent:
a terminal can choose to notify on status changes, potentially duplicating VT Code's configured alerts. VT Code
may still send one-time OSC 9 / OSC 777 / bell attention signals for HITL waits; those are events, while OSC 7501
records are state. OSC 133 shell integration continues to mark prompt/command boundaries; it cannot describe what
the program is doing or whether it is stuck.

Before adding an automatic mode, verify Rex presentation and representative unsupported terminals, recorders,
multiplexers, and SSH paths. Positive terminfo `Pst` advertising can permit reports; its absence does not prove
unsupported status. A protocol query reply is authoritative. Any future query must share the existing terminal
input reader and preserve startup/input ordering. Keep explicit enable/disable overrides available.

## Lifecycle

| Runtime evidence | Terminal state |
| --- | --- |
| Accepted operation or foreground tool phase | `working`, indeterminate unless task metadata supplies `progress` |
| Task panel with typed `completed`/`total` metadata | `working` with `progress=0-100` (absent when idle, done, error, or unknown) |
| Actual approval, question, or authentication wait | `blocked`, with `permission`, `question`, or `auth` kind |
| Recovery handoff awaiting guidance | `blocked`, without a kind |
| Successful terminal turn | `done` |
| Terminal turn failure | `error` |
| Cancellation | `idle` |
| Queued, running, or waiting delegated agent | `working` |
| Completed or failed delegated agent | `done` or `error` |
| Background subprocess or command session with natural exit zero | `done` |
| Unexpected child failure | `error` |
| Intentional child termination, including nonzero exits, or unknown stopped outcome | `idle` |

Delegated `Waiting` includes queued input during execution and does not imply a user wait. Help screens, palettes,
and ordinary navigation do not report blocked. Wait guards use ownership tokens; submission, denial, cancellation,
deferral, errors, and dropped futures remove only their own wait and reveal the latest underlying state.

Each session owns one opaque parent ID and stable opaque child IDs keyed by task kind and source identity. Source
IDs are never copied into reports. The session reports at most 64 records, including the parent. Active children
take priority, followed by recently updated finished children, with deterministic identity ordering for ties.
Removed or displaced children are cleared. Previous executions cannot settle a restarted managed task.

Finished outcomes remain until new work replaces them or the terminal dismisses them. Normal shutdown preserves
finished records and retires unfinished ones before terminal restoration. Panic and signal cleanup is best effort
and idempotent. An unfinished parent with finished children is retired to idle so clearing it cannot erase them.

## Ownership and privacy

`vtcode-commons::program_status` owns the bounded encoder and typed projection commands. `vtcode-ui` owns records,
deduplication, terminal writes, and restoration cleanup. Runtime turn outcomes and registry snapshots supply status;
`ActivityState` and `ThreadEvent` retain their existing authority. The adapter does not enable input or execution.

Reports contain `app=vtcode` and generic phase/task labels. They exclude prompts, commands, paths, summaries,
previews, credentials, and exception text. Free text uses standard base64; controls and invalid identifiers are
rejected, directional overrides are stripped, and all protocol byte limits are enforced. `progress` is only sent
with `working` or `blocked` (never with `idle`, `done`, `error`, or `clear`) and is normally driven by typed
task metadata; child records remain indeterminate (no `progress`) until a typed percent source exists.
Out-of-range values are omitted on the wire and ignored by the adapter, retaining the last good value.

While disabled, the adapter keeps only parent lifecycle and wait ownership up to date. It projects children from the
existing Local Agents snapshot on enable, without sorting or hashing child records while off. Once owned clears
succeed, disabled reporting skips terminal locking and restoration bookkeeping; failed clears remain retryable.

Only changed complete replacement reports are written. There are no heartbeat, spinner, or elapsed-time reports.
Writes use the existing terminal-operation lock, failures log context and remain retryable, and writes stop once
terminal restoration is claimed. Terminal titles, progress rendering, desktop notifications, reduced motion,
and screen-reader settings remain independent.

## Verification

Run focused regressions with:

```sh
cargo nextest run --locked --config-file .github/nextest.toml -p vtcode-commons -p vtcode-ui -p vtcode-config -p vtcode -p vtcode-core -E 'test(program_status) | test(background_completion) | test(local_agents)'
```

PTY captures validate default-off silence, live enabling/disabling, cancellation, failure, wire ordering, and silence
when any standard stream is redirected. On macOS they also check that `/usr/bin/script` retains the reports before
restoration. These byte checks do not prove recorder playback presentation. Rex visual acceptance requires an
installed Rex terminal; unit and PTY checks alone do not prove its presentation behavior.

Manual acceptance should cover:

| Host path | Check |
| --- | --- |
| Rex | Unfocused tab and session picker reflect working, blocked, done, and error; cancellation becomes idle |
| Terminal without protocol support | Input, navigation, and restoration remain usable; no visible status escape text |
| Multiplexer and SSH | Check report forwarding and behavior at each boundary; do not infer support from `TERM` |
| Recorder | Inspect captured OSC bytes and verify playback behavior; generic labels remain the only report text |
| Reduced motion and screen reader | Existing static feedback and input behavior remain unchanged |
| Redirected stdin, stdout, or stderr | No OSC 7501 reports |

Record the terminal version and host path with results. Treat unavailable hosts as unverified rather than passed.
