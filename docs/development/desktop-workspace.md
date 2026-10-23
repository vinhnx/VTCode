# VT Code Desktop Workspace

VT Code Desktop is a macOS 15+ GPUI Kit application backed by the matching local `vtcode desktop-server` executable. The
GUI sends typed JSON-RPC commands; the existing unified runloop remains responsible for provider calls, tool policy,
verification, cancellation, and durable session events. The desktop service does not initialize a terminal or emit
terminal presentation output to its protocol stream.

## Build and launch

For local development, build the service and launch the GUI from the workspace root:

```sh
cargo build --locked -p vtcode
cargo run --locked -p vtcode-desktop
```

Choose a workspace folder in the app. The desktop client starts `vtcode desktop-server` with an absolute workspace path
and uses the existing VT Code configuration, provider credentials, workspace trust, and sandbox policy. If startup
reports missing credentials, configure the provider from Terminal with the command shown by the app, then restart it.
Credential entry and OAuth onboarding are not part of this version.

To make a local application bundle on macOS 15 or newer:

```sh
scripts/package-desktop-macos.sh
```

The optional first argument selects the output `.app` path. The script builds the release GUI and matching service,
places the service in `Contents/Helpers`, signs both executables before signing the app, and verifies the bundle. The
default output is `dist/VT Code.app`; the script will not overwrite an existing output. Ad hoc signatures support local
development only. They do not establish Developer ID trust or notarize a downloaded app; see [Apple Developer ID](https://developer.apple.com/developer-id/).

The GUI crate is non-publishable and outside Cargo's default members. It uses the pinned [`gpui-kit` 0.8.0 setup](https://gpui-kit.com/docs/installation/).

## Runtime and session behavior

The app owns one selected workspace, one local service process, and at most one active turn. Workspace or session changes
wait for active work to cancel and its runloop to clean up. Closing the app or disconnecting the protocol also cancels
owned work and pending interactions.

Session history is discovered from VT Code's existing durable stores. Selecting a previous session explicitly loads its
bounded text projection and resume state. A restart does not replay a submitted prompt or an approval automatically.
The transcript follows new output while it is at the latest position and preserves the reader's position after they
scroll upward.

## UI quick reference

| Action | Behavior |
| --- | --- |
| Read streamed output | GPUI measures variable-height rows and follows the bottom of the newest message, including a message taller than the viewport. |
| Scroll upward | Tail following pauses; returning to the bottom resumes it. Older rows evicted by the history bound preserve the remaining reader anchor. |
| Expand tool details | The changed row is remeasured while the reading position is preserved. |
| Copy a message or code block | Message copy retains the original Markdown; code copy contains only the code. Markdown formatting, links, lists, and tables render through GPUI Kit. |
| Follow a link | Explicit activation opens HTTP/HTTPS links. Other URL schemes are blocked, images are not fetched, and raw HTML is removed from rendered Markdown. |
| Choose a workspace | One asynchronous native picker opens at a time. Selection is rejected if a turn started or service cleanup is pending before the picker returns. |

The UI drains at most 64 service events per foreground tick so a busy stream yields to input. Row data and GPUI's
measured list state are maintained together; streaming text, expansion, theme changes, and viewport resizing invalidate
the affected geometry. No estimated character count is used for transcript row heights.

Approval, plan approval, and question panels return the exact opaque option identities supplied by the service. Wizard
steps retain their order and collect free-form answers separately from display labels. The GUI does not create its own
agent loop or bypass VT Code's approval and execution policy.

## Local service protocol

`vtcode desktop-server` uses one UTF-8 JSON-RPC object per line on stdin/stdout. Diagnostics go to stderr. The client and
server negotiate protocol version `1.0`; incompatible ranges fail explicitly. The protocol crate caps a line at 1 MiB.
The runtime-event, service-notification, host-command, and desktop-request queues each hold at most 128 entries; the
GUI event bridge holds at most 256 events. Full queues apply backpressure to their producer.

| Method | Purpose |
| --- | --- |
| `desktop/initialize` | Negotiate protocol compatibility and confirm the selected workspace. |
| `config/discover` | Read active provider, model, and startup credential-preflight status. |
| `session/list`, `session/create`, `session/load`, `session/close` | Select, create, and restore durable sessions. |
| `turn/submit`, `turn/cancel` | Submit one user turn or cancel its exact session/turn identity. |
| `interaction/respond` | Resolve one pending approval or question using its session, turn, and interaction IDs. |
| `desktop/shutdown` | Cancel and clean up owned work before exit. |

Notifications stream the canonical `VersionedThreadEvent` from `vtcode-exec-events` along with interaction requests and
turn status. A client must reject stale or duplicate interaction responses. EOF triggers service cleanup. The full wire
types and framing helpers live in [`vtcode-desktop-protocol`](../../crates/common/vtcode-desktop-protocol/README.md).

The separation between client commands, streamed events, and interaction responses follows the public shape of the
[pinned Codex app-server protocol](https://github.com/openai/codex/blob/89461ea6833832b0551ce24ee32205824d5c108d/codex-rs/app-server-protocol/src/protocol/common.rs).
This reference informs the protocol boundary only; the desktop UI and runtime integration are VT Code-specific.

## Scope

The first desktop release is local and macOS-only. It has no embedded editor, terminal, remote service, automatic
updater, concurrent turns, GUI credential setup, or full parity with every TUI command.
