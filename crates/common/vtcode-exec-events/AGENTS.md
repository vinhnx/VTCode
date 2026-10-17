<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-exec-events

[Root AGENTS.md](../../../AGENTS.md) | Authoritative `ThreadEvent` contract. All runtime events flow through this crate.

## Key Types

`ThreadEvent` enum — the single event type (serde-tagged) | `VersionedThreadEvent` wrapper with schema version | `EventEmitter` trait | `Usage` token accounting | `ThreadItem` + `ThreadItemDetails` item taxonomy | `EVENT_SCHEMA_VERSION` semver string

## ThreadEvent Variants

`thread.started` | `thread.completed` | `thread.compact_boundary` | `context.reset` | `turn.started` | `turn.completed` | `turn.failed` | `turn.blocked` | `item.started` | `item.updated` | `item.completed` | `background_subprocess_completed` | `plan.delta` | `plan.approval.requested` | `plan.approval.resolved` | `matrix.updated` | `error`

## Rules

- **Do not invent parallel event types.** Extend `ThreadEvent` and `ThreadItemDetails` enums. `EVENT_SCHEMA_VERSION` must be bumped when the serialized contract changes. `EventEmitter` trait has a blanket `FnMut(&ThreadEvent)` impl. Feature-gated emitters: `telemetry-log` (LogEmitter), `telemetry-tracing` (TracingEmitter), `schema-export` (JSON Schema), `serde-json` (JSON helpers). `atif/` exports ATIF trajectories; `trace/` implements Agent Trace attribution.
- **Keep `ThreadEvent` compact**: large sparse payloads must be `Box`ed (see `thread_event_stays_compact` size-guard test; ≤80 bytes). `Box<T>` is serde/schema transparent.

## Gotchas

- `vtcode-core::exec::events` re-exports these types — consumers should use that path, not depend on this crate directly.
- Plan approval state is `PlanApprovalRequested/Resolved`; keep `PlanApprovalDecision` stable for headless/Open Responses clients. Bounded failures use `ReasoningItem` stage `"diagnosis"`; no parallel variant. `HarnessEventKind` additions need a schema bump.
- Schema history: `0.12.0` blocked-handoff metadata; `0.13.0` `turn.blocked` + fuse counters; `0.14.0` limit-grant harness kinds; `0.15.0` optional `turn.completed.in_progress_exec_sessions`; `0.16.0` background completion harness identity fields, which ATIF must preserve in step `extra`; keep legacy readable and ATIF stable.
- Schema `0.17.0` adds optional task/turn/item context, input origin, timestamps, command activity, and `Decision`. Preserve legacy decoding and exporter metadata. Sparse context and reasoning payloads remain boxed for the size guard.
- Schema `0.18.0` adds boxed `matrix.updated` checkpoints; `matrix.rs` owns specifications, assignments, attempts, and generation-bound command evidence. Runtime identities and cleanup confirmations belong to the scheduler, never worker-selected report fields.
