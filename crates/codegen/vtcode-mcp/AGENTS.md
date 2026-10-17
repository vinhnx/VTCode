<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->

# vtcode-mcp

[Root AGENTS.md](../../../AGENTS.md) | Model Context Protocol client, connection pooling, and tool discovery. Layer 1 crate -- depends on vtcode-config, vtcode-commons, vtcode-utility-tool-specs.

## Module Groups

| Area      | Modules                                                     |
| --------- | ----------------------------------------------------------- |
| Client    | `client.rs`, `provider.rs`, `rmcp_client.rs`                |
| Transport | `rmcp_transport.rs`, `connection_pool.rs`                   |
| Discovery | `tool_discovery.rs`, `tool_discovery_cache.rs`, `schema.rs` |
| Types     | `types.rs`, `traits.rs`, `errors.rs`, `enhanced_config.rs`  |
| Infra     | `conversion.rs`, `sandbox_context.rs`, `trust.rs`           |
| Utils     | `utils.rs`                                                  |

## Rules

- `cli.rs` stays in vtcode-core (depends on `crate::cli::input_hardening`). Re-export facade in vtcode-core (`mcp/mod.rs`) must stay in sync. `rmcp_client` is `pub(crate)` -- not part of the public API. `convert_to_rmcp()` is `pub(crate)` -- internal JSON bridge. Treat discovered server text as bounded untrusted resource content; retain host policy around it and keep enforcement in registration/tool policy.

## Gotchas

- `enhanced_config.rs` uses `#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]`. `rmcp-reqwest` is a renamed `reqwest` with rustls features -- not the same as the workspace `reqwest`. `DEFAULT_ENV_VARS` is platform-conditional (`#[cfg(unix)]` / `#[cfg(windows)]`). `McpSandboxContext` is optional for unsandboxed `McpClient::new`; session setup must pass it through initial, pooled, and reconnect stdio launches, whose stderr is bounded and redacted.
- `list_changed` refreshes are burst-coalesced and token-bucket limited; throttled notifications must retain a dirty bit for one refresh after refill.
- LLM-visible tool/resource/prompt lists must be deterministic: sorted in `provider.rs` (`filter_*_sorted`), providers iterated via `providers_sorted_by_name` in `client.rs`; on name collisions the first provider by name wins.
