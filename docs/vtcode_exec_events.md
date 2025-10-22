# `vtcode-exec-events`

The `vtcode-exec-events` crate packages the execution telemetry schema that powers VTCode's
automation timeline. Downstream applications can depend on this crate to deserialize
thread progress, command execution lifecycle updates, and tool activity without pulling in
the rest of `vtcode-core`.

## Event taxonomy

Events are serialized with `serde` using an externally tagged `ThreadEvent` enum. Each
variant captures a specific moment in the lifecycle of an execution thread:

- **Thread lifecycle** – `thread.started`, `turn.started`, `turn.completed`, and
  `turn.failed` events communicate coarse-grained scheduling milestones including token
  accounting through the `Usage` struct.【F:vtcode-exec-events/src/lib.rs†L12-L63】
- **Timeline items** – item events wrap a `ThreadItem` snapshot with stable identifiers so
  consumers can correlate incremental updates (`item.started`, `item.updated`) with their
  eventual completion (`item.completed`).【F:vtcode-exec-events/src/lib.rs†L65-L120】
- **Command execution** – `CommandExecutionItem` reports the command string, aggregated
  output, exit status, and a tri-state `CommandExecutionStatus` so dashboards can surface
  long-running processes and failures.【F:vtcode-exec-events/src/lib.rs†L122-L160】
- **File changes** – `FileChangeItem` enumerates per-file updates with a
  `PatchApplyStatus` outcome to indicate whether patches landed cleanly.【F:vtcode-exec-events/src/lib.rs†L162-L200】
- **Tooling and search** – `McpToolCallItem` and `WebSearchItem` track external tool
  invocations and provider metadata, including optional arguments and raw result payloads
  for audit trails.【F:vtcode-exec-events/src/lib.rs†L202-L240】
- **Errors** – `ThreadErrorEvent` and `ErrorItem` record terminal failures alongside the
  human-readable messages surfaced to operators.【F:vtcode-exec-events/src/lib.rs†L41-L44】【F:vtcode-exec-events/src/lib.rs†L242-L248】

The schema favors additive evolution: new fields default via `Option` or `#[serde(default)]`
so older consumers continue to deserialize previously known structures.

## Versioning and compatibility

The crate follows semantic versioning. Backwards-compatible additions (such as new event
variants or optional fields) result in a minor version bump, while any breaking schema
changes trigger a major release. The `ThreadEvent` enum is stringly tagged using
snake-cased identifiers (for example, `item.updated`), which makes diffing JSON logs and
indexing metrics straightforward. Consumers should pin to compatible minor versions when
building dashboards to avoid accidental breakage.

Every serialized stream can opt into metadata that advertises the schema version through
the `VersionedThreadEvent` wrapper and the `EVENT_SCHEMA_VERSION` constant. Downstream
pipelines can gate deserialization or migrations using this metadata before materializing
individual events.【F:vtcode-exec-events/src/lib.rs†L8-L44】【F:vtcode-exec-events/src/lib.rs†L270-L308】

## Feature flags

`vtcode-exec-events` ships with feature toggles so applications can pick the supporting
infrastructure they need:

- `serde-json` (default) – enables helper functions for serializing/deserializing events
  to JSON strings or values while preserving the existing ergonomic API.【F:vtcode-exec-events/src/lib.rs†L70-L92】
- `telemetry-log` – provides a ready-made `LogEmitter` that writes JSON payloads through
  the `log` facade so adopters can reuse existing logging pipelines.【F:vtcode-exec-events/src/lib.rs†L94-L133】
- `telemetry-tracing` – exposes a `TracingEmitter` that emits structured events tagged
  with the schema version, integrating directly with the `tracing` ecosystem.【F:vtcode-exec-events/src/lib.rs†L135-L187】
- `schema-export` – produces JSON Schema documents for both raw and versioned events so
  downstream services can validate telemetry payloads offline.【F:vtcode-exec-events/src/lib.rs†L189-L200】

## Integrating with VTCode runtimes

`vtcode-core` re-exports the schema and emits events from the `ExecEventRecorder` inside the
agent runner. You can attach an event sink by supplying a closure when constructing the
runner, enabling streaming telemetry or custom logging pipelines.【F:vtcode-core/src/core/agent/runner.rs†L42-L87】 The recorder
invokes the sink for every serialized `ThreadEvent`, including streaming message deltas and
command status transitions.【F:vtcode-core/src/core/agent/runner.rs†L89-L154】

A minimal consumer might look like the following:

```rust
use std::sync::{Arc, Mutex};
use vtcode_core::core::agent::runner::AgentRunnerBuilder; // hypothetical helper
use vtcode_exec_events::ThreadEvent;

let sink = Arc::new(Mutex::new(Box::new(|event: &ThreadEvent| {
    println!("{}", serde_json::to_string(event).expect("serialize"));
}) as Box<dyn FnMut(&ThreadEvent) + Send>));

let runner = AgentRunnerBuilder::default()
    .with_event_sink(sink)
    .build()?;

runner.execute().await?;
```

Downstream services can forward the JSON payloads to observability stacks, persist them for
postmortems, or feed them into realtime dashboards.

The crate also publishes reusable emitters so integrations can skip wiring boilerplate.
For example, the logging emitter can be attached directly to the runner:

```rust
use vtcode_exec_events::LogEmitter;

let mut emitter = LogEmitter::default();
runner.set_event_handler(move |event| emitter.emit(event));
```

When using the tracing feature, substitute `TracingEmitter::default()` to forward the
structured payloads into `tracing` subscribers with automatic schema-version tagging.

### Bridging `vtcode-bash-runner`

Enabling the `exec-events` feature in `vtcode-bash-runner` exposes an
`EventfulExecutor` wrapper that converts each shell invocation into
`ThreadEvent` updates. This lets downstream adopters reuse the same telemetry
vocabulary when commands run outside VTCode's core runtime—for example, when
shell actions are executed via the standalone runner in CI or automation
pipelines. Wrap any executor and provide an emitter to mirror command activity:

```rust
use vtcode_bash_runner::{BashRunner, EventfulExecutor, ProcessCommandExecutor};
use vtcode_exec_events::LogEmitter;

let executor = EventfulExecutor::new(ProcessCommandExecutor::new(), LogEmitter::default());
let policy = vtcode_bash_runner::AllowAllPolicy;
let mut runner = BashRunner::new(workspace_root.clone(), executor, policy)?;
runner.ls(None, false)?;
```

Each invocation emits `item.started` and `item.completed` events with aggregated
output and exit codes, so telemetry consumers receive the same stream whether
commands originate from VTCode's agent loop or the extracted runner crate.【F:vtcode-bash-runner/src/executor.rs†L358-L470】

## Examples

The repository ships with a runnable example that emits a short execution timeline and
exports each `ThreadEvent` as JSON Lines. Running the example prints both the serialized
events and a derived summary of completed commands:

```shell
cargo run -p vtcode-exec-events --example event_timeline
```

The source (`vtcode-exec-events/examples/event_timeline.rs`) demonstrates how to
construct timeline events, serialize them with `serde_json`, and post-process the
sequence to build command summaries outside of VTCode's runtime.

## Next steps

With schema metadata, feature-gated emitters, and documentation updates in place, the
`vtcode-exec-events` extraction tasks are complete. The remaining work in the component
extraction initiative is to execute the staged publishes outlined in the release plan and
close out the dependency bump follow-ups.
