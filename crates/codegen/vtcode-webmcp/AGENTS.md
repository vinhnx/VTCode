<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-webmcp

[Root AGENTS.md](../../../AGENTS.md) | Authenticated browser bridge and safe workspace adapter.

## Modules

- `protocol` — versioned browser/server messages. `pairing` — expiring one-time codes, sessions, origin binding, revocation, and atomic replacement. `event_hub` — bounded replay and slow-client handling for runtime events. `runtime` — adapter traits and result types used by active and headless sessions. `filesystem` — canonicalized, digest-checked headless workspace adapter using `vtcode-diff` for authoritative diffs. `remote_mcp` — authenticated read-only Streamable HTTP and legacy SSE transports.
- `server` — Axum WebSocket transport and request dispatch.
- `error.rs` — crate error types.

## Rules

- Never put pairing tokens in URLs, logs, or persistent storage.
- `VersionedThreadEvent` is the only runtime event payload accepted by the hub.
- Patch mutations require adapter authorization and a matching current digest; check authorization is a separate full-auto `exec_command` decision routed through `vtcode-safety`.
- Commands are argv-based and allowlisted; only bounded check invocations may run, and never invoke a shell.
- Browser listings, reads, and check sandboxes must apply the central component-sensitive-file exclusions. Filesystem access must use bound directory handles with no-follow traversal where supported; compare-and-replace must operate on the opened handle and fail closed when that guarantee is unavailable.
- The listener is loopback-only; remote access requires a TLS-terminating reverse proxy.
- Remote MCP accepts only proxy-injected internal bearer tokens; redact token values, keep its Origin policy separate, and never add a file-serving route.
- Multi-file mutations use serialized compare-and-rollback; preserve fail-closed behavior when rollback itself fails. Keep proposal count/byte budgets and subscriber count/byte budgets bounded.
- Active turn requests may carry a proposal identity; revalidate its stored snapshots and hand off the adapter-generated authoritative diff. Never trust or forward the browser-rendered diff as the source of truth.
- Pairing TTL is the one-time-code lifetime and authenticated-session inactivity lease; authenticated browser traffic may refresh it, and an in-flight authenticated operation may pin its lease until completion. Expiry checks outside an operation remain read-only.
- Multiple configured exact origins may share one listener; origin-specific pairing issues a new pending code without revoking existing sessions, while replacement revokes all sessions.
- Authenticated `status` may expose only non-secret bridge settings; the terminal/TUI remains authoritative for origins, roots, and policy. Keep browser settings refreshable from status/heartbeats without persisting tokens or pairing codes.
- Explanation operations share pairing/origin checks; advertise `explanations_available` only with a canonical adapter. Evidence references must remain session/digest-bound, pages bounded, and navigation review-only: never submit prompts, mutate files, or run commands. Headless adapters return `unsupported`.
