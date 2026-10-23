# vtcode-desktop-protocol

[Root AGENTS.md](../../../AGENTS.md) | Internal wire contract for the desktop app and local service.

## Conventions

- Keep messages typed and JSON-RPC framing bounded.
- Preserve opaque interaction option identities across the wire.
- Keep runtime history/events sourced from `vtcode-exec-events`.
- The live event channel (`live_event_channel`) coalesces under saturation: snapshots replace per item,
  plan deltas merge, ordered events are only dropped when nothing coalescable remains. A slow consumer
  must never fail the producing turn; only a dropped consumer half is fatal.

## Dependencies

- `serde` and `serde_json` (wire encoding)
- `tokio` (bounded asynchronous stdio framing)
- `vtcode-exec-events` (canonical runtime event payloads)
