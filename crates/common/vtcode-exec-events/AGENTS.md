<!-- markdownlint-disable MD013 -->
# vtcode-exec-events

[Root AGENTS.md](../../../AGENTS.md) | Authoritative `ThreadEvent` contract through which all runtime events flow.

## Modules

- `lib.rs` `ThreadEvent` (serde-tagged), `VersionedThreadEvent`, `EventEmitter`, `Usage`, `EVENT_SCHEMA_VERSION` | `items.rs` `ThreadItem`/`ThreadItemDetails`.
- `planning.rs` plan deltas, approval, recovery snapshots | `matrix.rs` matrix checkpoints | `diagnostic.rs` | `schema.rs`.
- `atif/` ATIF trajectory export | `trace.rs` Agent Trace attribution | feature-gated emitters in `log_support.rs`, `tracing_support.rs`, `json.rs`.

## Rules

- Do not invent parallel event types; extend `ThreadEvent` and `ThreadItemDetails` instead.
- Bump `EVENT_SCHEMA_VERSION` whenever the serialized contract changes, including `HarnessEventKind` additions.
- New fields stay optional and omitted when unset so legacy payloads keep decoding, and ATIF output stays stable.
- Keep `ThreadEvent` at or under 80 bytes by boxing large sparse payloads (guarded by `thread_event_stays_compact`).
- Redact diagnostics before persistence using the canonical severity/code/summary/details/identity/actions payload.
- Bounded failures use `ReasoningItem` stage `"diagnosis"`, never a parallel variant.
- Matrix runtime identities and cleanup confirmations belong to the scheduler, never to worker-selected report fields.
- Only explicit human input may call `PlanningRecoverySnapshot::begin_explicit_cycle`, and `PlanApprovalDecision` stays stable for headless/Open Responses clients.

## Gotchas

- Consumers should import through `vtcode-core::exec::events` rather than depending on this crate directly.
- New variants are unknown to old readers, so ATIF projects them as system steps with identity kept in step `extra`.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-exec-events.md) — read the section for the subsystem you change before editing it.
