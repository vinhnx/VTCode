# Web Fetch Security - Practical Examples

## Safe URLs (Will be Allowed)

### Documentation & Public Content
```
✅ https://github.com/vinhnx/vtcode
✅ https://docs.rust-lang.org/
✅ https://developer.mozilla.org/en-US/docs/
✅ https://wikipedia.org/wiki/Python
✅ https://news.ycombinator.com
```

### Public APIs
```
✅ https://api.github.com/repos/vinhnx/vtcode
✅ https://api.example.com/v1/public/data
✅ https://jsonplaceholder.typicode.com/posts
✅ https://openlibrary.org/api/
```

### Blog Posts & Articles
```
✅ https://medium.com/@author/article
✅ https://dev.to/author/article
✅ https://example.com/blog/2024/01/article
✅ https://example.org/resources/tutorial
```

### Public Services & Repositories
```
✅ https://crates.io/crates/tokio
✅ https://npmjs.com/package/express
✅ https://godoc.org/net/http
✅ https://pypi.org/project/requests
```

---

## Blocked URLs (Will be Rejected)

### 🚫 Banking & Financial Services

```
❌ https://paypal.com/login
   Reason: Access to sensitive domain 'paypal.com'

❌ https://stripe.com/account
   Reason: Access to sensitive domain 'stripe.com'

❌ https://square.com/dashboard
   Reason: Access to sensitive domain 'square.com'

❌ https://interac.ca/personal-banking
   Reason: Access to sensitive domain 'interac.ca'
```

### 🚫 Authentication & Identity Services

```
❌ https://accounts.google.com
   Reason: Access to sensitive domain 'accounts.google.com'

❌ https://github.com/login
   Reason: Access to sensitive domain 'github.com/login'

❌ https://login.microsoftonline.com
   Reason: Access to sensitive domain 'login.microsoftonline.com'

❌ https://okta.com/signin
   Reason: Access to sensitive domain 'okta.com'

❌ https://auth0.com/login
   Reason: Access to sensitive domain 'auth0.com'
```

### 🚫 Email Providers

```
❌ https://mail.google.com
   Reason: Access to sensitive domain 'mail.google.com'

❌ https://outlook.live.com
   Reason: Access to sensitive domain 'outlook.live.com'

❌ https://icloud.com/mail
   Reason: Access to sensitive domain 'icloud.com/mail'
```

### 🚫 Health & Medical Records

```
❌ https://health.apple.com
   Reason: Access to sensitive domain 'health.apple.com'

❌ https://health.google.com
   Reason: Access to sensitive domain 'health.google.com'

❌ https://healthvault.com/account
   Reason: Access to sensitive domain 'healthvault.com'

❌ https://myfitnesspal.com/login
   Reason: Access to sensitive domain 'myfitnesspal.com'
```

### 🚫 VPN & Privacy Services

```
❌ https://expressvpn.com/account
   Reason: Access to sensitive domain 'expressvpn.com'

❌ https://nordvpn.com/dashboard
   Reason: Access to sensitive domain 'nordvpn.com'
```

### 🚫 Legal Documents

```
❌ https://docusign.com/signin
   Reason: Access to sensitive domain 'docusign.com'

❌ https://adobe.com/sign/sso
   Reason: Access to sensitive domain 'adobe.com/sign'
```

### 🚫 URLs with Credentials in Query Parameters

```
❌ https://api.example.com?api_key=sk_live_1234567890abcdef
   Reason: URL contains sensitive pattern 'api_key='. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://example.com?password=MySecretPassword123
   Reason: URL contains sensitive pattern 'password='. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://example.com?token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
   Reason: URL contains sensitive pattern 'token='. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://database.example.com?session=abc123xyz
   Reason: URL contains sensitive pattern 'session='. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://api.example.com?oauth=...
   Reason: URL contains sensitive pattern 'oauth'. Fetching URLs with 
           credentials or sensitive data is blocked
```

### 🚫 URLs with Sensitive Paths

```
❌ https://example.com/admin/dashboard
   Reason: URL contains sensitive pattern '/admin'. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://example.com/private/files
   Reason: URL contains sensitive pattern '/private'. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://example.com/internal/api
   Reason: URL contains sensitive pattern '/internal'. Fetching URLs with 
           credentials or sensitive data is blocked

❌ https://example.com/secret/config
   Reason: URL contains sensitive pattern '/secret'. Fetching URLs with 
           credentials or sensitive data is blocked
```

