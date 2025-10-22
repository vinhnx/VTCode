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

## Next steps

The next milestone is to publish runnable examples that demonstrate emitting and consuming
execution events in isolation (for example, a CLI recorder or a WebSocket forwarder).
Once those examples land, the `vtcode-exec-events` extraction will be complete.
