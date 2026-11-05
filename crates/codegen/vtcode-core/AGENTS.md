<!-- markdownlint-disable MD013 -->
# vtcode-core

[Root AGENTS.md](../../../AGENTS.md) | Agent loop, tools, LLM orchestration, safety, prompts, and UI runtime shared by the binary and headless runners.

## Modules

Paths are under `src/`. `core/agent/` runtime, event recording, runner, request envelopes, completion assessment | `tools/` handlers, registry, policy, execution
`tools/registry/` admission, dispatch, results, history, completion verification | `tools/exec_session/` unified PTY/pipe registry and background slots | `tools/output_spooler/` bounded previews and spool references | `tools/source_inspection/` session-local source ledger
`llm/` re-export facade over `vtcode-llm` | `exec/events.rs` re-exports canonical `ThreadEvent` | `prompts/` static profiles and compiled runtime guidance | `compaction/` history compaction and memory envelopes | `subagents/matrix/` scheduler-owned workers | `mcp/`, `skills/` plugin MCP servers and skill policy/loading | `exec_policy/`, `command_safety/` separate policy layers | `notifications/` per-domain OS alerts | `utils/ansi/` renderer

## Rules

- Route every tool through `tools/registry`; `shell` and command aliases resolve to `unified_exec`, so never add a parallel dispatch path that bypasses the registry.
- Re-export public APIs from `lib.rs`, keep constants in `config::constants`, and keep providers in `vtcode-llm` and WebMCP transport/runtime in `vtcode-webmcp`.
- Session event sinks are authoritative: preserve ordering, fail closed on saturation, and write recovery/final messages through the canonical `ThreadEvent` path rather than history-only writes.
- Pipe, PTY, and MCP stdio children inherit filtered environment policy through the sandbox-aware launch APIs; never bypass them.
- Workspace-controlled lifecycle hooks gate the whole engine until the exact command-set digest is approved; build with `new_with_session_gated` and never add a bypass.
- `apply_patch` validates the turn cwd and every target against the session workspace with symlink-aware containment before mutation, and plugin MCP `command`/`cwd` are never re-resolved at spawn time.
- Request-time history normalization and tool-result clearing are request-only: never mutate durable session history or `ThreadEvent`s, and keep native provider replay windows unpruned.
- Completion requires terminal exit zero after the latest code mutation, required checks bypass result reuse and caches, and edits stay available while verification is pending.
- Treat learned `shell-pattern:` approval keys as opaque so a scoped approval never widens.
- Build and Auto are authority-parity destinations that share the same guards, safety checks, verification gate, budgets, and recovery limits.

## Gotchas

- Headless fallible work must run inside an awaited async block; an ordinary block does not contain `?` propagation, so errors would skip terminal recording and persistence close.
- Exec `wait`/`inspect` calls are turn-budget-exempt and counted in a separate control-plane bucket; do not re-couple them to the per-turn work budget.
- `AgentSessionState.messages` is `Arc<Vec<Message>>`; mutate via `messages_mut()`/`Arc::make_mut` so request and continuation histories stay shared.
- Async setup paths must use Tokio filesystem APIs or `spawn_blocking` for synchronous scans, persistence, or Git work.
- Force-cancel retains a session when direct-child termination is unconfirmed, because direct-child exit does not prove descendant cleanup.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-core.md) — read the section for the subsystem you change before editing it.
