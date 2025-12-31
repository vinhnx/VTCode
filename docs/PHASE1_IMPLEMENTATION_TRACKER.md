# Phase 1 Implementation Tracker

**Project**: Context Manager Invariant Enforcement
**Phase**: 1 - Core Invariants
**Timeline**: 2 weeks
**Status**: Ready to Start

---

## Daily Checklist

### Day 1-2: Add Core Types

**Objective**: Define types for output status and validation

**File**: `vtcode-core/src/core/agent/state.rs`

- [x] Add `ToolCallId` struct
- [x] Add `OutputStatus` enum (Success, Failed, Canceled, Timeout)
- [x] Add `PairableHistoryItem` enum
- [x] Add `MissingOutput` struct
- [x] Add `HistoryValidationReport` struct

**Verification**:
```bash
cargo check
```

**Time**: 30-60 min (✅ COMPLETE)

**Code Reference**: CONTEXT_MANAGER_IMPLEMENTATION.md Section 1

---

### Day 2-3: Implement Validation Methods

**Objective**: Add validation and normalization methods

**File**: `vtcode-core/src/core/agent/state.rs`

- [x] Implement `ensure_call_outputs_present(&mut self)`
- [x] Implement `remove_orphan_outputs(&mut self)`
- [x] Implement `validate_history_invariants(&self) -> HistoryValidationReport`
- [x] Implement helper methods:
  - [x] `extract_pairable(&self, item) -> Option<PairableHistoryItem>` (Note: Implemented via direct Message inspection in `validate_history_invariants`)
  - [x] `create_synthetic_output(...) -> HistoryItem` (Note: Implemented via `Message::tool_response`)
  - [x] `status_from_str(&self, status) -> OutputStatus` (Note: Implemented `as_str` on `OutputStatus`)

**Verification**:
```bash
cargo check
cargo clippy
```

**Time**: 2-3 hours (✅ COMPLETE)

**Code Reference**: CONTEXT_MANAGER_IMPLEMENTATION.md Section 2

---

### Day 3: Write Tests

**Objective**: Comprehensive test coverage for all scenarios

**File**: `vtcode-core/src/core/agent/state.rs` (test module)

- [x] Add test: `test_ensure_call_outputs_present()`
- [x] Add test: `test_remove_orphan_outputs()`
- [x] Add test: `test_normalize()`
- [x] Add test: `test_validation_report_summary()`
- [x] Add test: `test_canceled_tool_call()` (scenario-based)
- [x] Add test: `test_history_trimming_maintains_invariants()` (scenario-based)

**Verification**:
```bash
cargo test -p vtcode-core --lib core::agent::state::tests
```

**Expected**: All 10 tests pass (✅ COMPLETE)

**Time**: 1-2 hours (✅ COMPLETE)

**Code Reference**: CONTEXT_MANAGER_IMPLEMENTATION.md Section 4

---

### Day 4: Integration Points

**Objective**: Call normalization in real code paths

**Integration 1: Session Loading**

**File**: Session restoration code (find via grep)

```bash
grep -r "load_from_file\|restore_session" vtcode-core/src/
```

**Changes**:
```rust
// After loading history from disk
state.recover_from_crash().await?;
state.normalize();
```

- [ ] Find session loading code
- [ ] Add `recover_from_crash()` call
- [ ] Add `normalize()` call
- [ ] Add logging

**Integration 2: History Trimming**

**File**: History trimming code (likely in TaskRunState)

**Changes**:
```rust
pub fn trim_old_history(&mut self, keep_count: usize) {
    // Remove items
    self.history.drain(0..to_remove);
    
    // Ensure invariants maintained
    self.normalize();
}
```

- [ ] Find history trimming code
- [ ] Add `normalize()` call after trim
- [ ] Test that pairs are maintained

**Integration 3: Before Sending to LLM**

**File**: Where history is converted to prompts

**Changes**:
```rust
pub fn get_history_for_prompt(&mut self) -> Vec<ResponseItem> {
    self.normalize(); // Ensure valid state
    // ... rest of method
}
```

- [ ] Call `normalize()` before prompt generation
- [ ] Verify no performance regression

**Verification**:
```bash
cargo check
cargo clippy
```

**Time**: 2-3 hours

**Code Reference**: CONTEXT_MANAGER_IMPLEMENTATION.md Section 3

---

### Day 4-5: Configuration

**Objective**: Add optional config flags

**File**: `vtcode.toml` (example)

