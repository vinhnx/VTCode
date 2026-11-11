<!-- markdownlint-disable MD013 -->

# vtcode-exec-events maintainer notes

Detailed contracts and gotchas for `crates/common/vtcode-exec-events/`, moved verbatim from its [AGENTS.md](../../../crates/common/vtcode-exec-events/AGENTS.md). Read the section for the subsystem you are changing.

[Root AGENTS.md](../../../AGENTS.md) | Authoritative `ThreadEvent` contract. All runtime events flow through this crate.

## Key Types

`ThreadEvent` enum — the single event type (serde-tagged) | `VersionedThreadEvent` wrapper with schema version | `EventEmitter` trait | `Usage` token accounting | `ThreadItem` + `ThreadItemDetails` item taxonomy | `EVENT_SCHEMA_VERSION` semver string

## ThreadEvent Variants

`thread.started` | `thread.completed` | `thread.compact_boundary` | `context.reset` | `turn.started` | `turn.completed` | `turn.failed` | `turn.blocked` | `item.started` | `item.updated` | `item.completed` | `app.sandbox_denied` | `background_subprocess_completed` | `plan.delta` | `plan.approval.requested` | `plan.approval.resolved` | `matrix.updated` | `tracker.updated` | `error`

## Rules

- **Do not invent parallel event types.** Extend `ThreadEvent` and `ThreadItemDetails` enums. `EVENT_SCHEMA_VERSION` must be bumped when the serialized contract changes. `EventEmitter` trait has a blanket `FnMut(&ThreadEvent)` impl. Feature-gated emitters: `telemetry-log` (LogEmitter), `telemetry-tracing` (TracingEmitter), `schema-export` (JSON Schema), `serde-json` (JSON helpers). `atif/` exports ATIF trajectories; `trace/` implements Agent Trace attribution.
- **Keep `ThreadEvent` compact**: large sparse payloads must be `Box`ed (see `thread_event_stays_compact` size-guard test; ≤80 bytes). `Box<T>` is serde/schema transparent.

## Gotchas

- `vtcode-core::exec::events` re-exports these types — consumers should use that path, not depend on this crate directly.
- `PlanningRecoverySnapshot.request_id` scopes headless checkpoints to their task. `PlanningRecoverySnapshot::admit_request` owns the shared three-request allowance across continuations/resume; only explicit human input may call `begin_explicit_cycle`, which preserves closed research and the retained draft. All `response_class` writers must use `PlanningRecoverySnapshot::record_response_class` so the previous/current shift stays exact for repeat detection. Plan approval state is `PlanApprovalRequested/Resolved`; keep `PlanApprovalDecision` stable for headless/Open Responses clients. Bounded failures use `ReasoningItem` stage `"diagnosis"`; no parallel variant. `HarnessEventKind` additions need a schema bump.
- Schema history: `0.12.0` blocked-handoff metadata; `0.13.0` `turn.blocked` + fuse counters; `0.14.0` limit-grant harness kinds; `0.15.0` optional `turn.completed.in_progress_exec_sessions`; `0.16.0` background completion harness identity fields, which ATIF must preserve in step `extra`; keep legacy readable and ATIF stable.
- Schema `0.17.0` adds optional task/turn/item context, input origin, timestamps, command activity, and `Decision`. Preserve legacy decoding and exporter metadata. Sparse context and reasoning payloads remain boxed for the size guard.
- Schema `0.18.0` adds boxed `matrix.updated` checkpoints; `matrix.rs` owns specifications, assignments, attempts, and generation-bound command evidence. Runtime identities and cleanup confirmations belong to the scheduler, never worker-selected report fields.

Schema `0.19.0` adds optional boxed `PlanDeltaEvent.recovery` request checkpoints and `ReasoningItem.diagnostic` (stage `diagnosis`); use the canonical severity/code/summary/details/identity/actions payload and redact before persistence. ATIF retains it in step extra. Schema `0.20.0` adds optional `tool_call_id` correlation fields to permission events; preserve omission for legacy callers and accept older payloads without the fields. Schema `0.21.0` adds `ToolOutputItem.app_previews` (`AppPreviewItem`/`AppUiPosture`, omitted when empty); legacy payloads without the field decode to empty. Schema `0.22.0` adds boxed `app.sandbox_denied` (tool + tier + capped URIs); unknown-to-old-reader variant, ATIF projects it as a system step. Schema `0.23.0` adds session-messaging harness kinds (`SessionMessageSent/Received/Expired`, unknown-to-old-reader variants) and optional `HarnessEventItem.correlation_id` (omitted when unset); ATIF projects them as system steps with session/correlation identity in step extra. Schema `0.24.0` adds optional `TurnCompletedEvent.final_response_item_id` for explicit correlation with the final agent message. Schema `0.25.0` adds optional `FileUpdateChange.terminators_normalized` (`None` for legacy events); exact byte reversal must refuse normalized or unverifiable recordings.

Schema `0.26.0` adds optional `ToolOutputItem.is_delta`: `true` marks an incremental update; absent or false means a snapshot. Completion output is a snapshot.

Schema `0.27.0` adds boxed `tracker.updated` (`TrackerSnapshot`: raw title, explicit-or-derived completed/total, ordered `TrackerItemState` rows with dotted index paths, short descriptions, and canonical statuses); unknown-to-old-reader variant, ATIF skips it like matrix checkpoints. Emitted from structured `task_tracker` output by `ExecutionContextTracker::tracker_status_event` via both recorder fan-outs.

Schema `0.28.0` adds optional `PlanningRecoverySnapshot.previous_response_class` (omitted when unset) for repeated-identical-failure detection; legacy checkpoints without the field decode to `None` (no false repeat).
