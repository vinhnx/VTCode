# VT Code Async Documentation (Retired)

`ASYNC_ARCHITECTURE.md` was retired on 2026-09-13: its December 2024 content duplicated and contradicted the canonical
guide (stale file paths, superseded patterns, no task-ownership or cancel-safety rules).

The authoritative reference is now:

- **[Async Architecture Guide](../guides/async-architecture.md)** — Tokio runtime, event loop, task
  extent/error-propagation/cancel-safety rules, actor pattern, and pipeline decision criteria.
- [Architectural Invariant #21](../harness/ARCHITECTURAL_INVARIANTS.md) — every spawned task has an owner.
- [Code Organization Patterns](../guides/code-organization-patterns.md) — background task lifecycle.
