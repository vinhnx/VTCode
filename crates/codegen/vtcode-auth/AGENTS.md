# vtcode-auth

[Root AGENTS.md](../../../AGENTS.md) | OAuth PKCE flows and credential storage for LLM providers.

## Modules

- OAuth flows: `openai_chatgpt_oauth/`, `openrouter_oauth/`, `mcp_oauth/`, `oauth_server/` (callback), `pkce/`.
- Storage: `credentials/` (keyring + file), `openai_session_storage/`, `openrouter_token_storage/`,
  `storage_paths/`.
- Other: `auth_service/`, `config/`, `openai_refresh_policy/` (pure), `codex_auth_import.rs`.

## Rules

- All OAuth flows use PKCE via `generate_pkce_challenge()`.
- Canonical code is here; `vtcode-config::auth` only re-exports it.
- Use the `_with_mode` variants to pass the storage mode explicitly; keep writes and reads on the same mode.
- `openai_refresh_policy` returns an action only; deletion stays in the orchestration layer.

## Gotchas

- MCP OAuth tokens are bound to provider, config, and resource; unbound legacy tokens need reauthorization.
- Keyring is disabled in debug, tests, and CI (`VTCODE_DISABLE_KEYRING=0` re-enables it).
- API-key storage is keyed by `(provider, key_name)`; migrate provider-only entries only for the default key.
- Auth dirs are `0700` and files `0600`; `read_private_file` stays strict.
- Never log secrets: `OpenRouterToken` `Debug` is redacted, and errors exclude response bodies.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-auth.md) — read before changing a
subsystem.
