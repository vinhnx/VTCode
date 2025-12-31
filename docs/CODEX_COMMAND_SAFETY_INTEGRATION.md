# Command Safety System Integration: Codex Patterns into VT Code

## Executive Summary

OpenAI's Codex project implements a sophisticated command safety framework with:
- **Safe-by-default allowlists** for specific command subcommands with flag validation
- **Platform-specific dangerous command detection** (Windows PowerShell/CMD vs. Unix)
- **Recursive shell script parsing** for `bash -lc` invocations
- **Argument validation** to detect unsafe option combinations (e.g., `base64 -o`, `find -delete`)
- **Permission escalation tracking** (e.g., `sudo` wrapper detection)

VT Code already has a policy-based system (prefix/glob/regex matching). This document identifies gaps and proposes enhancements.

---

## Current State Comparison

### VT Code (Today)
✅ **Strengths:**
- Deny-list-first evaluation (precedence-based)
- Async evaluation with caching
- Pattern matching (prefix/glob/regex)
- Command resolution to filesystem paths

❌ **Gaps:**
- No per-subcommand safety rules (e.g., `git reset` safe but `git clone` allowed)
- No per-option validation (e.g., blocks `find` entirely rather than just `find -delete`)
- No shell script parsing for `bash -lc` chains
- Minimal Windows/PowerShell-specific logic
- String-based validation only (no AST understanding)

### Codex (Reference)
✅ **Strengths:**
- Granular subcommand allowlists: `git` only allows `branch|status|log|diff|show`
- Per-option blacklists: `find` blocks `-exec`, `-delete`, `-fls`
- Shell chain parsing: understands `bash -lc "cmd1 && cmd2"`
- Platform-specific: separate dangerous command detection for Windows/PowerShell
- Comprehensive test coverage: edge cases like `sudo git reset`

---

## Key Patterns to Adopt

### 1. **Safe-by-Subcommand Model**

**Codex approach:**
```rust
fn is_safe_to_call_with_exec(command: &[String]) -> bool {
    match cmd_name {
        "git" => matches!(command.get(1), Some("branch" | "status" | "log" | "diff" | "show")),
        "cargo" => command.get(1).map(String::as_str) == Some("check"),
        "find" => !command.iter().any(|arg| matches!(arg, "-exec" | "-delete" | ...)),
        _ => false,
    }
}
```

**VT Code should add:** A `SafeCommandRegistry` trait that maps commands to their safe subcommands and forbidden options.

---

### 2. **Windows/PowerShell-Specific Detection**

**Codex approach (windows_dangerous_commands.rs):**
- Parses PowerShell invocations to detect:
  - `Start-Process` with URL arguments
  - `ShellExecute` COM calls with URLs
  - Browser executables (`firefox`, `chrome`) with URLs
  - `mshta` (HTML Application runner)
- Parses CMD `start` subcommand for dangerous operations

**VT Code should add:** 
- Conditional module for Windows: `vtcode-tools/src/command_safety/windows.rs`
- URL detection in PowerShell command chains
- Browser launch prevention

---

### 3. **Shell Chain Parsing**

**Codex approach:**
Uses tree-sitter-bash to parse `bash -lc "..."` and validates each subcommand:
```rust
if command == ["bash", "-lc", "git reset --hard && rm -rf /"] {
    parse_shell_commands() => [["git", "reset", "--hard"], ["rm", "-rf", "/"]]
    // Check each: git reset dangerous, rm -rf dangerous => BLOCK
}
```

**VT Code should add:**
- Leverage existing `tree-sitter` integration in `vtcode-indexer/`
- Parse bash scripts before execution
- Validate each command in the AST

---

### 4. **Per-Subcommand/Option Validation**

**Example: `find` command**

Codex blocks these unsafe options:
- `-exec`, `-execdir`, `-ok`, `-okdir` (arbitrary command execution)
- `-delete` (file deletion)
- `-fls`, `-fprint*`, `-fprintf` (write to files)

