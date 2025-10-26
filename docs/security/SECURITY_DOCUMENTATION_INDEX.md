# Security Documentation Index

Complete guide to VT Code's security documentation.

## Quick Start

🚀 **New to VT Code Security?** Start here:
- [Security Quick Reference](./SECURITY_QUICK_REFERENCE.md) - Security at a glance

## Core Documentation

### For Users

**[Security Guide](./guides/security.md)**
- Security features overview
- Configuration instructions
- Best practices
- Incident response

⚡ **[Security Quick Reference](./SECURITY_QUICK_REFERENCE.md)**
- 5-layer security model summary
- Allowed/blocked commands
- Quick security checks
- Emergency response

### For Developers

**[Security Model](./SECURITY_MODEL.md)**
- Complete security architecture
- Defense-in-depth layers
- Threat model
- Adding new commands safely
- Security testing procedures

**[Security Audit](./SECURITY_AUDIT.md)**
- Current security posture
- Vulnerability analysis
- Testing checklist
- Recommendations

### For Organizations

**[Security Implementation](./SAFETY_IMPLEMENTATION.md)**
- Enterprise security controls
- Compliance considerations
- Deployment guidelines

📋 **[Tool Policies](./vtcode_tools_policy.md)**
- Command execution policies
- Approval workflows
- Policy configuration

## Recent Security Work

### October 25, 2025 Security Hardening

📝 **[Security Fix: Ripgrep Preprocessor](./SECURITY_FIX_2025-10-25.md)**
- Fixed HIGH severity vulnerability
- Ripgrep `--pre` flag argument injection
- Comprehensive test suite added

📊 **[Security Summary](./SECURITY_SUMMARY.md)**
- Executive summary of security work
- Testing results
- Impact assessment

## Security Features by Layer

### Layer 1: Command Allowlist
- **Location**: `vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 1](./SECURITY_MODEL.md#layer-1-command-allowlist)
- **Only 9 commands allowed**: ls, cat, cp, head, printenv, pwd, rg, sed, which

### Layer 2: Argument Validation
- **Location**: `vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 2](./SECURITY_MODEL.md#layer-2-per-command-argument-validation)
- **Per-command validators**: Explicit flag allowlists, execution flag blocking

### Layer 3: Workspace Isolation
- **Location**: `vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 3](./SECURITY_MODEL.md#layer-3-workspace-boundary-enforcement)
- **Path validation**: Normalization, symlink resolution, boundary checks

### Layer 4: Sandbox Integration
- **Location**: `vtcode-core/src/sandbox/`, `vtcode-core/src/tools/bash_tool.rs`
- **Documentation**: [Security Guide - Sandbox](./guides/security.md#sandbox-configuration)
- **Anthropic sandbox**: Filesystem isolation, network allowlist

### Layer 5: Human-in-the-Loop
- **Location**: `src/agent/runloop/unified/tool_routing.rs`
- **Documentation**: [Security Guide - Approval System](./guides/security.md#human-in-the-loop)
- **Three-tier approval**: Once, Session, Permanent

## Testing & Verification

### Security Test Suite
- **Location**: `vtcode-core/tests/execpolicy_security_tests.rs`
- **Tests**: 14 comprehensive security tests
- **Run**: `cargo test -p vtcode-core --test execpolicy_security_tests`

### Test Coverage
- Ripgrep preprocessor flags blocked
- Sed execution flags blocked
- Path traversal blocked
- Absolute paths outside workspace blocked
- Disallowed commands blocked
- Safe command usage allowed

## Configuration

### Tool Policy
- **File**: `~/.config/vtcode/tool_policy.toml`
- **Documentation**: [Tool Policies](./vtcode_tools_policy.md)

### Workspace Configuration
- **File**: `vtcode.toml`
- **Documentation**: [Configuration Guide](./CONFIGURATION.md)

### Sandbox Configuration
- **File**: `vtcode.toml` (sandbox section)
- **Documentation**: [Security Guide - Sandbox](./guides/security.md#sandbox-configuration)

## Integration Points

### Main README
- **File**: `../README.md`
- **Security Section**: Highlights 5-layer security model
- **Badge**: Security hardened badge

### Docs Hub
- **File**: `./README.md`
- **Security Section**: Comprehensive security features list
- **Links**: All security documentation

### Getting Started
- **File**: `./user-guide/getting-started.md`
- **Security Section**: Security best practices for new users

### Steering Rules
- **File**: `../.kiro/steering/vtcode.md`
- **Security Section**: Argument injection protection guidelines for agents

## External References

### Research & Standards
- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [GTFOBINS](https://gtfobins.github.io/)
- [LOLBINS Project](https://lolbas-project.github.io/)
- [OWASP Command Injection](https://owasp.org/www-community/attacks/Command_Injection)
- Trail of Bits: Argument Injection in AI Agents

### Anthropic Resources
- [Anthropic Safety Guidelines](https://www.anthropic.com/safety)
- [Anthropic Sandbox Runtime](https://docs.anthropic.com/en/docs/build-with-claude/tool-use#sandbox-runtime)

## Reporting Security Issues

### Responsible Disclosure
1. **Do Not Disclose Publicly** - Report privately first
2. **GitHub Security Advisory** - Use GitHub's security advisory feature
3. **Provide Details** - Include reproduction steps
4. **Coordinate Disclosure** - Allow time for fix

### Contact
- **GitHub**: [Security Advisories](https://github.com/vinhnx/vtcode/security/advisories)
- **Email**: See GitHub profile

## Changelog

### October 25, 2025
- Fixed ripgrep `--pre` flag vulnerability (HIGH)
- Added comprehensive security test suite (14 tests)
- Created security documentation suite
- Updated README and docs with security highlights
- Added security architecture diagram

### Future Work
- Bash tool allowlist argument validation
- Compiler flag validation
- Build tool argument validation
- Expand sandbox usage
- Add fuzzing for validators

See [Security Audit](./SECURITY_AUDIT.md) for detailed recommendations.

---

**Documentation Version**: 1.0  
**Last Updated**: October 25, 2025  
**Security Model Version**: 1.0
