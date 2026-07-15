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

Prefer plain ownership and borrowing first. Shared ownership is an opt-in
runtime cost, not the default shape for VT Code state.

Use `Rc<T>` only for single-threaded graphs, callbacks, or other cases where
multiple owners genuinely need to keep a value alive.

Use `Arc<T>` for immutable or externally synchronized shared state that crosses
tasks or threads.

Use `Rc<RefCell<T>>`, `Arc<Mutex<T>>`, or `Arc<RwLock<T>>` only when shared
mutable access is required; keep the mutable surface area small and explicit.

For back-references or background tasks that should not keep parent state alive
forever, prefer `Weak<T>` / `Arc::downgrade()` and exit when upgrade fails.

## FFI and Process-Handle Field Lifetimes

Foreign-function and OS-handle fields need their ownership made explicit in the
type system and in field comments, because the compiler cannot check them:

- Every `#[repr(C)]` field that holds a raw pointer (`*const`/`*mut`) must name,
  in a doc comment, which party owns and frees the pointee. A raw-pointer field
  with no named free owner is a latent leak — treat it as dead code and remove it.
- When a struct borrows validity from another resource (for example a raw
  function pointer copied out of a loaded `Library`), the field comment must
  state the lifetime invariant: the resource outlives `self`, and the struct has
  no `Drop` of its own when cleanup is delegated to that resource's `Drop`.
- Prefer delegating cleanup to an existing `Drop` (RAII) over manual free calls.
  A guard/handle type that drops its OS resource on scope exit is correct even
  across `?` early returns and panics.
- Centralize each `unsafe` FFI passage (symbol lookup, pointer decode) into one
  small safe wrapper with a single `// SAFETY:` note, so the audit surface is
  one place rather than scattered call sites.

`vtcode-skills/src/native_plugin.rs` (`NativePlugin`, `get_plugin_symbol`) and
`vtcode-bash-runner/src/process.rs` (`ProcessHandle`, `PtyHandles`) are the
reference implementations of these rules.

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

As part of adopting these patterns, the tool execution pipeline now explicitly owns and
cleans up its background processing task:

- Tracks spawned processing `JoinHandle`
- Rejects duplicate starts
- Awaits task completion during shutdown
- Aborts lingering task on drop

See: `vtcode-core/src/tools/exec_session.rs`.

