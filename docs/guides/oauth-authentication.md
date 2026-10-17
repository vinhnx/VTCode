# OAuth Authentication Guide

VT Code supports secure OAuth 2.0 authentication for multiple AI providers, enabling seamless account-based access
without managing API keys directly.

## Overview

OAuth integration in VT Code provides:

- **PKCE-Secured Flows**: RFC 7636 Proof Key for Code Exchange for client-only applications
- **Secure Token Storage**: OS-native credential storage (Keychain, Credential Manager, Secret Service)
- **Automatic Token Refresh**: Seamless token renewal without user intervention
- **Multi-Provider Support**: GitHub Copilot, OpenAI ChatGPT, and OpenRouter
- **Managed Auth Delegation**: GitHub Copilot authentication delegated to official `copilot` CLI
- **Fallback Encryption**: AES-256-GCM encrypted file storage when keyring unavailable

API-key credentials remain independent from OAuth sessions. Secure API-key entries use the normalized
`(provider, environment-variable name)` identity, so an OAuth session or one API-key profile is never reused as another
profile. For example, MiMo pay-as-you-go and Token Plan keys can be stored separately:

```bash
vtcode secret add mimo --key-name MIMO_API_KEY
vtcode secret add mimo --key-name MIMO_TOKEN_PLAN_KEY
```

Environment variables and workspace `.env` values take precedence over secure storage. A legacy provider-only API-key
entry is migrated only when the default provider key is requested; non-default identities must be named explicitly.

## Supported Providers

### GitHub Copilot Managed Auth

VT Code uses the official `copilot` CLI for GitHub Copilot authentication. No separate OAuth flow is implemented —
instead, authentication is delegated to the official `copilot` binary.

#### Setup

```bash
# Install GitHub Copilot CLI
# See: https://docs.github.com/en/copilot/how-tos/copilot-cli/install-copilot-cli

# Authenticate via the official copilot CLI
copilot login

# Or use VT Code's TUI
vtcode
# Then use: /login copilot
```

**How it Works**:

- VT Code shells out to `copilot` binary for device-flow authentication
- `copilot login` launches browser-based device flow (GitHub handles auth)
- Credentials stored securely by the `copilot` CLI (platform-native keyring)
- VT Code probes auth status and launches ACP session without managing tokens

#### Requirements

