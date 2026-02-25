# Web Fetch Security & Malicious URL Prevention

## Overview

The `web_fetch` tool now includes comprehensive security checks to prevent fetching from sensitive, malicious, or privacy-compromising URLs. These checks are applied at the validation stage before any network request is made.

## Security Layers

### 1. Protocol Validation
- **HTTPS only**: All URLs must use HTTPS protocol
- **Rejects**: HTTP, FTP, and other non-secure protocols

### 2. Network Isolation
- **Blocks local/private networks**: `localhost`, `127.0.0.1`, `0.0.0.0`, `::1`, `.local`, `.internal`
- **Prevents SSRF attacks**: Protects against Server-Side Request Forgery

### 3. Sensitive Domain Blocklist

Blocks access to sensitive/privacy-sensitive domains including:

#### Banking & Financial
- `paypal.com`
- `stripe.com`
- `square.com`
- `interac.ca`
- `wire.com`

#### Authentication & Identity
- `github.com/login`
- `gitlab.com/users/login`
- `okta.com`
- `auth0.com`
- `accounts.google.com`
- `login.microsoftonline.com`
- `login.apple.com`

#### Email Providers
- `mail.google.com`
- `outlook.live.com`
- `icloud.com/mail`

#### Personal/Private Services
- `myfitnesspal.com`
- `health.apple.com`
- `health.google.com`

#### VPN & Proxy Services
- `expressvpn.com`
- `nordvpn.com`

#### Medical & Health Records
- `healthvault.com`
- `epic.com`
- `cerner.com`

#### Legal Documents
- `docusign.com`
- `adobe.com/sign`

### 4. Sensitive URL Pattern Detection

Blocks URLs containing sensitive query parameters and paths:

- **Credentials**: `password=`, `token=`, `api_key=`, `secret=`, `session=`, `cookie=`
- **Authentication**: `auth=`, `oauth`, `bearer%20`, `x-auth`, `authorization:`
- **Private Paths**: `/admin`, `/private`, `/internal`, `/secret`

### 5. Malicious URL Indicators

Blocks URLs containing common malware and phishing indicators:

#### Obfuscation & Evasion
- Executable file patterns: `.zip"`, `.exe"`, `.scr"`, `.bat"`, `.cmd"`, `.vbs"`, `.ps1"`

#### Domain Confusion (Typosquatting)
- Homograph attacks: `g00gle`, `g0ogle`, `gooogle`, `micr0soft`, `micro$oft`, `amaz0n`, `facebk`, `faceb00k`

#### Suspicious Subdomains
- `admin.`, `backup.`, `dev.`, `test.`, `temp.`, `tmp.`

#### URL Shorteners
- `bit.ly/`, `short.link/`, `tinyurl.com/`, `goo.gl/`
- These can obscure the real destination and are commonly used in phishing campaigns

## Error Messages

When a URL is blocked, the tool returns a clear error message indicating the reason:

```json
{
  "error": "web_fetch: failed to fetch URL 'https://paypal.com/login': Access to sensitive domain 'paypal.com' is blocked for privacy and security reasons",
  "url": "https://paypal.com/login"
}
```

## Implementation Details

### Validation Flow

```
URL Input
  ↓
Protocol Check (HTTPS only)
  ↓
Network Isolation Check (no local/private)
  ↓
Safety Validation
  → Blocked Domains Check
  → Sensitive Pattern Check
  → Malicious Indicators Check
  ↓
Network Request (if all checks pass)
```

### Code Location

- **Main implementation**: `vtcode-core/src/tools/web_fetch.rs`
- **Key functions**:
  - `validate_url()` - Entry point for validation
  - `validate_url_safety()` - Sensitive domain and pattern checks
  - `check_malicious_indicators()` - Malware/phishing detection

## Testing

The implementation includes comprehensive test cases:

- `rejects_sensitive_banking_domains()` - Banking URL rejection
- `rejects_sensitive_auth_domains()` - Auth domain rejection
- `rejects_urls_with_credentials()` - Password parameter detection
- `rejects_urls_with_api_keys()` - API key parameter detection
- `rejects_urls_with_tokens()` - Token parameter detection
- `rejects_malicious_url_patterns()` - Executable file detection
- `rejects_typosquatting_domains()` - Domain confusion detection
- `rejects_url_shorteners()` - URL shortener blocking

Run tests with:
```bash
cargo test --lib web_fetch
```

## Future Enhancements

Potential improvements:

1. **Dynamic blocklist**: Load sensitive domains from external configuration
2. **Machine learning**: Add ML-based phishing detection
3. **Content scanning**: Scan fetched content for malware signatures
4. **DNS validation**: Check against public DNS blocklists (Google Safe Browsing, etc.)
5. **Certificate validation**: Enhanced SSL/TLS certificate validation
6. **Whitelist mode**: Optional mode to only allow whitelisted domains

## Security Considerations

### What This Protects Against

- Accidental credential leakage through URL parameters
- SSRF attacks
- Phishing attempts through typosquatting domains
- Malware delivery via executable downloads
- Privacy breaches through sensitive service access
- Token/API key exposure

### What This Does NOT Protect Against

- Malicious content served from legitimate domains
- Zero-day exploits in the HTTP client library
- Social engineering to craft URLs manually
- Attacks targeting the AI model's behavior

## Maintenance

### Adding New Blocked Domains

Edit `vtcode-core/src/tools/web_fetch.rs` in the `validate_url_safety()` function's `blocked_domains` array.

### Updating Malicious Patterns

Modify the `malicious_patterns` array in the `check_malicious_indicators()` function.

## References

- OWASP Top 10 - A10:2021 – Server-Side Request Forgery (SSRF)
- OWASP - URL Validation Best Practices
- NIST - Secure Software Development Framework (SSDF)
