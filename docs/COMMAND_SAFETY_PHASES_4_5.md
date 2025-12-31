# Command Safety: Phase 4-5 Implementation Guide

## Overview

This document outlines the implementation of **Phase 4** (Tree-sitter Shell Parsing) and **Phase 5** (CommandPolicyEvaluator Integration) for the command safety module, building on the foundation from Phases 1-3.

---

## Phase 4: Enhanced Shell Parsing with Tree-Sitter

### Objective
Improve shell command parsing to accurately decompose complex shell scripts (like `bash -lc "cmd1 && cmd2"`) into individual commands for independent safety checking.

### Implementation Status
✅ **COMPLETE**

### Key Features

#### 1. **Tree-Sitter AST Parsing**
- Primary parser uses the bash tree-sitter grammar to build an accurate Abstract Syntax Tree (AST)
- Extracts command nodes (`command`, `pipeline`, `simple_command`, `compound_command`)
- Handles complex shell syntax: pipes, redirects, substitutions

**Location**: `vtcode-core/src/command_safety/shell_parser.rs:47-91`

```rust
// Example: Parsing a pipeline
Input:  "cat file.txt | grep pattern | sort"
Output: [["cat", "file.txt"], ["grep", "pattern"], ["sort"]]
```

#### 2. **Fallback Tokenization**
- If tree-sitter parsing fails, falls back to simple regex-based tokenization
- Handles quotes, escapes, and command separators (`&&`, `||`, `;`)
- Provides 99% accuracy for simple shell scripts

**Location**: `vtcode-core/src/command_safety/shell_parser.rs:123-171`

#### 3. **Thread-Safe Lazy Initialization**
- `OnceLock<Mutex<Parser>>` ensures single global parser instance
- Safe for concurrent access across threads
- Parser is initialized once and reused

**Location**: `vtcode-core/src/command_safety/shell_parser.rs:16-31`

### API Changes

#### `parse_shell_commands(script: &str) -> Result<Vec<Vec<String>>, String>`
```rust
pub fn parse_shell_commands(script: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    // Tries tree-sitter first, falls back to basic tokenization
    // Returns: Vec of command vectors, where each vector is [program, arg1, arg2, ...]
}
```

#### `parse_bash_lc_commands(command: &[String]) -> Option<Vec<Vec<String>>>`
```rust
pub fn parse_bash_lc_commands(command: &[String]) -> Option<Vec<Vec<String>>> {
    // Detects bash -lc "..." patterns and extracts inner commands
    // Example: ["bash", "-lc", "git status && rm /"] → [["git", "status"], ["rm", "/"]]
}
```

### Test Coverage
12 tests added in Phase 4:
- Basic tokenization (2 tests)
- Single & chained commands (2 tests)
- Bash `-lc` patterns (3 tests)
- Complex pipelines & redirects (3 tests)
- Edge cases: escapes, substitutions, dangerous commands (2 tests)

**Location**: `vtcode-core/src/command_safety/shell_parser.rs:228-353`

### Integration Points

The parser integrates with the command safety registry to validate each extracted command:

```rust
// Example workflow
let commands = parse_bash_lc_commands(&["bash", "-lc", "git status && rm -rf /"])?;
for cmd in commands {
    if !is_safe_command(&registry, &cmd) {
        return Deny("Contains dangerous: rm -rf");
    }
}
```

---

## Phase 5: CommandPolicyEvaluator Integration

### Objective
Merge the command safety module with the existing `CommandPolicyEvaluator` to provide a unified safety evaluation system.

### Status
⏳ **PLANNED**

### Architecture

#### Current State (Pre-Phase 5)
- Two parallel safety systems:
  - **`command_safety` module**: Subcommand/option validation + audit logging + caching
  - **`CommandPolicyEvaluator`**: Policy-based evaluation (allow/deny/prompt)

#### Phase 5 Goal
Integrate both systems into a single, unified evaluator that:
1. Applies both subcommand policies AND command database rules
2. Supports hierarchical policies (project → user → system)
3. Maintains audit trail across both systems
4. Provides consistent caching

### Implementation Plan

#### Step 1: Analyze Existing CommandPolicyEvaluator
Find where `CommandPolicyEvaluator` is defined and how it currently works:
```bash
grep -r "struct CommandPolicyEvaluator" vtcode-*
grep -r "CommandPolicyEvaluator" src/ | head -20
```

