# vtcode-desktop-protocol

[Root AGENTS.md](../../../AGENTS.md) | Internal wire contract for the desktop app and local service.

## Conventions

- Keep messages typed and JSON-RPC framing bounded.
- Preserve opaque interaction option identities across the wire.
- Keep runtime history/events sourced from `vtcode-exec-events`.

## Dependencies

- `serde` and `serde_json` (wire encoding)
- `tokio` (bounded asynchronous stdio framing)
- `vtcode-exec-events` (canonical runtime event payloads)
