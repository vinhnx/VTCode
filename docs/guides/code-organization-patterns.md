# VT Code Code Organization Patterns

This guide adapts the Rust code organization patterns from Codex DeepWiki section `8.3` for VT Code's workspace and
runtime architecture.

It is intentionally pragmatic: use these rules when adding or changing code in `vtcode-core/`, `vtcode-llm/`, and `src/`.

## Scope Rules

### Session-Scoped State

Session-scoped state lives across many turns and should only store stable data:

- Configuration snapshots and feature flags
- Managers, registries, and shared clients
- Long-lived caches with clear invalidation

In VT Code, this maps to components like shared runloop/session configuration and global managers in `vtcode-core`.

### Turn-Scoped State

Turn-scoped state should be created for one user turn and dropped at turn end:

- Tool call runtime data
- Streaming/transient response state
- Turn-local diagnostics and temporary buffers

Rule: avoid adding turn-specific fields into long-lived session structs. Create a per-turn struct and pass it through
execution functions.

## Shared Ownership Rules

Prefer plain ownership and borrowing first. Shared ownership is an opt-in runtime cost, not the default shape for VT
Code state.

Use `Rc<T>` only for single-threaded graphs, callbacks, or other cases where multiple owners genuinely need to keep a
value alive.

Use `Arc<T>` for immutable or externally synchronized shared state that crosses tasks or threads.

Use `Rc<RefCell<T>>`, `Arc<Mutex<T>>`, or `Arc<RwLock<T>>` only when shared mutable access is required; keep the mutable
surface area small and explicit.

For back-references or background tasks that should not keep parent state alive forever, prefer `Weak<T>` /
`Arc::downgrade()` and exit when upgrade fails.

## FFI and Process-Handle Field Lifetimes

Foreign-function and OS-handle fields need their ownership made explicit in the type system and in field comments,
because the compiler cannot check them:

- Every `#[repr(C)]` field that holds a raw pointer (`*const`/`*mut`) must name, in a doc comment, which party owns and
  frees the pointee. A raw-pointer field with no named free owner is a latent leak — treat it as dead code and remove
  it.
- When a struct borrows validity from another resource (for example a raw function pointer copied out of a loaded
  `Library`), the field comment must state the lifetime invariant: the resource outlives `self`, and the struct has no
  `Drop` of its own when cleanup is delegated to that resource's `Drop`.
- Prefer delegating cleanup to an existing `Drop` (RAII) over manual free calls. A guard/handle type that drops its OS
  resource on scope exit is correct even across `?` early returns and panics.
- Centralize each `unsafe` FFI passage (symbol lookup, pointer decode) into one small safe wrapper with a single
  `// SAFETY:` note, so the audit surface is one place rather than scattered call sites.

`crates/codegen/vtcode-skills/src/native_plugin.rs` (`NativePlugin`, `get_plugin_symbol`) and
`crates/codegen/vtcode-bash-runner/src/process.rs` (`ProcessHandle`, `PtyHandles`) are the reference implementations of
these rules.

## Background Task Lifecycle

Every long-lived spawned task must have explicit lifecycle ownership:

- Store a `JoinHandle` in the owning struct
- Expose a shutdown path (`CancellationToken` or channel)
- Abort or await task completion on shutdown/drop

This avoids task leaks and makes shutdown behavior deterministic.

Every `tokio::spawn` has exactly one owner: awaited, guarded (Drop-abort or owned field), or _documented detached_ — a
comment stating why detachment is safe (bounded work, token/channel termination, observable outcome). Unawaited handles
discard panics and `Err` results silently, so detached tasks that can fail meaningfully must log or send their outcome.
Never spawn async cleanup inside `Drop`; cleanup that must happen before `std::process::exit` is awaited inline with a
timeout. Details and examples: "Task Extent, Error Propagation, and Cancel-Safety" in
`docs/guides/async-architecture.md`; invariant #21 in `docs/harness/ARCHITECTURAL_INVARIANTS.md`.

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

