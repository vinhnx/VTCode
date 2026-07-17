//! Security domain constants for web_fetch tool
//!
//! Contains built-in blocklists for sensitive domains and patterns,
//! plus malicious indicators to prevent access to dangerous URLs.

/// Built-in blocklist domains (used in restricted mode)
pub const BUILTIN_BLOCKED_DOMAINS: &[&str] = &[
    // Banking & Financial
    "paypal.com",
    "stripe.com",
    "square.com",
    "interac.ca",
    "wire.com",
    // Authentication & Identity
    "github.com/login",
    "gitlab.com/users/login",
    "okta.com",
    "auth0.com",
    "accounts.google.com",
    "login.microsoftonline.com",
    "login.apple.com",
    // Email providers
    "mail.google.com",
    "outlook.live.com",
    "icloud.com/mail",
    // Personal/Private services
    "myfitnesspal.com",
    "health.apple.com",
    "health.google.com",
    // VPN & Proxy services
    "expressvpn.com",
    "nordvpn.com",
    // Medical & Health records
    "healthvault.com",
    "epic.com",
    "cerner.com",
    // Legal documents
    "docusign.com",
    "adobe.com/sign",
];

/// Built-in blocked patterns (used in restricted mode)
pub const BUILTIN_BLOCKED_PATTERNS: &[&str] = &[
    "password=",
    "token=",
    "api_key=",
    "secret=",
    "auth=",
    "session=",
    "cookie=",
    "oauth",
    "bearer%20",
    "x-auth",
    "authorization:",
    "/admin",
    "/private",
    "/internal",
    "/secret",
];

/// Common malware delivery and phishing patterns
pub const MALICIOUS_PATTERNS: &[&str] = &[
    // Obfuscation and evasion
    ".zip\"",
    ".exe\"",
    ".scr\"",
    ".bat\"",
    ".cmd\"",
    ".vbs\"",
    ".ps1\"",
    // Domain confusion (typosquatting indicators)
    "g00gle",
    "g0ogle",
    "gooogle",
    "micr0soft",
    "micro$oft",
    "amaz0n",
    "facebk",
    "faceb00k",
    // Suspicious subdomains
    "admin.",
    "backup.",
    "dev.",
    "test.",
    "temp.",
    "tmp.",
    // Known malware hosting patterns
    "bit.ly/",
    "short.link/",
    "tinyurl.com/",
    "goo.gl/",
];
