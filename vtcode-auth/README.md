# vtcode-auth

`vtcode-auth` provides authentication and OAuth flows shared across VT Code.

It contains:

- OAuth login flows for OpenAI ChatGPT, OpenRouter, and MCP servers
- A local callback server (`axum`-based) for receiving OAuth authorization codes
- PKCE challenge generation for secure OAuth exchanges
- Credential storage backed by the OS keyring and on-disk fallback

## Usage

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
vtcode-auth = { path = "../vtcode-auth" }
```

```rust
use vtcode_auth::{AuthConfig, CredentialStorage, PkceChallenge, OAuthProvider};

// Generate a PKCE challenge for an OAuth flow
let challenge = vtcode_auth::generate_pkce_challenge();

// Start the local callback server for a provider
let server = vtcode_auth::start_auth_code_callback_server(OAuthProvider::OpenRouter).await?;
```

## API Reference

| Type | Purpose |
|------|---------|
| `AuthConfig` | Top-level authentication configuration |
| `CopilotAuthConfig` | Copilot-specific auth settings |
| `OpenAIAuthConfig` | OpenAI-specific auth settings |
| `CredentialStorage` | Read/write credentials via OS keyring or file |
| `CustomApiKeyStorage` | Store and retrieve user-provided API keys |
| `McpOAuthService` | Drive the MCP OAuth login flow |
| `AuthCodeCallbackServer` | Local HTTP server that captures OAuth callbacks |
| `OAuthProvider` | Enum of supported OAuth providers |
| `PkceChallenge` | PKCE code-verifier / code-challenge pair |

## Public entrypoints

- `credentials` — credential storage, migration, and custom API-key helpers
- `mcp_oauth` — MCP-server OAuth login lifecycle
- `oauth_server` — local callback server and provider definitions
- `openai_chatgpt_oauth` — OpenAI ChatGPT OAuth session management
- `openrouter_oauth` — OpenRouter OAuth token management
- `pkce` — PKCE challenge generation

## Related docs

- [Architecture overview](../docs/ARCHITECTURE.md)
