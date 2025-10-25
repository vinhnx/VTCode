# Security Policy

We take the security of VT Code seriously. If you discover a security vulnerability, we appreciate your responsible disclosure and will work to address it promptly.

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report security vulnerabilities via one of the following channels:

- Email: **security@vtcode.org** (replace with actual email if available)
- [GitHub Private Vulnerability Reporting](https://github.com/vinhnx/vtcode/security/advisories/new) - This is the preferred method for reporting vulnerabilities, as it allows for secure, private communication.

### What to Include in Your Report

When reporting a security vulnerability, please provide us with the following information:

- A brief description of the vulnerability and its potential impact
- Steps to reproduce the issue (POC code is appreciated)
- Affected versions (if known)
- Any possible mitigations you've identified

### What to Expect

- **Acknowledgment**: We will acknowledge your report within 48 hours
- **Updates**: We will provide regular updates on the status of the vulnerability and fix progress
- **Resolution**: We will work to fix the vulnerability as quickly as possible and coordinate the release of the fix with you
- **Credit**: We will publicly acknowledge your responsible disclosure (unless you prefer to remain anonymous)

## Security Best Practices for Users

### API Keys and Credentials
- Never commit API keys, tokens, or other sensitive credentials to version control
- Use environment variables for storing API keys instead of hardcoding them
- Consider using `.env` files with proper gitignore configuration
- Rotate your API keys regularly

### Configuration Security
- Keep your `vtcode.toml` configuration file secure and avoid sharing sensitive values
- Regularly review your tool policies to ensure only necessary operations are allowed
- Use secure connections when integrating with external services

### System Security
- Only run VT Code in trusted environments
- Be cautious when executing code or commands suggested by the AI agent
- Regularly update VT Code to the latest version to ensure you have the latest security patches

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.31.x  | Latest          |
| 0.30.x  | Critical fixes only |
| < 0.30  | No longer supported |

## Security Features

VT Code includes several built-in security features:

- **Sandboxed Execution**: Integration with Anthropic's sandbox runtime for secure command execution
- **Path Validation**: Prevents file system access outside the designated workspace
- **Tool Policies**: Configurable allow/deny/prompt policies for different operations
- **Network Restrictions**: Configurable network access controls
- **Token Management**: Secure handling of API keys and authentication tokens

## Security Architecture

For information about VT Code's security architecture, please see our documentation on:

- [Security Posture](README.md#security-posture)
- [Sandbox Runtime Integration](README.md#anthropic-sandbox-runtime-integration)
- [Tool Permission Policies](docs/config/TOOLS_CONFIG.md)

## Additional Resources

- [Rust Security Advisories](https://github.com/RustSec/advisories)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Secure Coding Guidelines](https://github.com/OWASP/CheatSheetSeries)

## Version Updates

We regularly update dependencies and monitor for security vulnerabilities in our dependencies. To check for known vulnerabilities in Rust dependencies, you can run:

```bash
# Install cargo-audit if you haven't already
cargo install cargo-audit

# Audit dependencies for known vulnerabilities
cargo audit
```

## Contact

For general security questions or concerns, please contact us via the channels mentioned above.

Thank you for helping keep VT Code and its users safe!