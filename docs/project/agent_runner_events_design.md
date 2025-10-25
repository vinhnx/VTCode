# Agent Runner Event Module Design

## Objective

Extract `ExecEventRecorder`, `ActiveCommand`, and `StreamingAgentMessage` from `vtcode-core/src/core/agent/runner.rs` into a dedicated module (`vtcode-core/src/core/agent/events.rs`). The module should provide a focused API for recording turn lifecycle events, streaming agent responses, command execution, file changes, and warnings.

## Design Principles

-   **Single Responsibility**: Encapsulate event bookkeeping away from the runner’s control flow logic.
-   **Thread Safety**: Maintain compatibility with optional `EventSink` callbacks guarded by `Arc<Mutex<...>>`.
-   **Non-blocking**: Event methods remain lightweight and avoid heavy computation or I/O.
-   **No Behavior Change**: Preserve existing event emission semantics to avoid downstream regressions.

## Proposed API Surface

```rust
pub struct ExecEventRecorder {
    thread_id: String,
    events: Vec<ThreadEvent>,
    event_sink: Option<EventSink>,
}

impl ExecEventRecorder {
    pub fn new(thread_id: impl Into<String>, event_sink: Option<EventSink>) -> Self;

    pub fn turn_started(&mut self);
    pub fn turn_completed(&mut self);
    pub fn turn_failed(&mut self, message: &str);

    pub fn agent_message(&mut self, text: &str);
    pub fn agent_message_stream_update(&mut self, text: &str) -> bool;
    pub fn agent_message_stream_complete(&mut self);

    pub fn reasoning(&mut self, text: &str);

    pub fn command_started(&mut self, command: &str) -> ActiveCommandHandle;
    pub fn command_finished(
        &mut self,
        handle: &ActiveCommandHandle,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        output: &str,
    );

    pub fn file_change_completed(&mut self, path: &str);
    pub fn warning(&mut self, message: &str);

    pub fn into_events(mut self) -> Vec<ThreadEvent>;
}

pub struct ActiveCommandHandle {
    id: String,
    command: String,
}
```

## Implementation Notes

-   Keep streaming buffer state internal; expose no mutable references to prevent misuse.
-   Convert current `finish` method to `into_events` to clarify ownership semantics.
-   Provide `#[cfg(test)]` helpers for constructing recorders without sinks.
-   Continue emitting `ThreadStarted` during constructor execution for parity with existing behavior.

## Testing Strategy

-   Unit tests verifying:
    -   Event ordering for basic turn lifecycle.
    -   Streaming updates coalesce into final message on `into_events`.
    -   Command start/finish produce expected `ThreadItem` structures.
    -   Warnings and file changes populate appropriate event variants.
-   Ensure tests avoid real sinks by using `None` or capturing sinks with channels.

## Migration Plan

1. Introduce new `events.rs` module with copied logic and minimal adjustments.
2. Update runner to use the new module, removing duplicated structs.
3. Ensure imports update accordingly and documentation references new location.
4. Run formatting and linting.
5. Execute targeted tests covering event workflows.

## Open Questions

-   Should `EventSink` type alias remain in runner module or move alongside recorder? (Proposal: relocate for cohesion.)
-   Are there additional consumers of `ExecEventRecorder` beyond runner that would benefit from public exposure? (Currently none, but documenting API allows future reuse.)
