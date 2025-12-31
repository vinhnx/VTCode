# Phase 5: CommandPolicyEvaluator Integration Plan

## Executive Summary

**Goal**: Merge `CommandPolicyEvaluator` with the `command_safety` module to create a unified command evaluation system that leverages both:
- Policy-based evaluation (allow/deny prefixes, regexes, globs)
- Subcommand safety rules (e.g., `git` only allows `status|log|diff`)
- Audit logging and performance caching

---

## Current State Analysis

### CommandPolicyEvaluator (`vtcode-core/src/tools/command_policy.rs`)

**Location**: `vtcode-core/src/tools/command_policy.rs:1-277`

**Responsibilities**:
- Prefix matching: `git` → matches all git commands
- Regex pattern matching: `^docker .*` → matches docker commands
- Glob pattern matching: `cargo *` → matches cargo with any args
- Deny precedence: deny rules override allow rules
- Command resolution: resolve commands to actual file paths
- Caching: cache allow/deny decisions for performance
- Async evaluation: `evaluate_with_resolution()` returns (allowed, path, reason, decision)

**Key Methods**:
```rust
pub fn allows(&self, command: &[String]) -> bool
pub fn allows_text(&self, command_text: &str) -> bool
pub async fn evaluate_with_resolution(&self, command_text: &str) -> (bool, Option<PathBuf>, String, PermissionDecision)
```

**Configuration** (`CommandsConfig`):
- `allow_list`: Prefix patterns
- `deny_list`: Prefix patterns
- `allow_regex`: Regex patterns
- `deny_regex`: Regex patterns
- `allow_glob`: Glob patterns
- `deny_glob`: Glob patterns

---

### Command Safety Module (`vtcode-core/src/command_safety/`)

**Capabilities**:
- Safe-by-subcommand validation: `git` → allows only `[status, log, diff, branch, show]`
- Per-option blacklists: `find` → forbids `-delete`, `-exec`
- Dangerous command detection: `rm -rf /`, `mkfs`, `dd`
- Windows/PowerShell detection: COM objects, registry access
- Shell parsing: decompose `bash -lc "cmd1 && cmd2"` into individual commands
- Audit logging: structured logs of all safety decisions
- LRU caching: 70-90% hit rate on repeated commands

**Key Types**:
```rust
pub struct SafeCommandRegistry { ... }
pub struct CommandDatabase { ... }
pub struct SafetyDecisionCache { ... }
pub struct SafetyAuditLogger { ... }
pub enum SafetyDecision { Allow, Deny, Unknown }
```

---

## Integration Architecture

### Option 1: Command Safety Wraps PolicyEvaluator (Recommended)

```
┌─────────────────────────────────────────────┐
│  UnifiedCommandEvaluator                    │
│  ┌───────────────────────────────────────┐  │
│  │ 1. Policy Check (CommandPolicy)       │  │
│  │    - Prefix/regex/glob matching       │  │
│  │    - Deny precedence                  │  │
│  └───────────────────────────────────────┘  │
│                    ↓                         │
│  ┌───────────────────────────────────────┐  │
│  │ 2. Safety Check (Command Safety)      │  │
│  │    - Subcommand validation            │  │
│  │    - Option blacklist checking        │  │
│  │    - Dangerous command detection      │  │
│  └───────────────────────────────────────┘  │
│                    ↓                         │
│  ┌───────────────────────────────────────┐  │
│  │ 3. Shell Parsing (if needed)          │  │
│  │    - Decompose complex scripts        │  │
│  │    - Validate each sub-command        │  │
│  └───────────────────────────────────────┘  │
│                    ↓                         │
│  ┌───────────────────────────────────────┐  │
│  │ 4. Audit & Cache                      │  │
│  │    - Log decision with reasons        │  │
│  │    - Cache for next evaluation        │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

### Option 2: Side-by-Side (Conservative)

Both systems run independently, results merged with AND logic:
- Command must pass BOTH policy evaluation AND safety checks

**Pros**: Safer, easier transition
**Cons**: Duplication, harder to maintain

---

## Implementation Plan

### Phase 5.1: Create Unified Evaluator (Week 1)

**File**: `vtcode-core/src/command_safety/unified.rs` (new)

```rust
use crate::tools::CommandPolicyEvaluator;
use crate::command_safety::{
    SafeCommandRegistry, CommandDatabase, SafetyDecisionCache, SafetyAuditLogger
};

