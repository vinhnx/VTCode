# vtcode-exec-events

[Root AGENTS.md](../AGENTS.md) | Authoritative `ThreadEvent` contract. All runtime events flow through this crate.

## Key Types

`ThreadEvent` enum — the single event type (serde-tagged) | `VersionedThreadEvent` wrapper with schema version | `EventEmitter` trait | `Usage` token accounting | `ThreadItem` + `ThreadItemDetails` item taxonomy | `EVENT_SCHEMA_VERSION` semver string

## ThreadEvent Variants

`thread.started` | `thread.completed` | `thread.compact_boundary` | `turn.started` | `turn.completed` | `turn.failed` | `item.started` | `item.updated` | `item.completed` | `plan.delta` | `error`

## Rules

- **Do not invent parallel event types.** Extend `ThreadEvent` and `ThreadItemDetails` enums.
- `EVENT_SCHEMA_VERSION` must be bumped on breaking schema changes.
- `EventEmitter` trait has a blanket `FnMut(&ThreadEvent)` impl.
- Feature-gated emitters: `telemetry-log` (LogEmitter), `telemetry-tracing` (TracingEmitter), `schema-export` (JSON Schema), `serde-json` (JSON helpers).
- `atif/` module exports ATIF (Agent Trace Interchange Format).
- `trace/` module implements Agent Trace spec for AI code attribution.

## Gotchas

- `vtcode-core::exec::events` re-exports these types — consumers should use that path, not depend on this crate directly.
- `HarnessEventItem` uses `HarnessEventKind` enum — adding variants requires schema version bump.