```rust
"find" => {
    const UNSAFE_OPTIONS: &[&str] = &["-exec", "-delete", "-fls", ...];
    !command.iter().any(|arg| UNSAFE_OPTIONS.contains(&arg.as_str()))
}
```

**VT Code approach:**
Instead of a flat allow/deny list, use a registry:
```rust
let find_rule = SafeCommandRule::new("find")
    .forbid_options(&["-exec", "-delete", "-fls"])
    .allow_options_subset(&["*"]); // allow all others
```

---

## Implementation Roadmap

### Phase 1: Refactor Command Safety Module (Week 1)
**Goal:** Establish modular structure matching Codex patterns

1. **Create `vtcode-core/src/command_safety/` module:**
   ```
   command_safety/
   ├── mod.rs                    # Public API
   ├── safe_command_registry.rs  # Subcommand + option rules
   ├── dangerous_commands.rs     # Dangerous command detection
   ├── windows.rs                # Windows/PowerShell-specific
   └── shell_parser.rs           # bash -lc chain parsing
   ```

2. **Define `SafeCommandRule` trait:**
   ```rust
   pub trait SafeCommandRule {
       fn command_name(&self) -> &str;
       fn is_safe(&self, args: &[String]) -> SafetyDecision;
       fn forbidden_options(&self) -> &[&str];
       fn allowed_subcommands(&self) -> &[&str];
   }
   ```

3. **Migrate existing validation to registry:**
   - Move hardcoded lists from `commands.rs` → `safe_command_registry.rs`
   - Keep backward compatibility with `command_policy.rs`

### Phase 2: Implement Safe Subcommand Rules (Week 1-2)
**Goal:** Add per-subcommand granularity for high-risk commands

Priority order (by adoption impact):
1. **Git** (most dangerous: `reset`, `rm`, `clean`)
   ```rust
   git => SAFE: [branch, status, log, diff, show]
          DANGEROUS: [reset, rm, clean -fdx]
   ```

2. **Cargo** (limit to read-only: `check`, `build`)
   ```rust
   cargo => SAFE: [check, build, clippy, fmt --check]
            DENY: [clean -target-dir, install]
   ```

3. **Find** (forbid `-exec` family and `-delete`)
   ```rust
   find => SAFE: everything except UNSAFE_OPTIONS
           UNSAFE_OPTIONS: [-exec, -delete, -fls, ...]
   ```

4. **Base64** (forbid `-o` output redirection)
5. **Sed** (only allow `-n {N|M,N}p` pattern)
6. **Ripgrep** (forbid `--pre` and `--hostname-bin`)

### Phase 3: Windows/PowerShell Support (Week 2)
**Goal:** Detect dangerous PowerShell operations

1. **Implement URL detection:**
   ```rust
   fn has_url_argument(tokens: &[String]) -> bool { ... }
   ```

2. **Detect shell.execute patterns:**
   ```rust
   fn is_shell_execute_call(tokens: &[String]) -> bool { ... }
   ```

3. **Block browser launches:**
   ```rust
   const BROWSER_EXES: &[&str] = &["firefox", "chrome", "msedge", ...];
   fn is_browser_launch(exe: &str) -> bool { ... }
   ```

### Phase 4: Shell Chain Parsing (Week 3)
**Goal:** Parse `bash -lc "..."` and validate each command

1. **Leverage tree-sitter-bash:**
   ```rust
   pub fn parse_shell_commands(script: &str) -> Result<Vec<Vec<String>>> { ... }
   ```

2. **Integrate with safety checks:**
   ```rust
   if command == ["bash", "-lc", script] {
       for subcommand in parse_shell_commands(script) {
           if !is_safe(&subcommand) { return false; }
       }
   }
   ```

3. **Handle edge cases:**
   - Variable expansion (best-effort)
   - Quoted arguments
   - Piping and redirection

### Phase 5: Integration with Policy Evaluator (Week 3)
**Goal:** Merge Codex patterns into existing `CommandPolicyEvaluator`

