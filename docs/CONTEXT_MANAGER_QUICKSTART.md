# Context Manager: Quick-Start Implementation Guide

## TL;DR

Adopt OpenAI Codex's **call/output pairing invariant** pattern to make VT Code's conversation history more robust and crash-safe. Implementation takes 1-2 weeks.

---

## What Problem Does This Solve?

**Scenario**: User cancels a long-running tool, or VT Code crashes mid-execution.

- ❌ **Current**: History may contain "dangling" tool calls without outputs → LLM sees incomplete context → confusion
- ✅ **After fix**: Dangling calls get synthetic "canceled" outputs → LLM sees consistent history → clear context

**Other benefits**:
- Session restoration is more reliable (no crashes from missing state)
- Debugging tool execution issues is easier
- Token counting is more accurate

---

## Implementation Checklist (1-2 weeks)

### Day 1-2: Add Core Types

**File**: `vtcode-core/src/core/agent/state.rs`

```rust
// Add these new types at the top of the file
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStatus {
    Success,
    Failed,
    Canceled,
    Timeout,
}

impl OutputStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Timeout => "timeout",
        }
    }
}
```

**Time**: 30 min

### Day 2-3: Add Validation Methods

**File**: `vtcode-core/src/core/agent/state.rs`

Copy the full implementation from [CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md) sections 2 & 3:
- `ensure_call_outputs_present()`
- `remove_orphan_outputs()`
- `validate_history_invariants()`
- `recover_from_crash()`
- `normalize()` (public wrapper)

**Time**: 2 hours

### Day 3: Write Tests

**File**: `vtcode-core/src/core/agent/state.rs` (tests module)

Copy test module from [CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md) section 4

Run: `cargo test history_invariant_tests`

**Time**: 1 hour

### Day 4: Integration

**Location**: Wherever sessions are loaded/saved/trimmed

Add these calls:

```rust
// In session loading:
state.recover_from_crash().await?;
state.normalize();

// In history trimming:
state.trim_old_history(500);
// normalize() is called inside trim_old_history()
```

**Time**: 1-2 hours

### Day 5: Testing & Validation

```bash
cargo clippy
cargo fmt
cargo test
```

**Time**: 1 hour

---

## Key Functions (Copy-Paste Ready)

### 1. Check if history needs repair

```rust
pub fn validate_history(&self) -> HistoryValidationReport {
    let mut call_ids = std::collections::HashSet::new();
    let mut output_ids = std::collections::HashSet::new();
    
    for item in &self.history {
        // Extract call IDs and output IDs
        // (see full implementation in CONTEXT_MANAGER_IMPLEMENTATION.md)
    }
    
    // Return report with missing_outputs and orphan_outputs
}
```

### 2. Repair dangling calls

```rust
pub fn ensure_call_outputs_present(&mut self) {
    // Find all calls without outputs
    // For each missing output, insert synthetic "canceled" response
    // (see full implementation)
}
```

### 3. Remove broken outputs

```rust
pub fn remove_orphan_outputs(&mut self) {
    // Find all outputs without matching calls
    // Remove them
    // (see full implementation)
}
```

### 4. Full repair

```rust
pub fn normalize(&mut self) {
    self.ensure_call_outputs_present();
    self.remove_orphan_outputs();
    // Log if issues found
}
```

---

## Testing Scenarios

### Scenario 1: User cancels tool execution

```rust
#[test]
fn test_canceled_tool_call() {
    let mut state = TaskRunState::new();
    
    // User starts a grep but cancels it
    state.history.push(HistoryItem::ToolCall {
        call_id: "call-1".to_string(),
        tool_name: "grep_file".to_string(),
        args: Default::default(),
        timestamp: 0,
    });
    
    // No output added (user canceled)
    
    // Validate shows issue
    assert!(!state.validate_history_invariants().is_valid());
    
    // Normalize fixes it
    state.normalize();
    
    // Now valid
    assert!(state.validate_history_invariants().is_valid());
    
    // Verify synthetic output was created
    let last = state.history.last().unwrap();
    assert!(matches!(last, HistoryItem::ToolOutput { .. }));
}
```

