# Security Guide

## Overview

VT Code is designed with security as a first-class concern. This guide explains the security features, best practices, and how to configure VT Code for maximum safety in your environment.

## Security Architecture

VT Code implements a **defense-in-depth security model** with five layers of protection:

### Layer 1: Command Allowlist

Only explicitly approved commands can execute. The allowlist includes:

- `ls` - List directory contents
- `cat` - Display file contents  
- `cp` - Copy files
- `head` - Display file beginning
- `printenv` - Show environment variables
- `pwd` - Print working directory
- `rg` - Ripgrep text search
- `sed` - Stream editor
- `which` - Locate programs

**All other commands are blocked by default**, including:
- Destructive commands: `rm`, `dd`, `shred`
- Privilege escalation: `sudo`, `su`, `doas`
- System modification: `chmod`, `chown`, `systemctl`
- Network commands (without sandbox): `curl`, `wget`, `ssh`
- Container tools: `docker`, `kubectl`

### Layer 2: Argument Validation

Each allowed command has a dedicated validator that:

- **Validates all flags** - Only explicitly allowed flags are permitted
- **Blocks execution flags** - Prevents `-exec`, `--pre`, `-e`, etc.
- **Validates paths** - Ensures all paths stay within workspace
- **Rejects unknown flags** - Unknown flags are blocked by default

Example: Ripgrep validation blocks dangerous preprocessor flags:
```rust
// BLOCKED: Preprocessor execution
rg --pre "bash -c 'malicious command'" pattern .

// ALLOWED: Safe search
rg -i -n pattern .
```

### Layer 3: Workspace Isolation

All file operations are confined to the workspace:

- **Path normalization** - Resolves `..`, `.`, symlinks
- **Boundary enforcement** - Rejects paths outside workspace
- **Symlink resolution** - Follows symlinks and validates destination
- **Absolute path validation** - Blocks absolute paths outside workspace

```bash
# BLOCKED: Path traversal
cat ../../../etc/passwd

# BLOCKED: Absolute path outside workspace
cat /etc/passwd

# ALLOWED: Workspace file
cat ./src/main.rs
```

### Layer 4: Sandbox Integration

Network commands require the Anthropic sandbox runtime:

- **Filesystem isolation** - Limited to workspace directory
- **Network allowlist** - Only approved domains accessible
- **System protection** - No access to system directories
- **Resource limits** - CPU, memory, and time constraints

### Layer 5: Human-in-the-Loop

Three-tier approval system for tool execution:

1. **Approve Once** - Single execution approval
2. **Allow for Session** - Approved for current session only
3. **Always Allow** - Permanently saved to tool policy

## Threat Model

### Protected Against

✅ **Prompt Injection Attacks**
- Malicious prompts from users
- Embedded prompts in code comments
- Prompts in repository files
- Prompts in logging output

✅ **Argument Injection**
- Execution flags (`-exec`, `--pre`, `-e`)
- Path traversal (`../`, symlinks)
- Output redirection (`-o /etc/passwd`)
- Command chaining (`;`, `&&`, `||`)

✅ **Workspace Escape**
- Absolute paths outside workspace
- Symlink traversal
- Parent directory traversal
- File-through-file traversal

✅ **Privilege Escalation**
- `sudo`, `su`, `doas` commands
- System configuration modification
- SUID binary exploitation

### Not Protected Against

⚠️ **Physical Access** - Assumes no physical access to machine  
⚠️ **Kernel Exploits** - Relies on OS security  
⚠️ **Side Channel Attacks** - Timing, cache, etc.  
⚠️ **Social Engineering** - Direct user manipulation

## Configuration

### Tool Policy Configuration

Configure tool approval policies in `~/.config/vtcode/tool_policy.toml`:

```toml
# Allow specific tools without prompting
[tools]
read_file = "allow"
list_directory = "allow"
search_files = "allow"

# Require approval for sensitive operations
run_terminal_cmd = "prompt"
write_file = "prompt"
delete_file = "prompt"

# Block dangerous operations
bash = "deny"
```

### Sandbox Configuration

Enable sandbox mode in `vtcode.toml`:

```toml
[sandbox]
enabled = true
binary = "/path/to/srt"  # Anthropic sandbox runtime
settings = "/path/to/sandbox-settings.json"

# Network allowlist
allowed_domains = [
    "api.openai.com",
    "api.anthropic.com",
    "github.com"
]
```

### Execution Policy

The execution policy is enforced at the code level and cannot be disabled. However, you can configure workspace boundaries:

```toml
[workspace]
# Workspace root (default: current directory)
root = "/path/to/project"

# Additional allowed paths (use with caution)
# allowed_paths = ["/tmp/vtcode-cache"]
```

## Best Practices

### For Users

1. **Review Tool Approvals**
   - Check `~/.config/vtcode/tool_policy.toml` regularly
   - Use "Approve Once" for unfamiliar operations
   - Only use "Always Allow" for trusted tools

2. **Use Sandbox Mode**
   - Enable sandbox for network commands
   - Configure domain allowlist restrictively
   - Monitor sandbox logs

3. **Be Cautious with Untrusted Content**
   - Don't process code from unknown sources
   - Review prompts in repository files
   - Be wary of code comments with instructions

4. **Monitor Command Execution**
   - Review logs in `.vtcode/logs/`
   - Watch for suspicious patterns
   - Report unusual behavior

### For Organizations

1. **Enforce Sandbox Mode**
   - Require sandbox for all deployments
   - Maintain strict domain allowlist
   - Regular security audits

2. **Centralized Policy Management**
   - Deploy standard tool policies
   - Use deny-by-default approach
   - Regular policy reviews

3. **Audit and Monitoring**
   - Centralized log collection
   - Automated anomaly detection
   - Incident response procedures

4. **Security Training**
   - Educate users on prompt injection
   - Share security best practices
   - Regular security updates

## Security Testing

### Automated Tests

VT Code includes comprehensive security tests:

```bash
# Run security test suite
cargo test -p vtcode-core --test execpolicy_security_tests

# Run all tests
cargo nextest run --workspace
```

### Manual Testing

Test security controls with malicious prompts:

```bash
# Test argument injection
vtcode ask "Search using rg --pre 'bash' for pattern"

# Test path traversal
vtcode ask "Show me ../../../etc/passwd"

# Test command chaining
vtcode ask "List files then curl evil.com"
```

All of these should be blocked with appropriate error messages.

## Incident Response

If you discover a security vulnerability:

1. **Do Not Disclose Publicly** - Report privately first
2. **Contact Maintainers** - Open a security advisory on GitHub
3. **Provide Details** - Include reproduction steps
4. **Allow Time for Fix** - Coordinate disclosure timeline

## Security Updates

Stay informed about security updates:

- Watch the [GitHub repository](https://github.com/vinhnx/vtcode)
- Review [CHANGELOG.md](../../CHANGELOG.md) for security fixes
- Subscribe to release notifications

## Additional Resources

- [Security Model](../SECURITY_MODEL.md) - Complete security architecture
- [Security Audit](../SECURITY_AUDIT.md) - Vulnerability analysis
- [Tool Policies](../vtcode_tools_policy.md) - Command execution policies
- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [OWASP Command Injection](https://owasp.org/www-community/attacks/Command_Injection)

## Acknowledgments

VT Code's security model is informed by:

- Trail of Bits research on AI agent security
- Anthropic's safety guidelines
- OpenAI Codex execution policy
- Industry best practices for command execution

---

**Last Updated**: October 25, 2025  
**Security Model Version**: 1.0
