<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-auth

[Root AGENTS.md](../../../AGENTS.md) | OAuth PKCE flows and credential storage for LLM providers.

## Modules

`openai_chatgpt_oauth/` OpenAI ChatGPT OAuth (`session.rs` model/handle/flow, `refresh.rs` token exchange + storage, `jwt.rs` claim parsing) | `openai_refresh_policy/` pure refresh classification | `openai_session_storage/` typed session persistence | `openrouter_oauth/` OpenRouter OAuth | `openrouter_token_storage/` typed token persistence | `mcp_oauth/` MCP server OAuth | `oauth_server/` local callback server | `pkce/` PKCE challenge generation | `credentials/` credential storage (keyring + file) | `auth_service/` OpenAIAccountAuthService | `config/` AuthConfig types | `storage_paths/` path resolution | `codex_auth_import.rs` Codex CLI auth.json reuse

## Rules

- All OAuth flows use PKCE — `generate_pkce_challenge()` is the entry point. `credentials::CredentialStorage` supports keyring and file-based backends. `oauth_server::run_auth_code_callback_server` starts a local HTTP server for OAuth callbacks.
- Re-exported from `vtcode-config::auth` for backward compat — canonical code is here.

## Gotchas

- `clear_openai_chatgpt_session_with_mode()` and `clear_oauth_token_with_mode()` accept storage mode — use the `_with_mode` variants for explicit control. `AuthCredentialsStoreMode::effective_mode()` returns `ResolvedStoreMode` (crate-private, no `Auto` variant) so backend matches are exhaustive without `unreachable!`; `_exact_with_mode` methods take the resolved mode.
- MCP OAuth is separate from provider OAuth — `mcp_oauth::McpOAuthService` handles it.
- `credentials::keyring_entry` short-circuits when `keyring_disabled()` is true (`cfg!(debug_assertions)`, `cfg!(test)`, `VTCODE_DISABLE_KEYRING`, or `CI`), so debug builds, tests, and CI fall back to file storage and never trigger macOS Keychain prompts. Debug-keyring can be re-enabled with `VTCODE_DISABLE_KEYRING=0`. `is_keyring_functional()` caches its result to avoid repeated Keychain round trips.
- API-key writes verify a secure-storage read-back; keep the configured `AuthCredentialsStoreMode` consistent between writes and runtime resolution. Encrypted auth directories are private (`0700`) and credential files are private (`0600`).
- API-key storage is scoped by normalized `(provider, key_name)`; `key_name` is normally the resolved environment variable. Provider-only entries are legacy fallbacks and may be migrated only for the provider default key, never for an ambiguous non-default profile.
- `read_private_file` is strict about regular files and Unix `0600`-compatible permissions; compatibility lookup uses `read_legacy_compatible_file` so canonical auth paths stay strict while true legacy rollback candidates remain readable until copied.
- `OpenRouterToken` has a redacted `Debug` implementation, and token-exchange errors must not include response bodies that could echo secrets.
- `openai_refresh_policy` returns a clear/preserve action only; storage deletion stays in the orchestration layer.
