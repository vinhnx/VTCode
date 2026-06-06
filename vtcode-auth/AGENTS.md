# vtcode-auth

[Root AGENTS.md](../AGENTS.md) | OAuth PKCE flows and credential storage for LLM providers.

## Modules

`openai_chatgpt_oauth/` OpenAI ChatGPT OAuth | `openrouter_oauth/` OpenRouter OAuth | `mcp_oauth/` MCP server OAuth | `oauth_server/` local callback server | `pkce/` PKCE challenge generation | `credentials/` credential storage (keyring + file) | `auth_service/` OpenAIAccountAuthService | `config/` AuthConfig types | `storage_paths/` path resolution

## Rules

- All OAuth flows use PKCE — `generate_pkce_challenge()` is the entry point.
- `credentials::CredentialStorage` supports keyring and file-based backends.
- `oauth_server::run_auth_code_callback_server` starts a local HTTP server for OAuth callbacks.
- Re-exported from `vtcode-config::auth` for backward compat — canonical code is here.

## Gotchas

- `clear_openai_chatgpt_session_with_mode()` and `clear_oauth_token_with_mode()` accept storage mode — use the `_with_mode` variants for explicit control.
- MCP OAuth is separate from provider OAuth — `mcp_oauth::McpOAuthService` handles it.
- `credentials::keyring_entry` short-circuits when `keyring_disabled()` is true (`cfg!(test)`, `VTCODE_DISABLE_KEYRING`, or `CI`), so tests/CI fall back to file storage and never trigger macOS Keychain prompts. Check scripts export `VTCODE_DISABLE_KEYRING=1` via `scripts/common.sh`.