1. **Update `command_policy.rs`:**
   ```rust
   pub async fn evaluate_with_safety(
       &self,
       command: &[String],
   ) -> SafetyDecision {
       // Check deny/allow rules (existing)
       if !self.allows(&command) { return Deny; }
       
       // Check safe command registry (NEW)
       match self.registry.is_safe(&command) {
           SafetyDecision::Deny(reason) => return Deny(reason),
           SafetyDecision::Allow => return Allow,
           SafetyDecision::Unknown => {
               // Fall back to existing policy
               return self.allows(&command) ? Allow : Deny;
           }
       }
   }
   ```

2. **Backward compatibility:**
   - Existing `allows_text()` continues to work
   - New registry is **additive** (more restrictive when matched)

---

## Testing Strategy

### Unit Tests (Codex patterns + VT Code extensions)

**Safe commands that should pass:**
```rust
#[test]
fn git_status_is_safe() { /* safe */ }
#[test]
fn git_reset_is_dangerous() { /* blocks */ }
#[test]
fn bash_lc_with_git_status_is_safe() { /* allow */ }
#[test]
fn bash_lc_with_git_reset_is_dangerous() { /* deny */ }
#[test]
fn find_without_delete_is_safe() { /* allow */ }
#[test]
fn find_delete_is_dangerous() { /* deny */ }
#[test]
fn cargo_check_is_safe() { /* allow */ }
#[test]
fn cargo_clean_is_denied() { /* deny */ }
```

**Windows/PowerShell edge cases:**
```rust
#[test]
fn powershell_start_process_with_url_is_dangerous() { /* deny */ }
#[test]
fn powershell_start_process_without_url_is_safe() { /* allow */ }
#[test]
fn cmd_start_https_is_dangerous() { /* deny */ }
```

### Integration Tests
- Existing `cargo test` suite should pass
- Add roundtrip tests: policy eval + registry eval should be consistent

---

## Files to Create/Modify

### New Files
| File | Purpose | Size |
|------|---------|------|
| `vtcode-core/src/command_safety/mod.rs` | Module exports | 50 lines |
| `vtcode-core/src/command_safety/safe_command_registry.rs` | Subcommand rules | 400 lines |
| `vtcode-core/src/command_safety/dangerous_commands.rs` | Dangerous detection | 150 lines |
| `vtcode-core/src/command_safety/windows.rs` | Windows-specific | 200 lines |
| `vtcode-core/src/command_safety/shell_parser.rs` | bash -lc parsing | 100 lines |
| `vtcode-core/src/command_safety/tests.rs` | Test suite | 500 lines |

### Modified Files
| File | Changes |
|------|---------|
| `vtcode-core/src/tools/command_policy.rs` | Add registry integration (50 lines) |
| `vtcode-core/src/tools/validation/commands.rs` | Deprecate in favor of registry |
| `Cargo.toml` | No new deps (use existing tree-sitter) |

---

## Backward Compatibility

- ✅ Existing `CommandPolicyEvaluator::allows()` unchanged
- ✅ Existing config files (`vtcode.toml`) unchanged
- ✅ Allow/deny lists continue to work
- ⚠️ New rules are **additive** (safer) — won't break existing workflows
- 📋 Document migration path for users with custom policies

---

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Break existing workflows | Low | Registry rules are additive; test against real command logs |
| Performance regression | Low | Cache subcommand checks like policy checks |
| Windows support incomplete | Medium | Start with most common patterns; expand iteratively |
| Parsing edge cases | High | Use tree-sitter; test with real bash scripts |

---

## Success Criteria

1. ✅ All Codex patterns adapted to VT Code architecture
2. ✅ 95%+ test coverage for `command_safety/` module
3. ✅ Zero regressions in `cargo test`
4. ✅ Per-subcommand rules for Git, Cargo, Find, Base64, Sed, Ripgrep
5. ✅ `bash -lc` chain parsing working
6. ✅ Windows PowerShell detection functional
7. ✅ Documentation updated with new patterns

---

## References

- **Codex source:** https://github.com/openai/codex/tree/main/codex-rs/core/src/command_safety
- **VT Code existing:** `vtcode-core/src/tools/command_policy.rs`
- **Related docs:** `docs/COMMAND_SECURITY_MODEL.md`
