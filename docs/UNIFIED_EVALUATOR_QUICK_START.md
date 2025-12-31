# UnifiedCommandEvaluator Quick Start Guide

## Basic Usage

### 1. Pure Safety Evaluation (No Policy)

```rust
use crate::command_safety::UnifiedCommandEvaluator;

let evaluator = UnifiedCommandEvaluator::new();
let cmd = vec!["git".to_string(), "status".to_string()];

let result = evaluator.evaluate(&cmd).await?;

if result.allowed {
    println!("Command allowed!");
} else {
    println!("Blocked: {}", result.primary_reason);
}
```

**What it checks:**
- Dangerous command patterns (rm -rf, mkfs, etc.)
- Safety registry rules (git only allows status, log, etc.)
- Shell parsing (decompose bash -lc scripts)
- Cache lookup (fast path)

---

### 2. With Policy Layer

```rust
let evaluator = UnifiedCommandEvaluator::new();
let cmd = vec!["git".to_string(), "status".to_string()];

// Policy says: deny all commands
let result = evaluator
    .evaluate_with_policy(&cmd, false, "policy blocks all")
    .await?;

// Result: blocked (policy denied)
assert!(!result.allowed);
```

**Precedence:**
1. If policy = DENY → block immediately
2. If policy = ALLOW → continue to safety checks
3. Safety rules can still block (override policy)

---

### 3. Adapter for Backward Compatibility

```rust
use crate::command_safety::PolicyAwareEvaluator;

// Without policy (pure safety)
let evaluator = PolicyAwareEvaluator::new();
let result = evaluator.evaluate(&cmd).await?;

// With policy
let evaluator = PolicyAwareEvaluator::with_policy(true, "allow");
let result = evaluator.evaluate(&cmd).await?;

// Update policy dynamically
evaluator.set_policy(false, "deny");
let result = evaluator.evaluate(&cmd).await?;

// Remove policy
evaluator.clear_policy();
let result = evaluator.evaluate(&cmd).await?; // Pure safety again
```

---

## Common Patterns

### Pattern 1: Evaluate Command from User Input

```rust
async fn handle_user_command(cmd_str: &str) -> Result<bool> {
    let evaluator = UnifiedCommandEvaluator::new();
    let cmd_parts: Vec<String> = cmd_str
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    
    let result = evaluator.evaluate(&cmd_parts).await?;
    Ok(result.allowed)
}
```

### Pattern 2: Audit Trail Access

```rust
let evaluator = UnifiedCommandEvaluator::new();
let result = evaluator.evaluate(&cmd).await?;

// Get audit logger for compliance
let audit_log = evaluator.audit_logger();

// Get cache metrics
let cache = evaluator.cache();
println!("Cache size: {}", cache.size().await);
```

### Pattern 3: Check Reason for Denial

```rust
use crate::command_safety::EvaluationReason;

let result = evaluator.evaluate(&cmd).await?;

match &result.primary_reason {
    EvaluationReason::DangerousCommand(desc) => {
        eprintln!("Dangerous: {}", desc);
    }
    EvaluationReason::SafetyDeny(reason) => {
        eprintln!("Safety: {}", reason);
    }
    EvaluationReason::PolicyDeny(reason) => {
        eprintln!("Policy: {}", reason);
    }
    EvaluationReason::CacheHit(allowed, reason) => {
        eprintln!("Cache: {}", reason);
    }
    _ => {}
}

// Get secondary reasons
for secondary in &result.secondary_reasons {
    eprintln!("  - {}", secondary);
}
```

### Pattern 4: Concurrent Evaluation

```rust
use std::sync::Arc;

let evaluator = Arc::new(UnifiedCommandEvaluator::new());

let mut handles = vec![];
for i in 0..10 {
    let eval = evaluator.clone();
    let handle = tokio::spawn(async move {
        let cmd = vec!["git".to_string(), "status".to_string()];
        eval.evaluate(&cmd).await
    });
    handles.push(handle);
}

for handle in handles {
    let result = handle.await??;
    assert!(result.allowed);
}
```

### Pattern 5: Custom Policy per Command

```rust
async fn evaluate_with_context(
    cmd: &[String],
    is_user_initiated: bool,
) -> Result<bool> {
    let evaluator = UnifiedCommandEvaluator::new();
    
    // Auto-allow for trusted users, require safety checks otherwise
    let policy_allowed = is_user_initiated;
    
    let result = evaluator
        .evaluate_with_policy(cmd, policy_allowed, "user_context")
        .await?;
    
    Ok(result.allowed)
}
```

---

## Command Safety Rules Reference

### Always Allowed
- `git status`, `git log`, `git branch`, `git show`, `git diff`
- `cargo build`, `cargo test`, `cargo check`
- `ls`, `cat`, `find .` (without -delete/-exec)
- `grep`