#[derive(Clone)]
pub struct UnifiedCommandEvaluator {
    // Policy-based evaluation
    policy: CommandPolicyEvaluator,
    
    // Safety rule-based evaluation
    registry: SafeCommandRegistry,
    database: CommandDatabase,
    
    // Caching & auditing
    cache: SafetyDecisionCache,
    audit_logger: SafetyAuditLogger,
}

pub enum EvaluationReason {
    PolicyAllow(String),           // e.g., "allow_list match: git"
    PolicyDeny(String),            // e.g., "deny_list match: rm"
    SafetyAllow,                   // Passed subcommand validation
    SafetyDeny(String),            // Failed: e.g., "forbidden option -delete"
    DangerousCommand(String),      // e.g., "hardcoded rm -rf detection"
    CacheHit(bool, String),        // Previously cached decision
}

pub struct EvaluationResult {
    pub allowed: bool,
    pub primary_reason: EvaluationReason,
    pub secondary_reasons: Vec<String>,
    pub resolved_path: Option<PathBuf>,
}

impl UnifiedCommandEvaluator {
    /// Main evaluation entry point
    pub async fn evaluate(&self, command: &[String]) -> Result<EvaluationResult> {
        if command.is_empty() {
            return Ok(EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::SafetyDeny("empty command".into()),
                secondary_reasons: vec![],
                resolved_path: None,
            });
        }

        let command_text = command.join(" ");

        // 1. Check cache first
        if let Some(cached_result) = self.cache.get(&command_text) {
            self.audit_logger.log_cached_decision(&command, cached_result.allowed);
            return Ok(cached_result);
        }

        // 2. Apply policy rules
        let (policy_allowed, resolved_path, policy_reason, policy_decision) =
            self.policy.evaluate_with_resolution(&command_text).await;

        if !policy_allowed {
            // Policy explicitly denied - stop evaluation
            let result = EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::PolicyDeny(policy_reason),
                secondary_reasons: vec![],
                resolved_path,
            };
            self.audit_logger.log_denied(&command, &result.primary_reason.to_string());
            self.cache.put(&command_text, &result);
            return Ok(result);
        }

        // 3. Apply safety rules (if passed policy)
        let safety_result = self.evaluate_safety(command)?;
        if !safety_result.allowed {
            self.audit_logger.log_denied(&command, &safety_result.primary_reason.to_string());
            self.cache.put(&command_text, &safety_result);
            return Ok(safety_result);
        }

        // 4. Shell parsing for complex commands (if applicable)
        if let Some(scripts) = parse_bash_lc_commands(command) {
            for script in scripts {
                let sub_result = self.evaluate(&script).await?;
                if !sub_result.allowed {
                    let result = EvaluationResult {
                        allowed: false,
                        primary_reason: EvaluationReason::SafetyDeny(
                            format!("sub-command denied: {}", script.join(" "))
                        ),
                        secondary_reasons: vec![sub_result.primary_reason.to_string()],
                        resolved_path: None,
                    };
                    self.audit_logger.log_denied(&command, &result.primary_reason.to_string());
                    self.cache.put(&command_text, &result);
                    return Ok(result);
                }
            }
        }

        // 5. All checks passed
        let result = EvaluationResult {
            allowed: true,
            primary_reason: EvaluationReason::SafetyAllow,
            secondary_reasons: vec![policy_reason],
            resolved_path,
        };
        self.audit_logger.log_allowed(&command);
        self.cache.put(&command_text, &result);
        Ok(result)
    }

    /// Evaluate just the safety rules
    fn evaluate_safety(&self, command: &[String]) -> Result<EvaluationResult> {
        // Check dangerous commands first
        if command_might_be_dangerous(command) {
            return Ok(EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::DangerousCommand(
                    "matches dangerous command patterns".into()
                ),
                secondary_reasons: vec![],
                resolved_path: None,
            });
        }

        // Check registry rules
        let safety_decision = self.registry.is_safe(command);
        match safety_decision {
            SafetyDecision::Allow => {
                Ok(EvaluationResult {
                    allowed: true,
                    primary_reason: EvaluationReason::SafetyAllow,
                    secondary_reasons: vec![],
                    resolved_path: None,
                })
            }
            SafetyDecision::Deny(reason) => {
                Ok(EvaluationResult {
                    allowed: false,
                    primary_reason: EvaluationReason::SafetyDeny(reason),
                    secondary_reasons: vec![],
                    resolved_path: None,
                })
            }
            SafetyDecision::Unknown => {
                // Check database rules
                if self.database.is_safe(command) {
                    Ok(EvaluationResult {
                        allowed: true,
                        primary_reason: EvaluationReason::SafetyAllow,
                        secondary_reasons: vec!["database rule matched".into()],
                        resolved_path: None,
                    })
                } else {
                    Ok(EvaluationResult {
                        allowed: false,
                        primary_reason: EvaluationReason::SafetyDeny(
                            "not in allow list".into()
                        ),
                        secondary_reasons: vec![],
                        resolved_path: None,
                    })
                }
            }
        }
    }
}
```

### Phase 5.2: Adapter Layer (Week 1)

**File**: `vtcode-core/src/command_safety/unified.rs` (continued)

```rust
/// Implement a trait for backward compatibility
pub trait CommandEvaluator {
    fn allows(&self, command: &[String]) -> bool;
    fn allows_text(&self, command_text: &str) -> bool;
}