- [ ] Add `[context]` section
- [ ] Add `enforce_history_invariants = false`
- [ ] Add `auto_recover_from_crash = true`
- [ ] Add `warn_on_invariant_violations = true`

**File**: Config loading code

- [ ] Create `ContextConfig` struct
- [ ] Load from `vtcode.toml`
- [ ] Add conditional logging

**Time**: 30-45 min

**Code Reference**: CONTEXT_MANAGER_IMPLEMENTATION.md Section 5

---

### Day 5: Final Verification

**Objective**: Ensure everything works together

**Checklist**:

```bash
# 1. Run all tests
cargo test

# 2. Run only our new tests
cargo test history_invariant_tests

# 3. Check for warnings
cargo clippy

# 4. Format code
cargo fmt

# 5. Build release (optional, slow)
cargo build --release

# 6. Check compilation
cargo check
```

**Expected Results**:
- ✅ All tests pass
- ✅ No clippy warnings
- ✅ Code formatted
- ✅ Zero compilation errors

**Logs to Check**:
- [ ] Enable `RUST_LOG=debug`
- [ ] Run a test
- [ ] Verify warnings/info messages appear

**Time**: 1-2 hours

---

## Code Snippets (Copy-Paste Ready)

### Snippet 1: Enable Logging

Add to test or main code:
```rust
use tracing::{warn, info};

// In function
tracing::warn!("Tool call {} missing output", call_id);
tracing::info!("Created synthetic output for call {}", call_id);
```

### Snippet 2: Call normalize() everywhere

Search for these patterns and add `self.normalize()`:

```bash
# After removing items
self.history.remove(idx);
self.normalize();

# After loading
let state = load_from_disk()?;
state.recover_from_crash().await?;

# Before using
history = state.get_history_for_prompt();
```

### Snippet 3: Log validation results

```rust
let report = self.validate_history_invariants();
if !report.is_valid() {
    tracing::warn!("History validation: {}", report.summary());
    // Optionally auto-fix
    self.normalize();
}
```

---

## Testing Checklist

### Unit Tests (vtcode-core)

```bash
cd vtcode-core
cargo test history_invariant_tests
```

- [ ] `test_ensure_call_outputs_present` passes
- [ ] `test_remove_orphan_outputs` passes
- [ ] `test_normalize` passes
- [ ] `test_validation_report_summary` passes
- [ ] `test_canceled_tool_call` passes
- [ ] `test_history_trimming_maintains_invariants` passes

### Integration Tests (manual)

**Test 1: Crash Recovery**
- [ ] Create a session
- [ ] Add a tool call without output
- [ ] Call `recover_from_crash()`
- [ ] Verify synthetic output created

**Test 2: History Trimming**
- [ ] Create history with 5 call/output pairs
- [ ] Trim to 2 items
- [ ] Call `normalize()`
- [ ] Verify pairs still valid

**Test 3: Dangling Call**
- [ ] Add tool call
- [ ] User cancels (no output)
- [ ] Call `normalize()`
- [ ] Verify "canceled" output created

### Performance Tests

**Test 4: No Regression**
```bash
# Before and after should be similar
time cargo test history_invariant_tests
```

- [ ] Tests run in < 1 second
- [ ] No noticeable slowdown

---

## Git Workflow

### 1. Create Feature Branch
```bash
git checkout -b feature/context-manager-invariants
git push -u origin feature/context-manager-invariants
```

### 2. Commit Strategy

Commit by day:

```bash
# Day 1-2: Types
git add vtcode-core/src/core/agent/state.rs
git commit -m "feat(context): add output status and validation types

- Add OutputStatus enum (Success, Failed, Canceled, Timeout)
- Add ToolCallId struct for call tracking
- Add PairableHistoryItem enum for validation
- Add HistoryValidationReport and MissingOutput structs

This is groundwork for call/output pairing invariants."

# Day 2-3: Methods
git commit -m "feat(context): implement validation and normalization

- Add ensure_call_outputs_present() to enforce calls have outputs
- Add remove_orphan_outputs() to remove outputs without calls
- Add validate_history_invariants() for checking state
- Add normalize() public wrapper
- Add helper methods for status conversion

Fixes: #<issue-number>"

# Day 3: Tests
git commit -m "test(context): add comprehensive invariant tests

- Test missing output creation (synthetic)
- Test orphan output removal
- Test normalization
- Test validation reporting
- Test crash recovery scenario
- Test history trimming maintains pairs

Coverage: 6 test cases"

# Day 4: Integration
git commit -m "refactor(state): integrate normalization into workflow

- Call normalize() after loading session
- Call normalize() after trimming history
- Call normalize() before prompt generation
- Add logging for invariant violations

Improves session reliability and crash recovery."

# Day 4: Config
git commit -m "config: add context invariant options

- Add [context] section to vtcode.toml
- Add enforce_history_invariants flag
- Add auto_recover_from_crash flag
- Add warn_on_invariant_violations flag

All options default to safe values."
```