- **Required**: `copilot` CLI on `PATH` (install via
  [GitHub's official guide](https://docs.github.com/en/copilot/how-tos/copilot-cli/install-copilot-cli))
- **Optional**: `gh` (GitHub CLI) — only used as fallback if already authenticated
- Active GitHub Copilot subscription

#### Configuration

In `vtcode.toml`:

```toml
[auth.copilot]
# Optional: Point to custom copilot binary if not on PATH
command = "/path/to/copilot"
```

Or via environment variable:

```bash
export VTCODE_COPILOT_COMMAND="/path/to/copilot"
```

#### Device Flow Authentication

When you run `/login copilot`:

1. VT Code invokes `copilot login`
2. `copilot` outputs a user code (e.g., `ABCD-EFGH`)
3. Browser opens to `github.com/login/device`
4. You enter the code and authorize
5. `copilot` stores credentials in OS keyring
6. VT Code detects auth status and activates Copilot provider

#### Token Management

Tokens are managed entirely by the `copilot` CLI:

```bash
# Check authentication status
vtcode auth status copilot

# Logout
vtcode logout copilot
# Or: /logout copilot
```

#### Troubleshooting

**`copilot` command not found**:

```bash
# Install copilot CLI
# macOS:
brew install gh-copilot

# Or download from GitHub:
# https://github.com/github/copilot-cli/releases
```

**VT Code doesn't detect auth**:

```bash
# Manually authenticate via copilot CLI first
copilot login

# Then check VT Code's detection
vtcode auth status copilot
```

**`gh` not found (optional)**:

- This is only a fallback; it's not required
- `copilot` works independently for auth
- VT Code only uses `gh` to detect existing auth sessions

### OpenAI ChatGPT OAuth

Authenticate with your OpenAI account to use ChatGPT models. VT Code implements an OAuth 2.0 PKCE authorization-code
flow for ChatGPT subscription auth — **the Codex CLI executable is NOT required** at runtime.

#### OAuth Client Identity (Unofficial Compatibility Flow)

By default, VT Code reuses the **Codex CLI's public-client identifier** (client ID `app_EMoamEEZ73f0CkXaXp7hrann`,
originator `codex_cli_rs`). This is a public OAuth client with no client secret — the ID is not a secret by OAuth 2.1
design. This is an **unofficial compatibility mechanism**: OpenAI has not documented or guaranteed that third-party
tools may reuse this client identifier, and it may stop working if OpenAI's client policy changes. Users should verify
applicable OpenAI terms of service.

If your organization has its own OpenAI-issued OAuth client, override both values:

```bash
export VTCODE_OPENAI_OAUTH_CLIENT_ID="your-client-id"
export VTCODE_OPENAI_OAUTH_ORIGINATOR="your-originator"
```

> **Note**: The custom client must accept the hard-coded localhost redirect URI, scopes, and Codex-specific
> authorization parameters VT Code sends. These variables do not make the flow a generic OAuth client implementation.
> The override must remain configured for the session lifetime — VT Code rereads the environment at refresh time.

#### Setup

```bash
# Option 1: In-process PKCE OAuth (full auto-refresh, no Codex CLI required)
vtcode
# Then use /login openai in the TUI, or:
vtcode login openai

# Option 2: Validate/reuse Codex CLI auth.json fallback (if you already ran `codex login`)
vtcode login openai --from-codex
```

**Authentication Methods**:

- In-process PKCE OAuth: VT Code's browser-based login with full auto-refresh (no Codex CLI required)
- Codex auth.json fallback: Automatically detected at runtime if `~/.codex/auth.json` exists
- API Key: Direct API key entry via `vtcode secret add openai`
- Manual Callback: Paste authorization code manually if browser auto-open fails

#### How ChatGPT Auth Works Without Codex CLI

VT Code implements an OAuth 2.0 PKCE authorization-code flow for OpenAI ChatGPT, using the Codex CLI's **public-client
identifier** by default. You do **not** need the Codex CLI or app installed. When you run `/login openai` or
`vtcode login openai`, VT Code:

1. Generates a PKCE challenge
2. Opens your browser to OpenAI's authorization page
3. Runs a local callback server to receive the authorization code
4. Exchanges the code for tokens
5. Stores them securely (keyring or encrypted file)
6. Refreshes them automatically when they expire

> **⚠️ Unofficial compatibility mechanism:** By default, VT Code reuses the Codex CLI's public PKCE client ID
> (`app_EMoamEEZ73f0CkXaXp7hrann`) and originator (`codex_cli_rs`). The client ID is **not a secret** (PKCE public
> clients have no client secret by OAuth 2.1 design), but reusing it is an **unofficial, unguaranteed** compatibility
> approach — it is not a VTCode-owned OAuth registration. OpenAI has not documented or guaranteed third-party reuse of
> this client identity, and a public identifier is **not authorization** to reuse another tool's OAuth registration.
> OpenAI may change or revoke it at any time. Organizations with their own OpenAI-issued client pair should set
> `VTCODE_OPENAI_OAUTH_CLIENT_ID` and `VTCODE_OPENAI_OAUTH_ORIGINATOR` (both must be set together; one-sided overrides
> are rejected as a configuration error).

If you happen to have the Codex CLI installed and authenticated (`codex login`), VT Code automatically detects
`~/.codex/auth.json` and uses it as a fallback when no VT Code-managed session is stored. This means:

- **Without Codex CLI**: VT Code's in-process PKCE flow handles everything — no dependency on the Codex executable.
- **With Codex CLI**: VT Code additionally reads Codex's auth.json as a fallback, so you don't need to authenticate
  twice. VT Code does **not** rotate Codex-owned refresh tokens; it rereads Codex's auth.json file to stay in sync with
  Codex's own refresh cycle. Run `codex logout` to clear the fallback.

#### Configuration

In `vtcode.toml`:

```toml
[auth.openai]
# Control where tokens are stored
credentials_store_mode = "keyring"  # "keyring" or "file"
# Preferred auth method: "chatgpt" (OAuth), "api_key", or "auto"
preferred_method = "auto"
# Auto-refresh tokens when they expire (default: true)
auto_refresh = true
```

#### Token Storage

**Default storage mode**:

- **macOS**: Encrypted file (avoids repeated Keychain authorization prompts)
- **Linux / Windows / other supported platforms**: Auto (keyring when functional, otherwise encrypted file)

When keyring is selected:

- **macOS**: Keychain
- **Windows**: Credential Manager
- **Linux**: Secret Service API / libsecret

**Encrypted file storage**:

- Location: the canonical user config directory's `auth/credential_<derived-name>.json` (see the
  [user data directories guide](user-data-directories.md) for platform resolution and overrides)
