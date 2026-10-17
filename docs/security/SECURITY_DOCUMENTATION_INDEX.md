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

**[Unsafe Code Inventory](./UNSAFE_INVENTORY.md)**

- Maintainer inventory of intentional unsafe and FFI boundaries
- Safety invariants and review rules for new sites

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

### Layer 6: Shell Shape Validation and Approval Learning

- **Location**: `crates/codegen/vtcode-safety/src/command_safety/shell_parser.rs`,
  `src/agent/runloop/unified/tool_routing/shell_approval.rs`
- **Documentation**:
  [Security Model - Shell Shape Validation](./SECURITY_MODEL.md#layer-6-shell-shape-validation-and-approval-learning)
- **Dynamic `find` syntax**: Rejected at preflight and excluded from learned read-only families

### Layer 7: Workspace Lifecycle Hook Approval

- **Location**: `crates/codegen/vtcode-config/src/loader/manager.rs` (workspace-hook collection),
  `crates/codegen/vtcode-core/src/hooks/lifecycle/engine.rs` (fail-closed gate + SHA-256 digest),
  `src/agent/runloop/unified/session_setup/hook_approval.rs` (approval overlay)
- **Documentation**:
  [Security Model - Workspace Lifecycle Hook Approval](./SECURITY_MODEL.md#layer-7-workspace-lifecycle-hook-approval),
  [Security Guide - Workspace Lifecycle Hook Approval](../guides/security.md#workspace-lifecycle-hook-approval)
- **Workspace-controlled hooks** (`vtcode.toml`, `.vtcode/`, project profiles, agent-spec files): gate the whole engine
  — no lifecycle hook runs until the exact command set is approved for the workspace; digest-bound approval revalidated
  before every spawn, after config reload, and across primary-agent switches

### Layer 8: Workspace Provider Configuration Trust Boundary

- **Location**: `crates/codegen/vtcode-config/src/loader/manager.rs`
- **Documentation**:
  [Security Model - Workspace Provider Configuration Trust Boundary](./SECURITY_MODEL.md#layer-8-workspace-provider-configuration-trust-boundary),
  [Security Guide - Workspace Provider Configuration Trust Boundary](../guides/security.md#workspace-provider-configuration-trust-boundary)
- **Repository-controlled provider settings**: non-empty custom providers, command-backed auth, and provider
  endpoint/credential overrides are rejected before provider registration; explicit user/system configuration remains
  supported

## Configuration

### Tool Policy

- **Defaults**: `vtcode.toml` in the canonical user config directory
- **Persisted approvals**: `tool-policy.json` in the canonical user config directory
- **Documentation**: [Tool Policies](../modules/vtcode_tools_policy.md),
  [User Data Directories](../guides/user-data-directories.md)

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

**Documentation Version**: 1.0 **Last Updated**: October 25, 2025 **Security Model Version**: 1.0
