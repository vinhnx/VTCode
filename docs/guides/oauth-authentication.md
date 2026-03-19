# OAuth Authentication Guide

VT Code supports secure OAuth 2.0 authentication for multiple AI providers, enabling seamless account-based access without managing API keys directly.

## Overview

OAuth integration in VT Code provides:

- **PKCE-Secured Flows**: RFC 7636 Proof Key for Code Exchange for client-only applications
- **Secure Token Storage**: OS-native credential storage (Keychain, Credential Manager, Secret Service)
- **Automatic Token Refresh**: Seamless token renewal without user intervention
- **Multi-Provider Support**: OpenAI ChatGPT and OpenRouter
- **Fallback Encryption**: AES-256-GCM encrypted file storage when keyring unavailable

## Supported Providers

### OpenAI ChatGPT OAuth

Authenticate with your OpenAI account to use ChatGPT models.

#### Setup

```bash
# Launch VT Code
vtcode

# Use the OAuth flow within the TUI
# VT Code will open your browser for OpenAI authentication
```

**Authentication Methods**:
- OAuth (recommended): Automatic browser-based login
- API Key: Direct API key entry
- Manual Callback: Paste authorization code manually

#### Configuration

In `vtcode.toml`:

```toml
[llm.openai]
# OAuth settings
preferred_auth_method = "oauth"  # "oauth", "api_key", or "manual_callback"

# Optional: Specify callback port for manual OAuth flow
auth_callback_port = 8080

[auth.openai]
# Control where tokens are stored
credentials_store_mode = "keyring"  # "keyring" or "file"
```

#### Token Storage

**Default (Keyring)**:
- **macOS**: Keychain
- **Windows**: Credential Manager
- **Linux**: Secret Service API / libsecret

**Fallback (Encrypted Files)**:
- Location: `~/.vtcode/auth/openai_chatgpt.json`
- Encryption: AES-256-GCM with machine-derived key
- Automatic migration from file-based to keyring storage

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
# Remove stored tokens (interactive prompt will request fresh auth)
vtcode auth clear openai
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

Same as OpenAI:
- **Keyring**: Platform-native credential store (default)
- **Fallback**: `~/.vtcode/auth/openrouter.json` (AES-256-GCM encrypted)

#### Refresh Tokens

OpenRouter tokens are automatically refreshed based on expiration:

```rust
// Automatic in production; refresh happens transparently
refresh_token_if_needed(&mut token_storage)?;
```

## Security Model

### Authentication Architecture

```
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
- Algorithm: PBKDF2 (SHA-256)
- Cipher: AES-256-GCM (AEAD)

**No Plain Text**:
- Tokens never stored unencrypted
- Keyring data encrypted at OS level
- Encrypted files use authenticated encryption

### PKCE Security

Implements [RFC 7636](https://tools.ietf.org/html/rfc7636) requirements:

- **Code Challenge**: SHA-256 hash of 128-byte random verifier
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

**Supported providers**: `openai`, `openrouter`

## Token Lifecycle

### Acquisition

1. User selects OAuth provider
2. PKCE challenge generated (128-byte random verifier)
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

- OpenAI: 30-day expiration
- OpenRouter: Provider-dependent
- Grace period: 5 minutes (token considered expired 5 min before actual expiration)

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
# OpenAI OAuth
export OPENAI_OAUTH_CLIENT_ID="your-client-id"
export OPENAI_OAUTH_REDIRECT_URI="http://localhost:8080/auth/callback"
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

See `vtcode-auth/src/openrouter_oauth.rs` for a reference implementation.

## See Also

- [Authentication Overview](../security/SECURITY_MODEL.md#authentication)
- [Configuration Guide](../config/CONFIGURATION_PRECEDENCE.md)
- [Provider Setup](../providers/PROVIDER_GUIDES.md)
- [PKCE RFC 7636](https://tools.ietf.org/html/rfc7636)