### 3. Create Pull Request

**Title**: `feat: Add context manager invariants (call/output pairing)`

**Description**:
```markdown
## Summary
Implements OpenAI Codex's proven call/output pairing invariants to improve 
conversation history reliability and crash recovery.

## Changes
- Add output status tracking for tool executions
- Implement ensure_call_outputs_present() for synthetic output creation
- Implement remove_orphan_outputs() for cleanup
- Add validation and normalization workflow
- Comprehensive test coverage (6+ tests)

## Motivation
Prevents dangling tool calls in history when user cancels or VT Code crashes.
Ensures LLM always sees consistent, valid conversation context.

## Testing
- [ ] All new tests pass: `cargo test history_invariant_tests`
- [ ] No clippy warnings: `cargo clippy`
- [ ] Code formatted: `cargo fmt`
- [ ] Manual crash recovery test (see docs/CONTEXT_MANAGER_QUICKSTART.md)

## Related
- Implements patterns from: https://github.com/openai/codex
- Design doc: docs/CONTEXT_MANAGER_ANALYSIS.md
- Implementation guide: docs/CONTEXT_MANAGER_IMPLEMENTATION.md

## Checklist
- [x] Tests written and passing
- [x] Documentation updated
- [x] Code formatted and linted
- [ ] PR review approved
- [ ] Ready to merge

## Risk Assessment
**Risk Level**: LOW
- Additive changes only (no breaking changes)
- Opt-in feature via config
- Comprehensive test coverage
- Production pattern from OpenAI Codex
```

---

## Progress Tracking

### Week 1

| Day | Task | Status | Notes |
|-----|------|--------|-------|
| 1-2 | Add types | ⬜ TODO | Types + enums |
| 2-3 | Methods | ⬜ TODO | Core logic |
| 3 | Tests | ⬜ TODO | 6+ test cases |

### Week 2

| Day | Task | Status | Notes |
|-----|------|--------|-------|
| 4 | Integration | ⬜ TODO | Session load + trim |
| 4 | Config | ⬜ TODO | vtcode.toml |
| 5 | Verification | ⬜ TODO | clippy + fmt + test |

---

## Success Criteria (Final Verification)

- [ ] All tests pass: `cargo test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] Code formatted: `cargo fmt --check`
- [ ] Dangling calls get synthetic outputs
- [ ] Orphan outputs are removed
- [ ] Sessions restore without errors
- [ ] No performance regression
- [ ] Logs show what was recovered
- [ ] PR reviewed and approved

---

## Debugging Tips

### Test Fails?

1. Check the test output:
```bash
cargo test history_invariant_tests -- --nocapture
```

2. Run specific test:
```bash
cargo test test_ensure_call_outputs_present -- --nocapture
```

3. Add debug output:
```rust
dbg!(&state.history);
println!("{:#?}", state.validate_history_invariants());
```

### Code Doesn't Compile?

1. Check error message:
```bash
cargo check 2>&1 | head -20
```

2. Verify imports:
```rust
use crate::core::agent::state::{ToolCallId, OutputStatus};
use anyhow::Result;
```

3. Check method signatures match documentation

### Integration Broken?

1. Verify normalize() is called:
```bash
RUST_LOG=debug cargo test 2>&1 | grep "normalize"
```

2. Check logs for warnings:
```bash
RUST_LOG=warn cargo test 2>&1 | grep "History"
```

---

## References During Implementation

- **Code patterns**: docs/CONTEXT_MANAGER_IMPLEMENTATION.md
- **Quick reference**: docs/CONTEXT_MANAGER_QUICKSTART.md
- **Full analysis**: docs/CONTEXT_MANAGER_ANALYSIS.md
- **This tracker**: docs/PHASE1_IMPLEMENTATION_TRACKER.md

---

## Sign-off

- [ ] Implementer: _________________ Date: _______
- [ ] Code Reviewer: ______________ Date: _______
- [ ] Tech Lead: _________________ Date: _______

---

**Status**: Ready to start
**Owner**: (Assign)
**Start Date**: (TBD)
**Target Completion**: 2 weeks
