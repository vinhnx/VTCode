# Web Fetch Tool Security Configuration

The `web_fetch` tool provides flexible security modes for controlling which URLs can be accessed by VT Code agents.

## Security Modes

### Restricted Mode (Default)

```toml
[tools.web_fetch]
mode = "restricted"
```

In **restricted mode**, a blocklist is used to prevent access to sensitive domains while allowing most other sites. This is the default mode and suitable for general use.

Note: WebFetch will prefer Markdown content from documentation sites by sending an `Accept` header with `text/markdown` to encourage token-efficient responses from servers that serve docs in multiple formats.

**Built-in blocked domains include:**

-   Banking/Financial: PayPal, Stripe, Square, etc.
-   Authentication: Google Accounts, Microsoft Login, Okta, etc.
-   Email providers: Gmail, Outlook, iCloud Mail
-   Medical records: HealthVault, Epic, Cerner
-   Legal: DocuSign, Adobe Sign
-   VPN/Proxy services

**Built-in blocked patterns include:**

-   Credentials in URLs: `password=`, `token=`, `api_key=`, `secret=`
-   Auth headers: `oauth`, `bearer`, `x-auth`, `authorization:`
-   Sensitive paths: `/admin`, `/private`, `/internal`, `/secret`

**Exemptions:** Use `allowed_domains` to exempt specific domains from the blocklist.

### Whitelist Mode (Strict)

```toml
[tools.web_fetch]
mode = "whitelist"
```

In **whitelist mode**, only explicitly allowed domains can be accessed. This is the strictest mode and recommended for highly sensitive environments.

When using whitelist mode, you **must** configure `allowed_domains`:

```toml
[tools.web_fetch]
mode = "whitelist"
allowed_domains = [
  "github.com",
  "docs.rs",
  "python.org",
  "npmjs.com"
]
```

## Dynamic Configuration Files

### Enable Dynamic Blocklist (Restricted Mode)

```toml
[tools.web_fetch]
dynamic_blocklist_enabled = true
dynamic_blocklist_path = "~/.vtcode/web_fetch_blocklist.json"
```

Create `~/.vtcode/web_fetch_blocklist.json`:

```json
{
    "blocked_domains": ["internal-api.company.com", "legacy-system.local"],
    "blocked_patterns": ["internal_token=", "company_secret="]
}
```

**Benefits:**

-   Reload configuration without restarting VTCode
-   Maintain organization-specific blocklists
-   Separate from main configuration file

### Enable Dynamic Whitelist (Whitelist Mode)

```toml
[tools.web_fetch]
mode = "whitelist"
dynamic_whitelist_enabled = true
dynamic_whitelist_path = "~/.vtcode/web_fetch_whitelist.json"
```

Create `~/.vtcode/web_fetch_whitelist.json`:

```json
{
    "allowed_domains": [
        "github.com",
        "github.io",
        "docs.rs",
        "rust-lang.org",
        "crates.io",
        "npmjs.com",
        "pypi.org",
        "wikipedia.org",
        "stackoverflow.com"
    ]
}
```

## Inline Configuration

### Restricted Mode with Exemptions

Allow specific domains while using blocklist:

```toml
[tools.web_fetch]
mode = "restricted"
# These domains are exempt from blocklist
allowed_domains = [
  "paypal.com/status",  # Status page is safe to check
  "company-docs.com"    # Internal documentation
]
```

### Additional Blocked Domains/Patterns

Add custom domains/patterns to the blocklist:

```toml
[tools.web_fetch]
mode = "restricted"
blocked_domains = [
  "legacy-app.company.com",
  "deprecated-api.local"
]
blocked_patterns = [
  "internal_api_key=",
  "/admin",
  "/super_admin"
]
```

## HTTPS Enforcement

By default, only HTTPS URLs are allowed for security:

```toml
[tools.web_fetch]
strict_https_only = true  # Default
```

Disable only for development/testing:

```toml
[tools.web_fetch]
strict_https_only = false  # Use with caution!
```

## Security Best Practices

1. **Use restricted mode by default** - It blocks known sensitive domains while allowing general web access
2. **Use whitelist mode for sensitive operations** - Only allow domains you explicitly trust
3. **Review built-in blocklists** - Understand what's protected by default
4. **Keep dynamic files updated** - Regularly review and update JSON configuration files
5. **Use HTTPS always** - Keep `strict_https_only = true` in production
6. **Monitor URL access** - Enable audit logging to track what URLs are accessed:

```toml
[tools.web_fetch]
enable_audit_logging = true
audit_log_path = "~/.vtcode/web_fetch_audit.log"
```

## Configuration Examples

### Development Environment (Permissive)

```toml
[tools.web_fetch]
mode = "restricted"
strict_https_only = false  # Allow HTTP for local testing
blocked_domains = []       # Minimal restrictions
```

### Production (Conservative)

```toml
[tools.web_fetch]
mode = "restricted"
strict_https_only = true
dynamic_blocklist_enabled = true
dynamic_blocklist_path = "~/.vtcode/web_fetch_blocklist.json"
enable_audit_logging = true
audit_log_path = "~/.vtcode/web_fetch_audit.log"
```

### Enterprise (Whitelist Only)

```toml
[tools.web_fetch]
mode = "whitelist"
dynamic_whitelist_enabled = true
dynamic_whitelist_path = "~/.vtcode/web_fetch_whitelist.json"
enable_audit_logging = true
audit_log_path = "~/.vtcode/web_fetch_audit.log"
```

## Troubleshooting

### URL blocked unexpectedly

1. Check if domain is in built-in blocklist
2. Check if URL contains blocked pattern (credentials, /admin, etc.)
3. Check dynamic blocklist file if enabled
4. In whitelist mode, verify domain is in whitelist

### "Whitelist mode enabled but no domains whitelisted"

Configure `allowed_domains` or load from dynamic file:

```toml
[tools.web_fetch]
mode = "whitelist"
allowed_domains = ["github.com", "docs.rs"]
```

### Dynamic file not loading

1. Verify file path is correct (use `~/` for home directory)
2. Ensure file is valid JSON
3. Check file permissions (must be readable)
4. Look for errors in debug logs if enabled

## Related Configuration

-   **Tool policies**: Control when web_fetch requires approval

    ```toml
    [tools.policies]
    web_fetch = "prompt"  # Ask before fetching
    ```

-   **Content types**: Only text-based content is supported

    -   `text/html`, `text/plain`, `text/markdown`
    -   `application/json`, `application/xml`
    -   Binaries (executables, archives, etc.) are rejected

-   **Size limits**: Maximum 500KB content per fetch
    -   Override with `max_bytes` parameter per request
    -   Prevents downloading large files