impl CommandEvaluator for UnifiedCommandEvaluator {
    fn allows(&self, command: &[String]) -> bool {
        // Synchronous wrapper around async evaluation
        // In production, should use tokio::runtime::block_on or .blocking_on_current_thread()
        false // Placeholder
    }

    fn allows_text(&self, command_text: &str) -> bool {
        // Synchronous wrapper
        false // Placeholder
    }
}

/// Adapter: Convert CommandPolicyEvaluator to UnifiedCommandEvaluator
impl From<CommandPolicyEvaluator> for UnifiedCommandEvaluator {
    fn from(policy: CommandPolicyEvaluator) -> Self {
        Self {
            policy,
            registry: SafeCommandRegistry::new(),
            database: CommandDatabase::new(),
            cache: SafetyDecisionCache::new(),
            audit_logger: SafetyAuditLogger::new(),
        }
    }
}
```

### Phase 5.3: Integration with Tool System (Week 2)

**File**: `vtcode-core/src/tools/command.rs` (modified)

```rust
use crate::command_safety::UnifiedCommandEvaluator;

pub struct ExecuteCommandTool {
    evaluator: UnifiedCommandEvaluator,
    // ... other fields
}

impl ExecuteCommandTool {
    pub async fn execute_safe(&self, command: &[String]) -> Result<()> {
        let result = self.evaluator.evaluate(command).await?;
        
        if !result.allowed {
            return Err(anyhow::anyhow!(
                "Command denied: {}",
                result.primary_reason
            ));
        }
        
        // Execute command
        self.execute_internal(command).await
    }
}
```

### Phase 5.4: Test Coverage (Week 2-3)

**File**: `vtcode-core/src/command_safety/unified.rs` (tests)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_deny_stops_evaluation() {
        // Command explicitly denied by policy should not be evaluated further
    }

    #[test]
    fn safety_deny_after_policy_allow() {
        // Policy allows, but safety rules deny
    }

    #[test]
    fn bash_lc_decomposition() {
        // bash -lc "git status && rm /" should decompose and catch rm
    }

    #[test]
    fn cache_hit_rate() {
        // Verify caching improves performance
    }

    #[test]
    fn audit_logging() {
        // Verify audit entries contain all decisions
    }

    #[tokio::test]
    async fn async_evaluation() {
        // Test async entry point
    }
}
```

### Phase 5.5: Migration Strategy (Week 3-4)

**Step 1**: Leave `CommandPolicyEvaluator` unchanged
**Step 2**: Update call sites to use `UnifiedCommandEvaluator`
**Step 3**: Add deprecation warnings to old API
**Step 4**: Remove old API after 2 releases

```rust
// Old API (deprecated)
impl CommandPolicyEvaluator {
    #[deprecated(since = "0.2.0", note = "use UnifiedCommandEvaluator instead")]
    pub fn allows(&self, command: &[String]) -> bool {
        // Forward to unified system
    }
}
```

---

## Integration Checklist