- Encryption: AES-256-GCM with machine-derived key and per-file salt
- Existing `openai_chatgpt.json` files are migrated automatically when loaded

Set `VTCODE_CONFIG` to select an absolute canonical config root for a managed deployment. New credentials are written
there; legacy credential locations are still read for compatibility when startup migration is bypassed. Do not use
`VTCODE_HOME` as a new credential destination: it identifies the preserved legacy source tree.

#### Codex auth.json Fallback

When VT Code has no stored ChatGPT session of its own, it automatically checks for Codex's `~/.codex/auth.json` (or
`$CODEX_HOME/auth.json`). If valid tokens are found, VT Code uses them at runtime with a special
`CodexAuthJsonRefresher` that re-reads the file when tokens need refreshing. VT Code does not rotate Codex-owned refresh
tokens itself — this avoids racing Codex's own refresh cycle or invalidating its stored credentials. Instead, it rereads
`auth.json` to pick up tokens Codex has refreshed.

```bash
# Validate Codex credentials are available (does not persist a VT Code session)
vtcode login openai --from-codex

# Check auth status (shows Codex fallback if active)
vtcode auth status openai

# Logout — clears VT Code session; Codex fallback remains until `codex logout`
vtcode logout openai
```

**Key difference from VT Code-managed OAuth**: Codex-sourced tokens are managed by the Codex CLI. When they expire, run
`codex login` to refresh them. A VT Code-managed session (`vtcode login openai`) refreshes tokens in-process without
requiring the Codex executable at runtime.

#### Troubleshooting

**Keyring unavailable on Linux**:

```bash
# Install a keyring daemon (e.g., gnome-keyring)
sudo apt-get install gnome-keyring

# Or use file-based storage
[auth.openai]
credentials_store_mode = "file"
```

**Clear OAuth Session**:

```bash
# Remove stored VT Code tokens
vtcode logout openai

# Also remove Codex fallback if present
codex logout
```

### OpenRouter OAuth

Authenticate with OpenRouter for access to multiple model providers.

#### Setup

```bash
# Launch VT Code with OpenRouter OAuth
vtcode

# Enable OAuth in the provider selection flow
```

**PKCE Flow**:

- Secure authorization without client secrets
- Callback server runs on `localhost:8484` (configurable)
- Browser-based authentication

#### Configuration

In `vtcode.toml`:

```toml
[llm.openrouter]
use_oauth = true               # Enable OAuth flow
auto_refresh = true            # Automatically refresh tokens
flow_timeout_secs = 300        # Browser flow timeout

[auth.openrouter]
callback_port = 8484           # Local OAuth callback server port
credentials_store_mode = "keyring"
```

#### Token Storage

Same shared storage as OpenAI:

- **Keyring**: Platform-native credential store when selected
- **Encrypted file**: the canonical user config directory's `auth/credential_<derived-name>.json`
- Existing `openrouter.json` files are migrated automatically when loaded

#### Refresh Tokens

OpenRouter tokens are automatically refreshed based on expiration:

```rust
// Automatic in production; refresh happens transparently
refresh_token_if_needed(&mut token_storage)?;
```

## Security Model

### Authentication Architecture

```text
User Request
    ↓
[Check Stored Token] ← Keyring (primary)
    ↓                   ← Encrypted File (fallback)
[Token Valid?]
    ├─ Yes → Use Token
    └─ No  → [PKCE OAuth Flow]
               ↓
            [Browser Auth]
               ↓
            [Callback Server]
               ↓
            [Token Exchange]
               ↓
            [Secure Storage]
```

### Key Management

**Machine-Derived Encryption Key** (file storage fallback):

- Based on: hostname + user ID + static salt
- Algorithm: SHA-256 (unkeyed hash of machine identity material)
- Cipher: AES-256-GCM (AEAD)

**No Plain Text**:

