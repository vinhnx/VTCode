# Copilot runtime ownership

The binary keeps one Copilot runtime host and private runtime adapters. These
live under `agent/runloop/unified/turn/turn_processing/llm_request/`.

| Owner | State and responsibility | Lifecycle boundary |
| --- | --- | --- |
| `copilot_runtime.rs`: `CopilotRuntimeHost` | Borrowed registry, UI/session state, permission caches, safety validator, hooks, harness budgets, and runtime request dispatch | The request renderer borrows the host; host drop aborts its remaining local terminal sessions |
| `copilot_runtime/terminal.rs`: local terminal sessions | Private terminal state and tasks, output/exit snapshots, observed-tool association, and the host's terminal create/output/release/kill/wait methods | Explicit release or host drop aborts remaining work; completion publishes terminal exit state |
| `copilot_runtime/observed.rs` | Private started/finished flags, previous output, command display, and Unicode output deltas | Updates return full output snapshots and one-time start/finish signals; the host emits harness events and accounts for failures |
| `copilot_runtime/presentation.rs` | Shared inline spinner, prepared progress reporter, PTY runtime, and output callback | Finish stops the spinner, drops the callback, completes progress, and schedules runtime shutdown with the caller's final color |
| `copilot_runtime/streaming.rs` | Prompt text/reasoning accumulation, finish-reason conversion, queued-update draining, and prompt cancellation guard | Polling installs the guard; dropping an active stream cancels the prompt; successful completion disarms it before emitting `Completed` |
| `llm_request/mod.rs` | Starts the prompt session, retains the runtime request receiver, and runs the shared streaming renderer | Startup interruption/first-progress timeout and renderer success/error remain request-owned |

The existing `prompt_session_to_stream` entrypoint delegates synchronously to the
private adapter, retaining its return shape and caller path. Runtime requests
remain on their separate receiver. The adapter does not acquire permission,
execute tools, or own terminal tasks.

The stream loop disables its update receive arm after channel closure. On prompt
completion, it drains queued updates before constructing the final response.
Reasoning normalization retains paragraph breaks, Unicode characters, closing
punctuation, and the spacing rules between chunks. Join/provider errors retain
the shared runtime error mapping.

The terminal module owns the existing per-session monitor context, polling,
bounded UTF-8 suffixes, exit notification, and inline terminal stream lifetime.
Its named output/completion/setup records replace private tuples. Session state
and task fields remain private; the host sees session handles and binding results.
Terminal handlers still execute through registry harness APIs. Release removes
the session, aborts its monitor, and closes its registry exec session; kill requests
termination; host drop retains its existing monitor-abort loop. This refactor does
not add an independent host or shutdown coordinator.

Observed calls and local terminals use the same private presentation helper.
The caller prepares progress: observed calls start with a fresh reporter, while
local terminals retain total/progress initialization and their elapsed-time guard.
The caller also maps status to color; an unknown local exit remains a warning.
Finishing still schedules completion and shutdown in a background task. Shutdown
drains accepted output before applying the final color; dropping an unfinished
presentation aborts its spinner/runtime without marking progress complete.

The observed adapter borrows only PTY configuration from the registry. Its state
fields remain private, with a name accessor and one update method. Harness output
events use full snapshots; the inline PTY callback receives only the computed
delta. Placeholder names can be enriched, repeated output is suppressed, and
terminal observed statuses finish presentation once. Local terminal association
continues through the terminal module before remote observed-state processing.

Permission handling, exposed-tool allowlists, tool budgets, hook rewrites,
sandbox-aware registry execution, verification guards, and harness-event emission
remain in the host. Splitting these areas requires tracing their shared state
and lifecycle exits first; do not create another runtime host or execution path.

Run binary runtime regressions and the existing provider cancel-handle regression:

```sh
cargo nextest run --locked -p vtcode -p vtcode-llm -E 'test(copilot_runtime) | test(prompt_session_cancel_handle) | test(inline_events)'
```

The adapter's unit tests cover conversion boundaries. They do not construct a
live `PromptSession` or exercise stream-drop/completion races end to end. The
provider regression verifies cancel notification, active-prompt cleanup, and
completion-task abortion separately. A live Copilot CLI/provider session remains
outside this refactor's validation.
