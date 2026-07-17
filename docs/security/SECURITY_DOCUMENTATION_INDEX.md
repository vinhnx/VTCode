# Security Documentation Index

Complete guide to VT Code's security documentation.

## Quick Start

**New to VT Code Security?** Start here:

- [Security Model](./SECURITY_MODEL.md) - Complete security architecture

## Core Documentation

### For Users

**[Security Guide](../guides/security.md)**
- Security features overview
- Configuration instructions
- Best practices
- Incident response

### For Developers

**[Security Model](./SECURITY_MODEL.md)**
- Complete security architecture
- Defense-in-depth layers
- Threat model
- Adding new commands safely
- Security testing procedures

**[Web Fetch Security](./SECURITY_WEB_FETCH.md)**
- Web fetch security policies

### For Organizations

**[Tool Policies](../modules/vtcode_tools_policy.md)**
- Command execution policies
- Approval workflows
- Policy configuration

## Security Features by Layer

### Layer 1: Command Allowlist
- **Location**: `crates/codegen/vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 1](./SECURITY_MODEL.md#layer-1-command-allowlist)
- **Only 9 commands allowed**: ls, cat, cp, head, printenv, pwd, rg, sed, which

### Layer 2: Argument Validation
- **Location**: `crates/codegen/vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 2](./SECURITY_MODEL.md#layer-2-per-command-argument-validation)
- **Per-command validators**: Explicit flag allowlists, execution flag blocking

### Layer 3: Workspace Isolation
- **Location**: `crates/codegen/vtcode-core/src/execpolicy/mod.rs`
- **Documentation**: [Security Model - Layer 3](./SECURITY_MODEL.md#layer-3-workspace-boundary-enforcement)
- **Path validation**: Normalization, symlink resolution, boundary checks

### Layer 4: Sandbox Integration
- **Location**: `crates/codegen/vtcode-core/src/sandbox/`, `crates/codegen/vtcode-core/src/tools/bash_tool.rs`
- **Documentation**: [Security Guide - Sandbox](../guides/security.md#sandbox-configuration)
- **Anthropic sandbox**: Filesystem isolation, network allowlist

### Layer 5: Human-in-the-Loop
- **Location**: `src/agent/runloop/unified/tool_routing.rs`
- **Documentation**: [Security Guide - Approval System](../guides/security.md#human-in-the-loop)
- **Three-tier approval**: Once, Session, Permanent

## Configuration

### Tool Policy
- **File**: `~/.config/vtcode/tool_policy.toml`
- **Documentation**: [Tool Policies](../modules/vtcode_tools_policy.md)

### Workspace Configuration
- **File**: `vtcode.toml`
- **Documentation**: [Configuration Guide](../config/config.md)

### Sandbox Configuration
- **File**: `vtcode.toml` (sandbox section)
- **Documentation**: [Security Guide - Sandbox](../guides/security.md#sandbox-configuration)

## Reporting Security Issues

### Responsible Disclosure
1. **Do Not Disclose Publicly** - Report privately first
2. **GitHub Security Advisory** - Use GitHub's security advisory feature
3. **Provide Details** - Include reproduction steps
4. **Coordinate Disclosure** - Allow time for fix

### Contact
- **GitHub**: [Security Advisories](https://github.com/vinhnx/vtcode/security/advisories)
- **Email**: See GitHub profile

---

**Documentation Version**: 1.0
**Last Updated**: October 25, 2025
**Security Model Version**: 1.0