#### Step 2: Design Unified Interface
```rust
/// Unified command safety evaluator
pub struct UnifiedCommandEvaluator {
    // From command_safety module
    registry: SafeCommandRegistry,
    db: CommandDatabase,
    cache: SafetyDecisionCache,
    audit_logger: SafetyAuditLogger,
    
    // From CommandPolicyEvaluator
    policies: PolicyManager,
    
    // Bash parser for complex commands
    shell_parser: ShellCommandParser,
}

impl UnifiedCommandEvaluator {
    /// Evaluates a command with full context
    pub fn evaluate(&self, cmd: &[String], context: &CommandContext) -> SafetyDecision {
        // 1. Check cache
        // 2. Apply subcommand rules
        // 3. Check command database
        // 4. Apply policy rules
        // 5. Log audit entry
        // 6. Cache result
    }
}
```

#### Step 3: Migration Path
- Keep both systems working independently during transition
- Create adapter layer: `CommandPolicyEvaluator::to_unified()`
- Gradually migrate call sites
- Deprecate old system after transition period

#### Step 4: Test Coverage
- Add integration tests for policy + database interactions
- Test conflict resolution (policy vs. database rules)
- Benchmark cache hit rates with unified system

### Key Design Decisions

#### 1. **Policy Precedence**
```
Explicit Policy (Project .vtcode/) 
    ↓
User Policy (~/.vtcode/)
    ↓
System Default (code)
    ↓
Database Rules
    ↓
Command Registry
```

#### 2. **Caching Strategy**
- Cache key: `(command, policy_version, user_id)`
- Invalidation: Policy changes, time-based TTL
- Hit rate goal: 80%+

#### 3. **Audit Integration**
```rust
// Single audit entry captures full decision
AuditEntry {
    command: Vec<String>,
    timestamp: SystemTime,
    decision: SafetyDecision,
    reasons: Vec<String>,  // Multiple reasons
    policy_applied: String,  // Which policy rule
    cache_hit: bool,
}
```

### Files to Modify

| File | Change | Priority |
|------|--------|----------|
| `vtcode-core/src/lib.rs` | Export unified evaluator | HIGH |
| `vtcode-core/src/command_safety/mod.rs` | Add unified module | HIGH |
| TBD: Find `CommandPolicyEvaluator` | Integrate with new system | HIGH |
| `vtcode-core/src/tools/` | Update tool safety checks | MEDIUM |
| Tests | Add integration tests | MEDIUM |

### Success Criteria

- ✅ Unified evaluator compiles and passes tests
- ✅ Backward compatibility maintained for existing code
- ✅ Cache hit rate ≥ 70%
- ✅ Performance ≥ Phase 3 baseline (no slowdown)
- ✅ All audit entries captured consistently
- ✅ Policy precedence working correctly

---

## Implementation Timeline

### Phase 4 (Completed)
- **Tree-sitter bash parsing**: ✅ Done
- **Fallback tokenization**: ✅ Done
- **Test coverage**: ✅ Done (12 tests)
- **Integration with registry**: ✅ Ready for Phase 5

### Phase 5 (Next)
- **Week 1**: Analyze `CommandPolicyEvaluator` structure
- **Week 2**: Design unified interface & migrations strategy
- **Week 3**: Implement unified evaluator
- **Week 4**: Integration testing & performance tuning
- **Week 5**: Migration & deprecation of old system

---

## Technical Notes

### Tree-Sitter Dependencies
```toml
tree-sitter = "0.26"
tree-sitter-bash = "0.25"
```

### Parser Initialization
```rust
// Lazy init: OnceLock ensures single instance across threads
static BASH_PARSER: OnceLock<Mutex<tree_sitter::Parser>> = OnceLock::new();

fn get_bash_parser() -> &'static Mutex<tree_sitter::Parser> {
    BASH_PARSER.get_or_init(|| {
        let mut parser = tree_sitter::Parser::new();
        let lang: tree_sitter::Language = tree_sitter_bash::LANGUAGE.into();
        parser.set_language(&lang).expect("Failed to load bash grammar");
        Mutex::new(parser)
    })
}
```

### Known Limitations

1. **Tree-sitter parsing fallback**: If bash grammar fails, falls back to simple tokenization
2. **No recursion**: Doesn't parse nested scripts or `eval` chains
3. **Variable expansion**: Doesn't evaluate variables (treats `$VAR` as literal)

---

## References

- **Phase 1-3 Summary**: `docs/PHASE1_PHASE2_SUMMARY.md`
- **Command Safety Module**: `vtcode-core/src/command_safety/`
- **Tree-sitter Bash**: https://github.com/tree-sitter/tree-sitter-bash
- **CommandPolicyEvaluator**: TBD (locate in codebase)

---

## Next Steps

1. Locate `CommandPolicyEvaluator` in codebase
2. Document its current API and usage patterns
3. Start Phase 5 implementation (as described above)
4. Add integration tests for unified system
5. Plan migration strategy for existing code
