# vtcode-desktop-protocol

Private, versioned JSON-RPC contract used between the VT Code desktop client
and its matching local `vtcode desktop-server` process.

The transport uses one JSON-RPC object per UTF-8 line, capped at 1 MiB. The
server and client negotiate a compatible protocol range, runtime events use
`VersionedThreadEvent` from `vtcode-exec-events`, and host/service queues are
bounded with backpressure.

This crate is an internal workspace dependency and is not published to
crates.io.

The wire boundary follows the general separation used by the [pinned Codex
app-server protocol](https://github.com/openai/codex/blob/89461ea6833832b0551ce24ee32205824d5c108d/codex-rs/app-server-protocol/src/protocol/common.rs):
clients submit typed commands, servers stream authoritative runtime events, and interaction requests receive separately
correlated responses. VT Code does not depend on or assume Codex's desktop UI implementation.

See the [desktop workspace guide](../../../docs/development/desktop-workspace.md) for the method list, local launch, and
packaging instructions.

