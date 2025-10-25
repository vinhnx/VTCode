# Security Quick Reference

## 🔒 Security at a Glance

VT Code implements **5 layers of security** to protect against prompt injection and argument injection attacks.

## Security Layers

| Layer | Protection | Status |
|-------|-----------|--------|
| **1. Command Allowlist** | Only 9 safe commands allowed | Active |
| **2. Argument Validation** | Per-command flag validation | Active |
| **3. Workspace Isolation** | Path boundary enforcement | Active |
| **4. Sandbox Integration** | Network command isolation | Optional |
| **5. Human-in-the-Loop** | Three-tier approval system | Active |

## Allowed Commands

Only these commands can execute:

```
ls       - List directory contents
cat      - Display file contents
cp       - Copy files
head     - Display file beginning
printenv - Show environment variables
pwd      - Print working directory
rg       - Ripgrep text search
sed      - Stream editor
which    - Locate programs
```

All other commands are **blocked by default**.

## Blocked Patterns

### ❌ Dangerous Commands
```bash
rm, sudo, chmod, docker, curl (without sandbox)
```

### ❌ Execution Flags
```bash
--pre, --pre-glob, -exec, -e, --command
```

### ❌ Path Traversal
```bash
../../../etc/passwd
/etc/passwd (outside workspace)
```

### ❌ Command Chaining
```bash
ls; curl evil.com
ls && rm -rf /
```

## Approval System

| Level | Scope | Persistence |
|-------|-------|-------------|
| **Approve Once** | Single execution | None |
| **Allow for Session** | Current session | Memory only |
| **Always Allow** | All sessions | Saved to policy |

## Quick Security Checks

### Safe Operations
```bash
# Search in workspace
rg -i "pattern" .

# Read workspace file
cat ./src/main.rs

# List workspace directory
ls -la ./src
```

### ❌ Blocked Operations
```bash
# Preprocessor execution
rg --pre "bash" pattern .

# Path escape
cat ../../../etc/passwd

# Network without sandbox
curl https://evil.com
```

## Configuration Files

| File | Purpose |
|------|---------|
| `~/.config/vtcode/tool_policy.toml` | Tool approval policies |
| `vtcode.toml` | Workspace configuration |
| `.vtcode/logs/` | Execution audit logs |

## Security Checklist

- [ ] Review tool approvals regularly
- [ ] Use "Approve Once" for unfamiliar operations
- [ ] Enable sandbox for network commands
- [ ] Monitor logs for suspicious activity
- [ ] Don't process untrusted code
- [ ] Keep VT Code updated

## Emergency Response

If you suspect a security issue:

1. **Stop execution** - Press Ctrl+C
2. **Review logs** - Check `.vtcode/logs/`
3. **Report issue** - Open GitHub security advisory
4. **Update policies** - Revoke suspicious approvals

## Learn More

- [Complete Security Model](./SECURITY_MODEL.md)
- [Security Audit](./SECURITY_AUDIT.md)
- [Security Guide](./guides/security.md)
- 📋 [Tool Policies](./vtcode_tools_policy.md)

## Security Updates

**Latest Security Fix**: October 25, 2025
- Fixed ripgrep `--pre` flag vulnerability (HIGH)
- Added comprehensive security test suite
- Enhanced documentation

See [SECURITY_FIX_2025-10-25.md](./SECURITY_FIX_2025-10-25.md) for details.

---

**Security Model Version**: 1.0  
**Last Updated**: October 25, 2025
