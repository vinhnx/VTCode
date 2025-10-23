# `vtcode-exec-events` Extraction Strategy

## Overview
`vtcode-exec-events` encapsulates the structured telemetry that VTCode emits while orchestrating autonomous execution threads. The existing module models thread lifecycle milestones, command execution state, file-change summaries, and tool invocations using serde-tagged enums and structs so events can be serialized to JSON streams or persisted for later analysis.【F:vtcode-core/src/exec/events.rs†L1-L123】【F:vtcode-core/src/exec/events.rs†L124-L200】 Extracting the schema into its own crate will let other agents, dashboards, and workflow engines consume or emit the same event vocabulary without depending on the full VTCode runtime.

## Extraction Goals
- Provide a lightweight crate that exports the event enums/structs with serde derives, ready for downstream serialization and storage layers.
- Establish a versioning and compatibility policy so breaking schema changes are communicated via semantic version bumps and explicit changelog entries.
- Offer optional integrations (feature-gated) for emitting events via logging, tracing, or custom telemetry sinks without forcing extra dependencies on consumers.
- Document integration patterns for both event producers (agents) and consumers (dashboards, log processors) to encourage consistent adoption.

## Schema Versioning and Stability
- Introduce a crate-level `EventSchemaVersion` constant and embed it in serialized payloads (e.g., top-level metadata event) so consumers can negotiate compatibility.
- Define guidelines for additive vs. breaking changes (e.g., new enum variants require feature flags until the next major release, field removals require a major bump).
- Maintain a migration guide that captures schema evolution alongside code examples, enabling downstream services to update parsers incrementally.
- Consider publishing JSON Schema artifacts (optional feature) to allow validation and schema registry workflows.

## Feature Flag Plan
- `serde_json` (default): re-exports helpers for converting events into JSON `Value` or string payloads, preserving current ergonomics.
- `telemetry-tracing`: enables integration adapters that convert events into `tracing` spans/events for applications already instrumented with `tracing`.
- `telemetry-log`: exposes log-style emitters that format events for structured logging sinks without pulling in tracing.
- `schema-export`: builds JSON Schema definitions and associated helpers so adopters can validate event payloads offline.

## Integration Points
- Provide an optional `EventEmitter` trait with blanket implementations for `Fn(&ThreadEvent)` closures so applications can plug in custom pipelines (message buses, sockets, etc.).
- Offer convenience adapters that map command execution events to the `vtcode-bash-runner` telemetry hooks, keeping cross-crate integrations aligned once both crates are published.
- Coordinate with `vtcode-commons` telemetry traits so workspace paths and error metadata can flow through a shared contract without cyclic dependencies.
- Document how to replay event streams into analytics dashboards or CLI viewers using the JSON representations.

## Testing and Tooling
- Add serialization round-trip tests to guarantee that events maintain backwards-compatible field names and serde tags across releases.
- Provide snapshot tests (feature-gated) that exercise representative execution flows, ensuring new variants or fields are reflected in fixtures before publishing.
- Ship examples illustrating both producing and consuming event streams:
  - `examples/emit_json.rs`: serializes events from a mock execution thread to stdout.
  - `examples/ingest_log.rs`: parses a JSONL log of events and aggregates command success metrics.
- Integrate schema validation (when the `schema-export` feature is enabled) into CI to catch accidental breaking changes.

## Migration Checklist
- [x] Extract the existing event types into the new crate while preserving serde tags and field names.
  - Migrated the schema to `vtcode-exec-events` and added a `VersionedThreadEvent` wrapper plus the `EVENT_SCHEMA_VERSION` constant so consumers can negotiate compatibility.
- [x] Publish emitter traits/adapters and align them with `vtcode-commons` telemetry hooks.
  - Introduced the `EventEmitter` trait with default closure support alongside optional `LogEmitter` and `TracingEmitter` implementations for logging/tracing stacks.
- [x] Document versioning guidelines, feature flags, and integration recipes for producers and consumers.
  - Refreshed `docs/vtcode_exec_events.md` with schema metadata guidance, feature flag descriptions, and emitter examples.
- [x] Provide examples and tests that cover serialization, deserialization, and schema validation workflows.
  - Added unit tests for versioned round-trips and left the existing JSONL example in place; schema exports are covered under the new `schema-export` feature.
- [ ] Release the crate with semantic versioning once downstream integrations are ready.