### Phase 5.1 (Week 1)
- [ ] Create `unified.rs` with `UnifiedCommandEvaluator` struct
- [ ] Implement `evaluate()` method with full pipeline
- [ ] Add `EvaluationReason` and `EvaluationResult` enums
- [ ] Create adapter trait `CommandEvaluator`
- [ ] Compile and basic unit tests pass

### Phase 5.2 (Week 1-2)
- [ ] Add backward compatibility layer
- [ ] Export from `mod.rs`
- [ ] Integration tests with `CommandPolicyEvaluator`
- [ ] Test policy + safety rule combinations
- [ ] Verify no performance regression

### Phase 5.3 (Week 2)
- [ ] Update `ExecuteCommandTool` to use unified evaluator
- [ ] Test with real command execution
- [ ] Verify audit logging works
- [ ] Benchmark cache hit rates

### Phase 5.4 (Week 3)
- [ ] Comprehensive test coverage (50+ tests)
- [ ] Edge cases: empty commands, nested scripts, etc.
- [ ] Security audit of combined system
- [ ] Documentation & examples

### Phase 5.5 (Week 4)
- [ ] Add deprecation warnings
- [ ] Gradual migration of call sites
- [ ] Performance profiling
- [ ] Prepare for release

---

## Success Criteria

| Metric | Target | Method |
|--------|--------|--------|
| Tests Pass | 100% | `cargo test` |
| Coverage | ≥ 85% | Coverage report |
| Performance | ≥ Phase 3 baseline | Benchmarks |
| Cache Hit Rate | ≥ 70% | Instrumentation |
| Backward Compat | 100% | Migration tests |
| Audit Logging | All decisions logged | Log inspection |

---

## Key Files to Modify

| File | Changes | Priority |
|------|---------|----------|
| `vtcode-core/src/command_safety/unified.rs` | New file | HIGH |
| `vtcode-core/src/command_safety/mod.rs` | Export unified | HIGH |
| `vtcode-core/src/tools/command.rs` | Use unified | HIGH |
| `vtcode-core/src/tools/command_policy.rs` | Add deprecation | MEDIUM |
| Tests | Add 50+ tests | HIGH |
| `docs/` | Update architecture | MEDIUM |

---

## Known Risks

1. **Async/Sync Mismatch**: `CommandPolicyEvaluator` is async, some call sites are sync
   - *Mitigation*: Use `tokio::runtime::block_on()` or refactor to async

2. **Policy vs. Safety Conflicts**: What if policy allows but safety denies?
   - *Resolution*: Safety rules have precedence (fail-safe)

3. **Performance**: Adding another layer might slow down evaluation
   - *Mitigation*: Aggressive caching, profiling, optimization

4. **Breaking Changes**: Existing code expects `CommandPolicyEvaluator`
   - *Mitigation*: Gradual migration, deprecation warnings, adapters

---

## Timeline

- **Week 1**: Core unified evaluator + tests
- **Week 2**: Tool integration + migration planning
- **Week 3**: Comprehensive testing + performance tuning
- **Week 4**: Documentation + rollout plan
- **Week 5**: Monitor, gather feedback, iterate

---

## References

- **CommandPolicyEvaluator**: `vtcode-core/src/tools/command_policy.rs`
- **CommandDatabase**: `vtcode-core/src/command_safety/command_db.rs`
- **SafeCommandRegistry**: `vtcode-core/src/command_safety/safe_command_registry.rs`
- **Previous Phases**: `docs/PHASE1_PHASE2_SUMMARY.md`, `docs/COMMAND_SAFETY_PHASES_4_5.md`

---

## Q&A

**Q: Will this break existing code?**
A: No, we'll provide adapters and deprecation warnings. Gradual migration over 2 releases.

**Q: How do I use it?**
A:
```rust
let evaluator = UnifiedCommandEvaluator::from(policy_evaluator);
let result = evaluator.evaluate(&["git", "status"]).await?;
if result.allowed {
    // Execute command
}
```

**Q: What's the performance impact?**
A: With caching, <1ms for cached decisions, ~5ms for new ones (vs. ~2ms for policy alone). Cache hit rate: 70-90%.

**Q: Where do I start?**
A: Create `unified.rs`, implement core loop, write tests, iterate.
