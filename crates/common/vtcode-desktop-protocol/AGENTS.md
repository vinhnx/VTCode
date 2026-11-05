# vtcode-desktop-protocol

[Root AGENTS.md](../../../AGENTS.md) | Internal wire contract for the desktop app and local service.

## Conventions

- Keep messages typed and framing bounded; Git review echoes request/workspace identities to reject stale snapshots.
- Preserve opaque interaction option identities across the wire. `sharedUi` carries only non-secret CLI preferences.
- For `interaction/respond`, validate without consuming, enqueue to the active runtime, then resolve; failed delivery
  must leave it retryable.
- `sessionOptions` are typed idle-session selections; `availableModes` holds CLI-discovered primary-agent metadata.
  `FullAccess` needs one-shot `fullAccessConfirmed` (desktop consent modal only); the service rejects unattested
  requests and never merges the attestation into stored options. Resource names are bounded CLI-safe tokens.
- Primary-agent display notifications are ephemeral and session-scoped; keep configured `sessionOptions.mode` separate.
- `config/theme` accepts a built-in CLI theme ID. Settings DTOs carry bounded dotted keys and JSON values, never
  config file paths, full TOML, or credentials.
- `config/secret/store` is the one credential-carrying command: the key travels the local channel only, responses and
  errors carry provider labels, and `SecretStoreParams` redacts the key in `Debug`.
- Use `vtcode-exec-events` history types; review/activity snapshots never replay live lifecycle events.
- Hidden model-only history rows retain archive indices; only system rows may be omitted from the visible transcript.
- Shared scalar wire values (method names, sentinel ids like `SESSION_START_TURN_ID`) live as `pub const` here
  so the service and client reference one definition.
- The live event channel (`live_event_channel`) coalesces under saturation: snapshots replace per item,
  plan deltas merge, ordered events are only dropped when nothing coalescable remains. A slow consumer
  must never fail the producing turn; only a dropped consumer half is fatal.

## Dependencies

- `serde` and `serde_json` (wire encoding)
- `tokio` (bounded asynchronous stdio framing)
- `vtcode-exec-events` (canonical runtime event payloads)