Rule of thumb: if understanding a change requires unrelated sections, split the module.

### Provider Protocol Boundaries

OpenResponses keeps its public provider type and constructors in
`crates/codegen/vtcode-llm/src/providers/openresponses/provider.rs`. Private children keep protocol-specific work local:

| Child module | Responsibility |
| --- | --- |
| `provider/native.rs` | Native request serialization and completed-response parsing |
| `provider/legacy.rs` | Chat Completions payloads and generation/streaming fallback |
| `provider/compaction.rs` | Standalone compact requests and canonical replay windows |
| `provider/provider_impl.rs` | Capabilities and the three native generation/streaming entrypoints |

The entrypoints share `send_native_request` for native HTTP dispatch and bounded error diagnostics. A 404 stays with
the caller so each entrypoint uses its matching legacy adapter. Other HTTP errors surface without protocol fallback.
Keep native parsing, legacy decoding, and compaction replay separate; they have distinct wire contracts.
Methods needed by sibling modules use `pub(super)` rather than expanding the provider's public API.

Verify dispatch boundaries with:

```sh
cargo nextest run --locked -p vtcode-llm -E 'test(providers::openresponses::)' --no-tests=fail
```

## Applied in VT Code

As part of adopting these patterns, the tool execution pipeline now explicitly owns and cleans up its background
processing task:

- Tracks spawned processing `JoinHandle`
- Rejects duplicate starts
- Awaits task completion during shutdown
- Aborts lingering task on drop

See: `crates/codegen/vtcode-core/src/tools/exec_session.rs`.

## Refactored Runtime Boundaries

- `src/agent/runloop/unified/turn/turn_loop.rs` retains request-loop policy;
  its private `compaction.rs` child coordinates request-boundary compaction and
  `finalization.rs` owns ordered turn outcomes and bounded exec cleanup.
- `crates/codegen/vtcode-core/src/core/agent/runner/runtime_prompt.rs` owns
  headless prompt-bundle construction and catalog-aware refresh. Task lifecycle,
  budgets, and cleanup remain in `execute.rs`; `runner/completion.rs` owns
  evaluator acceptance and shared pre/post verification assessment. Unchanged provider-visible
  identities retain the frozen envelope across catalog bookkeeping changes.
- `src/agent/runloop/unified/turn/session_loop_runner/orchestration/mod.rs` owns session transitions.
  `session_iteration.rs` runs one session (setup, hydration, interaction) and hands its
  `SessionTailContext` to the `session_teardown.rs` child, which runs the exit tail: terminal
  event, one bounded maintenance group, TUI/terminal restoration, exit summary, and the
  resume/`/new` restart decision. Teardown also owns bounded exit drains, completed-artifact
  cleanup, persistent-memory finalization, and subagent shutdown. Emit the terminal event
  before draining persistence; retain the existing ordering and error precedence in the tail.
  Completed-artifact filesystem cleanup runs on the blocking pool and is awaited before
  persistent-memory finalization, so tracker archival finishes before the next phase.
- `src/agent/runloop/tool_output/mod.rs` dispatches output. Its `tracker.rs` child owns
  tracker transcript rows, panel metadata, and rendering. Shared Git-diff detection stays
  in the dispatch module, and existing callers use its re-exported tracker helpers.
  Panel and compact transcript focus use the same typed-row selection helper; selecting
  the current transcript task does not require constructing the full panel vectors.
- `crates/codegen/vtcode-core/src/tools/registry/execution_history.rs` owns the bounded
  shared record deque and public facade. Its private children separate replay and
  invalidation (`replay.rs`), task telemetry (`telemetry.rs`), and repeated-call detection
  (`loop_detection.rs`). Preserve spool verification and read-shape checks during extraction.
- Headless normal and fallback tool execution share `record_successful_tool_result` in
  `core/agent/runner/tool_exec.rs`: output completion precedes exec-session correlation
  and file-change events. Invocation admission and completion remain with each caller.
