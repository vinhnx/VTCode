# Terminal program status

VT Code can report its activity and child tasks through the
[Program Status Protocol (OSC 7501)](https://www.superlogical.com/rex/docs/build/program-status).
See also Mitchell Hashimoto's rationale in
[A Terminal Protocol for Program Status (OSC 7501)](https://mitchellh.com/writing/program-status-osc7501):
the program that knows its state should report it over the pty, so inboxes and
terminals do not have to guess from window-title spinners or screen contents,
or integrate with per-inbox socket APIs.
Reporting is on by default. Disable it in any normal configuration layer:

```toml
[ui.program_status]
enabled = false
```

Valid live configuration reloads apply this setting. Disabling clears only the current session's owned subtree.
Reports go to the interactive TUI's stderr terminal stream, only when stdin, stdout, and stderr are terminals.
Headless, ACP, JSON, and redirected output remain silent, as do terminals whose `VTE_VERSION` predates the parser
that safely ignores unknown OSC sequences (pre-0.54, mirroring Codex). No terminal feature query or additional
input reader is used.

## Adoption and existing telemetry

Reporting shipped opt-in until terminal compatibility was established, then moved to on-by-default for Codex parity:
conforming terminals ignore unknown OSC sequences, Codex emits unconditionally at scale with only a VTE-version
guard, and VT Code keeps the stricter all-streams-terminal gate plus the same VTE guard. Wire tests still do not
prove how every recorder or multiplexer handles the bytes, so the explicit `enabled = false` override stays
available. Terminal names and `TERM` values alone do not establish support.

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

Before the default-on change, Rex parsing was verified live (2026-10-31): vtcode-style child reports and
root-style reports round-tripped through a scratch Rex session's `program_status` API with correct
state/kind/progress/app/title/msg. Positive terminfo `Pst` advertising can permit reports; its absence does not
prove unsupported status. A protocol query reply is authoritative. Any future query must share the existing terminal
input reader and preserve startup/input ordering. Keep explicit enable/disable overrides available.

## Terminal pane icons (Rex/Ghostty)

Status records drive the working/blocked/completion chrome; the provider icon tile itself is resolved
terminal-side from a built-in program registry keyed by the OSC 7501 `app` id (and the foreground process
name). Rex ships icons for its known agents (Claude Code, Codex, OpenCode, …) but has no documented
custom-program-icon mechanism, so a `vtcode` tile appears only after Rex maps the id. VT Code's side of that
registration is fixed and ready:

- `app` id: `vtcode` (emitted on every report; see `encode_report`)
- Process names: `vtcode` (binary)
- Artwork: `resources/icons/vtcode-mono.svg` (monochrome V mark, white on transparent for dark terminal
  chrome), with `vtcode-mono-32.png` / `vtcode-mono-120.png` raster handoffs

When requesting the mapping upstream, point at this section plus the icon files (Rex is closed-source beta
with no public tracker; the channel is the private Rex Beta Discord). No CLI-side escape sequence can set
profile artwork; do not fork the adapter to spoof another program's id.

Copy-paste request body (verified against the spec 2026-11-02; emission is conformant, only the tile is missing):

```text
Please add a program-registry icon tile for VT Code (https://github.com/vinhnx/vtcode).

- OSC 7501 `app` id: `vtcode` (sent on every report; root title "VT Code")
- Foreground process name: `vtcode`
- Artwork (monochrome V mark, white on transparent for dark tab chrome):
  resources/icons/vtcode-mono.svg (vector source)
  resources/icons/vtcode-mono-32.png (32x32 handoff)
  resources/icons/vtcode-mono-120.png (120x120 handoff)
- Spec conformance: complete-replacement reports, opaque slash-scoped ids,
  kind/progress gated to blocked/working, base64 single-line title/msg with
  no controls, 4KB/64-record caps, no feature query (emit unconditionally,
  as the spec permits).

Today Rex tabs running VT Code show the generic terminal glyph; mapping
`vtcode` lights up the tile with no CLI-side change needed.
```

## iTerm2 session status (OSC 21337)

Direct iTerm2 sessions (no tmux/screen) additionally receive the proprietary session-status pill alongside the
7501 records, mirroring Codex: the parent state maps to `Idle` (green), `Working` (orange), or `Waiting` (blue)
with the same generic detail label, finished states read as idle, and restoration clears the pill. Children,
progress percents, and kinds stay 7501-only; the pill is one session-level status, deduplicated like the records.

## Tab progress (OSC 9;4)

Sessions on terminals documented to render ConEmu progress additionally receive `OSC 9;4` tab-progress reports
on the same stream, under the same `enabled` switch: a native tab ring/bar with no per-terminal install. The
parent state maps onto the ConEmu states ([Windows Terminal docs](https://learn.microsoft.com/en-us/windows/terminal/tutorials/progress-bar-sequences)):

| Runtime evidence | Tab progress |
| --- | --- |
| Working with typed task percent | `1` + value (`0-100`, clamped) |
| Working, indeterminate | `3` (indeterminate) |
| Actual approval, question, or authentication wait | `4` (paused — a wait, not a failure) |
| Terminal turn failure | `2` (error) |
| Idle, successful turn, disable, or shutdown | `0` (remove) |

Emission is allowlist-only, because everywhere else `OSC 9` may mean notifications: Windows Terminal
(`WT_SESSION`/`WT_PROFILE_ID`), ConEmu (`ConEmuPID`), or exactly `TERM_PROGRAM=ghostty` with
`TERM_PROGRAM_VERSION >= 1.2.0` (older Ghostty parses `OSC 9` as a notification, so unversioned or
unparsable versions fail closed). Unknown terminals never receive the sequence. Like the 7501 records,
only changed states are written and restoration removes the indicator.

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
cargo nextest run --locked --config-file .config/nextest.toml -p vtcode-commons -p vtcode-ui -p vtcode-config -p vtcode -p vtcode-core -E 'test(program_status) | test(background_completion) | test(local_agents)'
```

PTY captures validate explicit-disable silence, live enabling/disabling, cancellation, failure, wire ordering,
and silence when any standard stream is redirected. On macOS they also check that `/usr/bin/script` retains the
reports before restoration. These byte checks do not prove recorder playback presentation. Rex record parsing was
verified live through its `program_status` API; visual icon-tile acceptance needs the upstream registry mapping
described above.

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