### Always Denied
- `rm -rf /` (or any dangerous variants)
- `mkfs` (filesystem formatting)
- `dd if=... of=...` (disk operations)
- `sudo rm -rf /` (sudo doesn't help)

### Allowed with Caution (Require Confirmation)
- File operations that modify
- Database operations
- Package managers

### Subcommand-Based Rules
- **git**: Only safe operations (status, log, branch, diff, show)
- **find**: Allowed, but -delete and -exec forbidden
- **docker**: No access (dangerous)

---

## Decision Flowchart

```
evaluate(command)
    ↓
[Cache Check]
  ├─ Hit → return cached result
  └─ Miss → continue
    ↓
[Dangerous Pattern Match?]
  ├─ Yes → DENY (DangerousCommand)
  └─ No → continue
    ↓
[Safety Registry Check]
  ├─ DENY → DENY (SafetyDeny)
  ├─ Allow → continue
  └─ Unknown → continue
    ↓
[Shell Parsing (bash -lc)?]
  ├─ Yes → validate each sub-command recursively
  └─ No → continue
    ↓
[Policy Layer (if configured)?]
  ├─ DENY → DENY (PolicyDeny)
  ├─ ALLOW → continue
  └─ None → continue
    ↓
[All Checks Passed]
  └─ ALLOW (SafetyAllow)
```

---

## EvaluationResult Structure

```rust
pub struct EvaluationResult {
    pub allowed: bool,                          // Final decision
    pub primary_reason: EvaluationReason,       // Main reason
    pub secondary_reasons: Vec<String>,         // Additional context
    pub resolved_path: Option<PathBuf>,         // Command path (if resolved)
}

pub enum EvaluationReason {
    PolicyAllow(String),            // Policy allowed this
    PolicyDeny(String),             // Policy denied this
    SafetyAllow,                    // Passed all safety checks
    SafetyDeny(String),             // Failed safety check
    DangerousCommand(String),       // Matched dangerous pattern
    CacheHit(bool, String),         // Retrieved from cache
}
```

---

## Performance Tuning

### Cache Size
```rust
// Default: 1000 entries
let evaluator = UnifiedCommandEvaluator::new();

// Custom size via cache field
// (Currently fixed in constructor, can be made configurable)
```

### Metrics Access
```rust
let cache = evaluator.cache();
println!("Hits: {}", cache.hits().await);
println!("Misses: {}", cache.misses().await);
println!("Hit rate: {:.2}%", cache.hit_rate().await * 100.0);
```

### Typical Performance
- First evaluation (no cache): ~1-5ms
- Cached evaluation: ~0.1ms
- Expected cache hit rate: 70-90%

---

## Error Handling

All operations return `Result<T>`:

```rust
match evaluator.evaluate(&cmd).await {
    Ok(result) => {
        if result.allowed {
            // Execute command
        } else {
            eprintln!("Blocked: {}", result.primary_reason);
        }
    }
    Err(e) => {
        eprintln!("Evaluation error: {}", e);
        // Default to deny on error
    }
}
```

---

## Testing Examples

### Test Safe Command
```rust
#[tokio::test]
async fn test_git_status_allowed() {
    let evaluator = UnifiedCommandEvaluator::new();
    let cmd = vec!["git".to_string(), "status".to_string()];
    let result = evaluator.evaluate(&cmd).await.unwrap();
    assert!(result.allowed);
}
```

### Test Dangerous Command
```rust
#[tokio::test]
async fn test_rm_rf_denied() {
    let evaluator = UnifiedCommandEvaluator::new();
    let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
    let result = evaluator.evaluate(&cmd).await.unwrap();
    assert!(!result.allowed);
}
```

### Test Policy Override
```rust
#[tokio::test]
async fn test_policy_deny_wins() {
    let evaluator = UnifiedCommandEvaluator::new();
    let cmd = vec!["git".to_string(), "status".to_string()];
    let result = evaluator
        .evaluate_with_policy(&cmd, false, "denied")
        .await
        .unwrap();
    assert!(!result.allowed);
}
```

---

## Integration with CommandTool

CommandTool automatically uses UnifiedCommandEvaluator:

```rust
let tool = CommandTool::with_commands_config(workspace, config);

// Internally calls unified evaluator
let invocation = tool.prepare_invocation(&input).await?;
```

No code changes needed - it's automatic!

---

## Migration from CommandPolicyEvaluator

### Old Code (Still Works)
```rust
let policy = CommandPolicyEvaluator::from_config(&config);
if policy.allows(&cmd) {
    execute(&cmd)?;
}
```

### New Code (Better)
```rust
let evaluator = UnifiedCommandEvaluator::new();
let result = evaluator.evaluate(&cmd).await?;
if result.allowed {
    execute(&cmd)?;
}
```

### Gradual Migration
```rust
let adapter = PolicyAwareEvaluator::new();
// Behaves exactly like safety evaluation
// Can be swapped into existing code

// Later: enable policy layer
adapter.set_policy(true, "policy_reason");

// Eventually: replace with UnifiedCommandEvaluator directly
```

---

## Troubleshooting

### Command is Denied but Should be Allowed
1. Check `result.primary_reason` for specific reason
2. Look at `result.secondary_reasons` for context
3. Verify safety registry rules
4. Check if shell parsing is decomposing incorrectly

### Unexpected Cache Behavior
1. Cache is per-evaluator instance (not shared globally)
2. Exact command string match required for cache hit
3. Clear cache by creating new evaluator instance

### Policy Not Being Applied
1. Ensure `evaluate_with_policy()` is used (not `evaluate()`)
2. Check policy_allowed value (true = allow, false = deny)
3. Remember safety rules still apply (override policy)

---

## Resources

- Full documentation: `docs/COMMAND_SAFETY_PHASE5_COMPLETE.md`
- Implementation details: `docs/PHASE5_CHANGES_SUMMARY.md`
- Source code: `vtcode-core/src/command_safety/unified.rs`
- Tests: `vtcode-core/src/command_safety/integration_tests.rs`
