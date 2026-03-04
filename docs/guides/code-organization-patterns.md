# VT Code Code Organization Patterns

This guide adapts the Rust code organization patterns from Codex DeepWiki section `8.3`
for VT Code's workspace and runtime architecture.

It is intentionally pragmatic: use these rules when adding or changing code in
`vtcode-core/` and `src/`.

## Scope Rules

### Session-Scoped State

Session-scoped state lives across many turns and should only store stable data:

- Configuration snapshots and feature flags
- Managers, registries, and shared clients
- Long-lived caches with clear invalidation

In VT Code, this maps to components like shared runloop/session configuration and
global managers in `vtcode-core`.

### Turn-Scoped State

Turn-scoped state should be created for one user turn and dropped at turn end:

- Tool call runtime data
- Streaming/transient response state
- Turn-local diagnostics and temporary buffers

Rule: avoid adding turn-specific fields into long-lived session structs. Create
a per-turn struct and pass it through execution functions.

## Shared Ownership Rules

Use `Arc<T>` for immutable or externally synchronized shared state.

Use `Arc<Mutex<T>>` or `Arc<RwLock<T>>` only when shared mutable access is
required.

For background tasks that should not keep parent state alive forever, prefer
`Arc::downgrade()` and exit when upgrade fails.

## Background Task Lifecycle

Every long-lived spawned task must have explicit lifecycle ownership:

- Store a `JoinHandle` in the owning struct
- Expose a shutdown path (`CancellationToken` or channel)
- Abort or await task completion on shutdown/drop

This avoids task leaks and makes shutdown behavior deterministic.

## Channel Boundaries

Use channels to isolate producers and consumers instead of sharing mutable state:

- Bounded channels for inbound work queues (backpressure)
- Unbounded channels for non-blocking event fanout only when justified
- `oneshot` for request/response handshakes

Document channel capacity decisions close to the type definition.

## Error Boundary Conventions

- Internal functions: `anyhow::Result<T>` with `.with_context(...)`
- Boundary APIs (UI/protocol/tool surface): use domain-specific error mapping
- Never `unwrap()` in production paths

This keeps errors debuggable internally and predictable externally.

## File Organization

When editing large modules:

- Keep public API and entry points near the top
- Keep private helpers lower in the file
- Split files when they mix unrelated concerns

Rule of thumb: if understanding a change requires unrelated sections, split the
module.

## Applied in VT Code

As part of adopting these patterns, `AsyncToolPipeline` now explicitly owns and
cleans up its background processing task:

- Tracks spawned processing `JoinHandle`
- Rejects duplicate starts
- Awaits task completion during shutdown
- Aborts lingering task on drop

See: `vtcode-core/src/tools/async_pipeline.rs`.