### 🚫 Malware & Phishing Indicators

```
❌ https://suspicious-site.com/download/file.exe
   Reason: URL contains potentially malicious pattern. Access blocked for safety

❌ https://suspicious-site.com/malware.bat
   Reason: URL contains potentially malicious pattern. Access blocked for safety

❌ https://suspicious-site.com/script.ps1
   Reason: URL contains potentially malicious pattern. Access blocked for safety

❌ https://g00gle.com/search
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (This is a typosquatting domain mimicking Google)

❌ https://micr0soft.com/download
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (This is a typosquatting domain mimicking Microsoft)

❌ https://gooogle.com
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (This is a typosquatting domain mimicking Google)

❌ https://facebk.com
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (This is a typosquatting domain mimicking Facebook)

❌ https://amaz0n.com/products
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (This is a typosquatting domain mimicking Amazon)
```

### 🚫 URL Shorteners (Security Risk)

```
❌ https://bit.ly/abc123xyz
   Reason: URL contains potentially malicious pattern. Access blocked for safety
   (URL shorteners can hide the actual destination)

❌ https://short.link/post/12345
   Reason: URL contains potentially malicious pattern. Access blocked for safety

❌ https://tinyurl.com/mylink
   Reason: URL contains potentially malicious pattern. Access blocked for safety

❌ https://goo.gl/abc123
   Reason: URL contains potentially malicious pattern. Access blocked for safety
```

### 🚫 Non-HTTPS & Private Network Access

```
❌ http://example.com
   Reason: Only HTTPS URLs are allowed for security

❌ https://localhost:8080
   Reason: Access to local/private networks is blocked

❌ https://127.0.0.1:3000
   Reason: Access to local/private networks is blocked

❌ https://0.0.0.0
   Reason: Access to local/private networks is blocked

❌ https://[::1]:5000
   Reason: Access to local/private networks is blocked

❌ https://api.local
   Reason: Access to local/private networks is blocked

❌ https://internal.company.internal
   Reason: Access to local/private networks is blocked
```

---

## Security Best Practices

### ✅ DO:
- Use public documentation URLs
- Fetch from known, trusted public services
- Use URLs without any credentials
- Access public APIs with proper endpoint structure

### ❌ DON'T:
- Embed API keys in URLs (use request headers instead)
- Include passwords in URLs
- Use URL shorteners (always use full URLs)
- Access local/private services from the web_fetch tool
- Use HTTP (always use HTTPS)
- Access sensitive personal or financial services

---

## Error Response Format

When a URL is blocked, you'll receive a JSON response with this format:

```json
{
  "error": "web_fetch: failed to fetch URL 'https://example.com': [REASON]",
  "url": "https://example.com"
}
```

**Example responses:**

```json
{
  "error": "web_fetch: failed to fetch URL 'https://paypal.com/login': Access to sensitive domain 'paypal.com' is blocked for privacy and security reasons",
  "url": "https://paypal.com/login"
}
```

```json
{
  "error": "web_fetch: failed to fetch URL 'https://example.com?password=secret': URL contains sensitive pattern 'password='. Fetching URLs with credentials or sensitive data is blocked",
  "url": "https://example.com?password=secret"
}
```

---

## Testing the Security

You can test these security measures by attempting to fetch blocked URLs:

```rust
// Test 1: Banking domain should be rejected
web_fetch("https://paypal.com/login", "Extract login form")

// Test 2: Credentials in URL should be rejected
web_fetch("https://api.example.com?api_key=sk_123", "Get data")

// Test 3: Typosquatting should be rejected
web_fetch("https://g00gle.com", "Search for something")

// Test 4: Safe URLs should be allowed
web_fetch("https://github.com/vinhnx/vtcode", "List repositories")
```

---

## Support & Maintenance

For questions about blocked URLs or to suggest additions to the blocklist, refer to:
- Implementation: `vtcode-core/src/tools/web_fetch.rs`
- Full documentation: `docs/SECURITY_WEB_FETCH.md`
- This guide: `docs/WEB_FETCH_EXAMPLES.md`
