<!-- markdownlint-disable MD013 -->
# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, agent runloop wiring, and the headless desktop-server JSON-RPC service, whose stdout is protocol-only with internal tags stripped.

## Modules

`main.rs` entry | `cli/` commands, WebMCP, companion exec | `startup/` onboarding, `StartupPolicy` | `updater/` self-replacement, release notes | `main_helpers/` tracing/init | `codex_app_server/` bridge | `allocator.rs`, `process_hardening.rs`
`agent/runloop/` single-agent loop: `session_loop_runner/` facade, `orchestration/` loop body/setup/teardown, `turn_loop/` request-boundary compaction + ordered finalization, `session_setup/hook_approval.rs` hook overlay
`agent/runloop/tool_output/` tracker/diff | `unified/turn/compaction/` delegates to core | `unified/webmcp.rs` bridge | `unified/instruction_audit.rs` `/doctor prompt-audit`

## Rules

- Keep the binary thin; runtime logic, spool previews, and shell classification belong in `vtcode-core`; binary serializes typed references. Rendering: `vtcode-tui-runtime`.
- Compare identity strings via canonical consts: `ThreadEvent::wire_name()`/`wire_type` for wire types, `config::constants::tools` for tool names, `vtcode_commons::provider` for providers. No raw identity literals.
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
- CLI `/desktop` hands the desktop app a fork, never its live thread id: the session's exclusive lock is held for the whole session, so a resume-in-place handoff always fails with `SessionBusy`.
- Desktop-server startup is auth-tolerant: missing credentials are recorded in `provider_auth_error`, never fatal.
- Async checkpointing acknowledges consumed steering intents only after a `Persisted` history result, never after a throttled checkpoint.
- The stop signal carries a force bit: a forced stop widens exec-session teardown to every session where the stop is observed, while the idle escape hatch queues only (an unhandled stop signal belongs to the next submitted turn).
- A turn still active 5s after cancel — or immediately on a forced repeat — escalates to force-stop and restart from history, and unconfirmed termination holds `session_transitioning` instead of reporting a stop.
- `session/recent_prompts` serves Everywhere/Project scopes from archives via `clean_archived_prompts`; Session answers empty since the client's live thread owns it.
- A hard budget stop (wall-clock, USD; legacy loop-budget vocabulary) is a handoff, never an auto-continue trigger: tracker and planning continuation gates stay shut and only explicit user input resumes the work.

Details: [maintainer notes](../docs/development/maintainer-notes/vtcode-binary.md) — read the section for the subsystem you change before editing it.
