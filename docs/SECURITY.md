# Security Policy

We take the security of VT Code seriously. If you discover a security vulnerability, we appreciate your responsible
disclosure and will work to address it promptly.

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report security vulnerabilities via one of the following channels:

- Email: **<vinhnguyen2308@gmail.com>**
- [GitHub Private Vulnerability Reporting](https://github.com/vinhnx/vtcode/security/advisories/new) - This is the
  preferred method for reporting vulnerabilities, as it allows for secure, private communication.

### What to Include in Your Report

When reporting a security vulnerability, please provide us with the following information:

- A brief description of the vulnerability and its potential impact
- Steps to reproduce the issue (POC code is appreciated)
- Affected versions (if known)
- Any possible mitigations you've identified

### What to Expect

- **Acknowledgment**: We will acknowledge your report within 48 hours
- **Updates**: We will provide regular updates on the status of the vulnerability and fix progress
- **Resolution**: We will work to fix the vulnerability as quickly as possible and coordinate the release of the fix
  with you
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

| Version | Supported           |
| ------- | ------------------- |
| 0.133.x | Latest              |
| 0.132.x | Critical fixes only |
| < 0.132 | No longer supported |

## Security Features

VT Code includes several built-in security features:

- **Path Validation**: Prevents file system access outside the designated workspace
- **Tool Policies**: Configurable allow/deny/prompt policies for different operations
- **Shell approval hardening**: Learned read-only command families and command preflight reject dynamic shell syntax in
  `find` commands, so expansion cannot turn a reviewed shape into `-exec` or another destructive option
- **Workspace lifecycle hook approval**: Shell commands defined in workspace-controlled configuration (`vtcode.toml` /
  `.vtcode` in the repository) never run at session start or during lifecycle events until you explicitly approve the
  exact command set for that workspace; any change to those commands invalidates the approval and is skipped until
  reviewed
- **PII Protection**: Automatic tokenization of sensitive data in code execution
- **Token Management**: Secure handling of API keys and authentication tokens

## CI/CD Security Controls

VT Code enforces the following CI/CD security controls for GitHub Actions:

- **Forbidden trigger policy**: `pull_request_target` and `workflow_run` are blocked by policy checks.
- **Immutable action pinning**: every third-party and first-party action in workflows is pinned to a full 40-character
  commit SHA.
- **Workflow policy check in CI**: `scripts/check_workflow_security.sh` runs in CI and can be run locally via
  `./scripts/check.sh workflow-security`.
- **Least-privilege token permissions**: workflows default to `permissions: {}` and grant only job-level minimum
  permissions.
- **Checkout credential hardening**: all `actions/checkout` usage sets `persist-credentials: false` unless explicitly
  required.
- **Release credential isolation**: publishing runs in the dedicated `release` deployment environment (with reviewer
  gate), rather than broad repository-level credential exposure.
- **No release-path cache**: release workflows do not use dependency/build cache restore to reduce cache-poisoning
  exposure on publish paths.

## Security Architecture

For information about VT Code's security architecture, please see our documentation on:

- [Security Posture](README.md#overview)
- [Tool Permission Policies](./config/TOOLS_CONFIG.md)

## Additional Resources

- [Rust Security Advisories](https://github.com/RustSec/advisories)
- [OWASP Top 10](https://owasp.org/www-project-top-ten/)
- [Secure Coding Guidelines](https://github.com/OWASP/CheatSheetSeries)

## Version Updates

We regularly update dependencies and monitor for security vulnerabilities in our dependencies. CI runs the
[cargo-deny](https://embarkstudios.github.io/cargo-deny/) policy (`deny.toml`) on every pull request and weekly via
`.github/workflows/deny.yml`: RustSec advisories and yanked crates fail the build, licenses/duplicates/sources are
policy-checked. To run the same policy locally:

```bash
# Install cargo-deny if you haven't already
brew install cargo-deny   # or: cargo install cargo-deny --locked

# Audit dependencies against the RustSec Advisory Database
cargo deny --locked check

# House shortcuts
./scripts/check.sh deny        # same policy as CI
./scripts/check-dev.sh --deny  # opt-in from the fast gate
```

cargo-audit remains available as a secondary lockfile-wide cross-check:

```bash
cargo install cargo-audit
cargo audit
```

### Audit Provenance (cargo-vet)

Advisories catch _known_ vulnerabilities; [cargo-vet](https://mozilla.github.io/cargo-vet/) records _who audited what_.
CI enforces `cargo vet --locked` in `.github/workflows/deny.yml` using the store in `supply-chain/` (Mozilla and
Google audit imports, project exemptions ratcheted down over time). Any new or bumped dependency must be covered by a
trusted audit or a recorded exemption, or the build fails. To work with it locally:

```bash
# Install cargo-vet if you haven't already
cargo binstall cargo-vet   # or: cargo install cargo-vet --locked

cargo vet --locked   # same deterministic check as CI (reuses imports.lock)
cargo vet            # refresh upstream audits, then re-check (may update imports.lock)
cargo vet suggest    # low-hanging fruit to convert exemptions into real audits
cargo vet certify    # record an audit you performed yourself
```

House shortcuts (`./scripts/check.sh deny`, `./scripts/check-dev.sh --deny`) run deny and vet together. Never
hand-edit `supply-chain/` — use `cargo vet` subcommands so the files stay well-formed, and keep the exemptions list
shrinking: every entry there is unaudited code we ship.

First-party workspace crates are trusted (`audit-as-crates-io = false`): vetting our own code as third-party
forced exact-version self-exemptions that broke on every release, with no downstream signal, since exemptions
never propagate to consumers; only `cargo vet certify` audits do. If the project starts publishing real audits
of its own releases, re-enable the per-crate policies.

### Auditing Shipped Binaries

Release artifacts are built with [cargo-auditable](https://github.com/rust-secure-code/cargo-auditable): the exact
dependency list is embedded into each binary as compressed, inert metadata (`.dep-v0` section, no runtime overhead).
Anyone — including us — can scan a downloaded binary for known vulnerabilities without the source tree:

```bash
cargo install cargo-audit
cargo audit bin path/to/vtcode        # RustSec scan of the binary itself
```

trivy (v0.31+), grype (v0.83+), and osv-scanner (v2.0.1+) read the same section, so container/distro scanners pick it
up automatically. The GNU Linux, Windows, and macOS release builds embed the data; the `cross`-built musl/aarch64
artifacts do not (cargo runs inside the cross container, so a custom image would be required). Every release prints a
per-artifact coverage report during staging (`report_audit_data_coverage` in `scripts/release-assets.sh`), so a silent
regression in the auditable build step shows up in the release log instead of shipping unnoticed.

Embedded metadata records crate names and versions only (URLs/paths are redacted); it is an SBOM, not a
supply-chain defense.

### Dependency Publisher Review

For periodic human review of _who_ you are trusting (crates.io publishers with push rights to every dependency), use
[cargo supply-chain](https://github.com/rust-secure-code/cargo-supply-chain):

```bash
cargo install cargo-supply-chain
cargo supply-chain update        # fetch crates.io daily dump once (large download)
cargo supply-chain publishers    # list publishers across the dependency graph
```

This is an interactive audit tool, not a CI gate — run it when onboarding major new dependencies or during periodic
security reviews.

## Contact

For general security questions or concerns, please contact us via the channels mentioned above.

Thank you for helping keep VT Code and its users safe!