- Tokens never stored unencrypted
- Keyring data encrypted at OS level
- Encrypted files use authenticated encryption

### PKCE Security

Implements [RFC 7636](https://tools.ietf.org/html/rfc7636) requirements:

- **Code Challenge**: SHA-256 hash of 64-character random verifier
- **No Client Secret**: Suitable for public/native clients
- **Protected from CSRF**: State parameter included in flow

## CLI Usage

### Interactive Mode

```bash
vtcode
```

Follow the provider selection flow; OAuth authentication triggers automatically when enabled.

### Token Management

```bash
# View current auth status
vtcode auth status <provider>

# Clear authentication
vtcode auth clear <provider>

# Re-authenticate
vtcode auth refresh <provider>
```

**Supported providers**: `copilot`, `openai`, `openrouter`

**GitHub Copilot note**: Tokens are managed by the `copilot` CLI and stored in the OS keyring. VT Code probes status but
does not store credentials directly.

## Token Lifecycle

### Acquisition

1. User selects OAuth provider
2. PKCE challenge generated (64-character random verifier)
3. Browser opens to provider authorization page
4. User grants permission
5. Code exchanged for token
6. Token stored securely

### Refresh

1. Token checked before use
2. If expired, automatic refresh attempted
3. New token stored, old token discarded
4. If refresh fails, user prompted for re-authentication

### Expiration

- OpenAI: Token expiry follows the `expires_in` value from the token endpoint or the `exp` claim in the JWT. VT Code
  refreshes proactively every 8 minutes and treats tokens as expired 60 seconds before their actual expiry.
- OpenRouter: Provider-dependent

## Troubleshooting

### "Keyring not available"

**Linux**: Install and start a keyring daemon:

```bash
sudo apt-get install gnome-keyring
# Or use KDE Wallet, pass, etc.
```

**All Platforms**: Use file storage:

```toml
[auth.openai]
credentials_store_mode = "file"
```

### "Token exchange failed"

1. Check internet connection
2. Verify provider's OAuth service is operational
3. Ensure callback port (8080/8484) is not blocked by firewall
4. Try clearing session and re-authenticating:

   ```bash
   vtcode auth clear openai
   vtcode
   ```

### "Browser didn't open"

**Manual callback flow**:

1. Copy the authorization URL
2. Open manually in browser
3. Paste the authorization code back into VT Code

## Environment Variables

Control OAuth behavior via env vars:

```bash
# OpenAI OAuth client identity (override the default Codex CLI public client)
export VTCODE_OPENAI_OAUTH_CLIENT_ID="your-client-id"
export VTCODE_OPENAI_OAUTH_ORIGINATOR="your-originator"

# OpenAI auth preference
export OPENAI_PREFERRED_AUTH_METHOD="oauth"

# OpenRouter OAuth
export OPENROUTER_USE_OAUTH="true"
export OPENROUTER_CALLBACK_PORT="8484"

# Token storage
export VTCODE_AUTH_STORE_MODE="keyring"  # or "file"
```

## Development

### Testing OAuth Flows

```rust
// Example: Testing OpenRouter OAuth
use vtcode_auth::{
    get_auth_url,
    exchange_code_for_token,
    AuthCredentialsStoreMode,
};

// Get authorization URL
let (auth_url, verifier) = get_auth_url()?;
println!("Visit: {}", auth_url);

// Exchange authorization code for token
let token = exchange_code_for_token(
    code,
    &verifier,
    AuthCredentialsStoreMode::Keyring
)?;
```

### Adding a New OAuth Provider

1. **Create provider module**: `src/oauth_<provider>.rs`
2. **Implement PKCE flow**: Use `generate_pkce_challenge()`
3. **Token exchange**: Implement code ↔ token exchange
4. **Storage**: Use `CredentialStorage` for secure storage
5. **Configuration**: Add provider config to `AuthConfig`

See `crates/codegen/vtcode-auth/src/openrouter_oauth.rs` for a reference implementation.

## See Also

- [Authentication Overview](../security/SECURITY_MODEL.md#authentication)
- [Configuration Guide](../config/CONFIGURATION_PRECEDENCE.md)
- [Provider Setup](../providers/PROVIDER_GUIDES.md)
- [PKCE RFC 7636](https://tools.ietf.org/html/rfc7636)
