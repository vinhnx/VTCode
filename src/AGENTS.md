<!-- markdownlint-disable MD013 -->
# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, agent runloop wiring, and the headless desktop-server JSON-RPC service, whose stdout is protocol-only with internal tags stripped.

## Modules

`main.rs` entry | `cli/` command handlers, WebMCP serving, companion exec supervision | `startup/` onboarding and `StartupPolicy` | `updater/` downloads and self-replacement | `main_helpers/` tracing and runtime init | `codex_app_server/` bridge | `allocator.rs`, `process_hardening.rs` process setup
`agent/runloop/` single-agent loop: `unified/turn/session_loop_runner/` facade with `orchestration/` owning the loop body, setup, and teardown; `unified/turn/turn_loop/` request-boundary compaction and ordered finalization; `unified/session_setup/hook_approval.rs` workspace hook approval overlay
`agent/runloop/tool_output/` tracker rows, dispatch, and ANSI diff rendering | `agent/runloop/unified/turn/compaction/` delegates to `vtcode-core::compaction` | `agent/runloop/unified/webmcp.rs` active WebMCP bridge

## Rules

- Keep the binary thin; runtime logic, spool preview generation, and shell activity classification belong in `vtcode-core`, and the binary only serializes typed references.
- Use the shared `ThreadEvent` contract for telemetry; unexpected exits must emit terminal lifecycle events before draining, and `take_active_turn` is the single take point for terminal emission.
- Build `LifecycleHookEngine` with `new_with_session_gated` so workspace-controlled hooks stay gated behind the approval overlay.
- Verification gates completion only, never editing; verifier responses require a terminal exit code before clearing the gate, and verification credit is never derived from request-only projections.
- Request context is persisted once at a user-turn boundary and never rewritten, and the tool-free recovery directive is request-only so the cached prompt prefix stays byte-identical.
- Exit is session-terminal: check stop state before recovery, tool admission, and autonomous follow-ups.
- Updater asset URLs stay on HTTPS GitHub release paths, downloads never use API credentials, and missing or invalid checksums abort installation.
- Companion setup preserves operator CLI/config policy, never grants trust or inherits proxy credentials, and keeps raw stderr private.
- Internal harness follow-ups use `try_queue_synthetic_follow_up_input` and preserve recovery budgets; never classify them as fresh user requests.

## Gotchas

- Session load probes the exclusive event-log lock first and fails fast when another process holds the session.
- Desktop-server startup is auth-tolerant: missing credentials are recorded in `provider_auth_error`, never fatal.
- Async checkpointing acknowledges consumed steering intents only after a `Persisted` history result, never after a throttled checkpoint.
- A turn still active 5s after cancel escalates to force-stop and restart from history, and unconfirmed termination holds `session_transitioning` instead of reporting a stop.
- `session/recent_prompts` serves Everywhere/Project scopes from archives via `clean_archived_prompts`; Session answers empty since the client's live thread owns it.

Details: [maintainer notes](../docs/development/maintainer-notes/vtcode-binary.md) — read the section for the subsystem you change before editing it.