### Scenario 2: Crash recovery

```rust
#[test]
async fn test_crash_recovery() {
    let mut state = TaskRunState::new();
    
    // Simulate interrupted session
    state.history.push(HistoryItem::ToolCall {
        call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        args: Default::default(),
        timestamp: 0,
    });
    
    state.history.push(HistoryItem::ToolCall {
        call_id: "call-2".to_string(),
        tool_name: "grep_file".to_string(),
        args: Default::default(),
        timestamp: 0,
    });
    
    // Only call-1 has output
    state.history.push(HistoryItem::ToolOutput {
        call_id: "call-1".to_string(),
        status: "success".to_string(),
        content: "file contents".to_string(),
        timestamp: 0,
    });
    
    // Recover fills in missing output for call-2
    state.recover_from_crash().await.unwrap();
    
    // Now both calls have outputs
    assert_eq!(state.history.len(), 4); // 2 calls + 2 outputs
}
```

### Scenario 3: History trimming

```rust
#[test]
fn test_trim_maintains_invariants() {
    let mut state = TaskRunState::new();
    
    // Build history: 3 call/output pairs
    for i in 0..3 {
        state.history.push(HistoryItem::ToolCall {
            call_id: format!("call-{}", i),
            tool_name: "grep".to_string(),
            args: Default::default(),
            timestamp: i as u64,
        });
        
        state.history.push(HistoryItem::ToolOutput {
            call_id: format!("call-{}", i),
            status: "success".to_string(),
            content: "result".to_string(),
            timestamp: i as u64,
        });
    }
    
    // Trim to keep only the last pair
    state.trim_old_history(2);
    
    // Still valid (pairs are intact)
    assert!(state.validate_history_invariants().is_valid());
    
    // Should have last call + output only
    assert_eq!(state.history.len(), 2);
}
```

---

## Files to Modify

| File | Changes | Complexity |
|------|---------|-----------|
| `vtcode-core/src/core/agent/state.rs` | Add types + methods + tests | Medium |
| Session loading code | Call `recover_from_crash()` + `normalize()` | Low |
| History trimming code | Call `normalize()` after trim | Low |
| `vtcode.toml` | Add optional config section | Low |

**Total estimated effort**: 3-4 days of active coding

---

## Risk Mitigation

- ✅ Opt-in feature via config flag
- ✅ Non-breaking: adds methods, doesn't change existing ones
- ✅ Comprehensive tests before enabling
- ✅ Extensive logging of issues found/fixed
- ✅ Can disable via `enforce_history_invariants = false` in `vtcode.toml`

---

## Success Criteria

- [ ] All new tests pass
- [ ] No performance regression (< 1ms per normalize call)
- [ ] Session loads without errors
- [ ] Dangling calls are created synthetic outputs
- [ ] Orphan outputs are removed
- [ ] `cargo clippy` passes

---

## Next Steps

1. **Read full docs**:
   - [CONTEXT_MANAGER_ANALYSIS.md](./CONTEXT_MANAGER_ANALYSIS.md) - Understanding
   - [CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md) - Code

2. **Implement Phase 1**:
   - Copy code from CONTEXT_MANAGER_IMPLEMENTATION.md
   - Run tests: `cargo test history_invariant_tests`

3. **Integrate**:
   - Add calls in session loading/trimming
   - Monitor logs for invariant violations

4. **Optional Phase 2** (later):
   - Implement standalone `HistoryManager` if beneficial
   - Add telemetry on tool execution patterns

---

## Questions?

Refer to the full analysis and implementation docs in `./docs/CONTEXT_MANAGER_*.md`

Key source: https://github.com/openai/codex/tree/main/codex-rs/core/src/context_manager
