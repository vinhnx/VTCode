# vtcode-exec-events

Structured execution telemetry event schema used across VT Code crates.

This crate exposes the serialized schema for thread lifecycle updates,
command execution results, and other timeline artifacts emitted by the
agent runtime. Downstream applications can deserialize these structures
to drive dashboards, logging, or auditing pipelines without depending on
the full `vtcode-core` crate.

## Modules

| Module | Purpose |
|---|---|
| `atif` | ATIF trajectory format for agent execution traces |
| `trace` | [Agent Trace](https://agent-trace.dev/) attribution for AI-generated code |
| `json` | JSON serialization/deserialization helpers (feature-gated) |
| `schema` | JSON Schema export via `schemars` (feature-gated) |

## Public entrypoints

- `VersionedThreadEvent` — schema-versioned event wrapper
- `ThreadEvent` — concrete event enum (started, completed, item updates, turn lifecycle, …)
- `EVENT_SCHEMA_VERSION` — current schema version (`"0.4.0"`)
- `EventEmitter` trait — sink for processing events

## Usage

```rust,ignore
use vtcode_exec_events::{ThreadEvent, VersionedThreadEvent, EVENT_SCHEMA_VERSION};

let event = ThreadEvent::ThreadStarted(/* … */);
let versioned = VersionedThreadEvent::new(event);
assert_eq!(versioned.schema_version, EVENT_SCHEMA_VERSION);

let json = serde_json::to_string(&versioned)?;
```

## Feature flags

| Flag | Description |
|---|---|
| `serde-json` (default) | JSON helpers in the `json` module |
| `telemetry-tracing` | `TracingEmitter` for the `tracing` crate |
| `telemetry-log` | `LogEmitter` for the `log` crate |
| `schema-export` | JSON Schema generation via `schemars` |

## API reference

See [docs.rs/vtcode-exec-events](https://docs.rs/vtcode-exec-events).
