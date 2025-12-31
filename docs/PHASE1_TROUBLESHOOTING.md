# Phase 1 Troubleshooting & Debugging Guide

Quick solutions for common issues during implementation.

---

## Compilation Issues

### Issue: "cannot find type `ToolCallId` in this scope"

**Cause**: Type not defined yet

**Solution**: Ensure you added the type definition:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub String);
```

**Check**:
```bash
grep -n "pub struct ToolCallId" vtcode-core/src/core/agent/state.rs
```

---

### Issue: "method `ensure_call_outputs_present` not found"

**Cause**: Method not implemented on TaskRunState

**Solution**: Add the method to TaskRunState impl block:
```rust
impl TaskRunState {
    pub fn ensure_call_outputs_present(&mut self) { ... }
}
```

**Check**:
```bash
grep -n "fn ensure_call_outputs_present" vtcode-core/src/core/agent/state.rs
```

---

### Issue: "error: cannot borrow `self` as mutable more than once"

**Cause**: Multiple mutable borrows in a method

**Solution**: Collect data before mutating:
```rust
// ❌ Wrong: borrows self mutably while iterating
for item in self.history.iter() { // immutable
    self.history.push(...); // mutable - ERROR!
}

// ✅ Right: collect first, then mutate
let items_to_add: Vec<_> = self.history.iter().map(...).collect();
for item in items_to_add {
    self.history.push(item);
}
```

**Check**: Run `cargo check` and read error message carefully

---

## Recent Issues & Fixes (Dec 31, 2025)

### Issue: "failed to resolve: use of unresolved module or unlinked crate `wiremock`"

**Cause**: `wiremock` was used in `lmstudio` client tests but not added to `dev-dependencies`.

**Solution**: Add `wiremock` to `dev-dependencies` in `vtcode-core/Cargo.toml`:
```bash
cargo add --dev wiremock -p vtcode-core
```

---

### Issue: "`LMStudioClient` doesn't implement `std::fmt::Debug`"

**Cause**: `unwrap_err()` was called on a `Result<LMStudioClient, ...>` in tests, requiring `Debug` implementation for the success type.

**Solution**: Add `#[derive(Debug)]` to `LMStudioClient` struct in `vtcode-core/src/llm/providers/lmstudio/client.rs`.

---

## Test Failures

### Issue: Tests don't compile

**Debug Steps**:
1. Run the test in isolation:
```bash
cargo test test_ensure_call_outputs_present -- --nocapture
```

2. Check output:
```bash
cargo test 2>&1 | grep -A5 "error\|failed"
```

3. Verify the test module structure:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_name() {
        // test code
    }
}
```

---

### Issue: "assertion failed: report.is_valid()"

**Cause**: Validation report has errors when it shouldn't

**Solution**: Add debug output:
```rust
let report = state.validate_history_invariants();
println!("Missing outputs: {:?}", report.missing_outputs);
println!("Orphan outputs: {:?}", report.orphan_outputs);
assert!(report.is_valid(), "Report: {}", report.summary());
```

**Then run with output**:
```bash
cargo test test_name -- --nocapture
```

---

### Issue: "thread panicked at 'index out of bounds'"

**Cause**: Inserting/removing items with wrong indices

**Solution**: Verify insertion logic:
```rust
// When inserting in reverse order:
for (idx, item) in items.iter().rev() {
    self.history.insert(idx + 1, item); // +1 to insert AFTER
}
```

**Test with small example**:
```rust
let mut history = vec!["A", "B", "C"];
history.insert(0 + 1, "X"); // Insert after index 0
// Result: ["A", "X", "B", "C"] ✓
```

---

## Integration Issues

### Issue: `normalize()` called but no synthetic outputs created

**Debug Steps**:

1. Check if the method is actually called:
```bash
RUST_LOG=debug cargo test 2>&1 | grep -i normalize
```

2. Add tracing to method:
```rust
pub fn ensure_call_outputs_present(&mut self) {
    tracing::info!("ensure_call_outputs_present called");
    // ... rest of code
}
```

3. Verify method is on right type:
```bash
grep -B3 "fn ensure_call_outputs_present" vtcode-core/src/core/agent/state.rs
# Should show: impl TaskRunState
```

---

### Issue: Session loads but doesn't recover from crash

**Debug Steps**:

1. Check if `recover_from_crash()` is called:
```bash
grep -n "recover_from_crash" <session_loading_file>
```

2. Add logging to session load:
```rust
state.recover_from_crash().await?;
tracing::info!("Session recovered");
```

3. Run with logging enabled:
```bash
RUST_LOG=debug cargo run 2>&1 | grep -i "recover\|crash"
```

---

### Issue: History trimming crashes or loses data

**Debug Steps**:

1. Verify trim calls normalize:
```rust
pub fn trim_old_history(&mut self, keep_count: usize) {
    let to_remove = self.history.len().saturating_sub(keep_count);
    self.history.drain(0..to_remove);
    
    // Add this line:
    self.normalize();  // ← MUST be here
}
```

2. Test trimming in isolation:
```bash
cargo test test_trim_maintains_invariants -- --nocapture
```

3. If test fails, check for missing pairs:
```rust
let report = state.validate_history_invariants();
assert!(report.missing_outputs.is_empty(), "Missing: {:?}", report.missing_outputs);
assert!(report.orphan_outputs.is_empty(), "Orphan: {:?}", report.orphan_outputs);
```

---

## Clippy Warnings

### Issue: "warning: this function can be simplified"

**Common clippy warnings**:

```rust
// ❌ Clippy warning: use match instead of if let + else
if let Some(x) = value {
    do_thing(x);
} else {
    do_default();
}

// ✅ Better: use match
match value {
    Some(x) => do_thing(x),
    None => do_default(),
}
```

**Fix all warnings**:
```bash
cargo clippy --fix
cargo fmt
```

---

### Issue: "warning: this match has only one variant"

**Solution**: Simplify single-variant matches:
```rust
// ❌ Unnecessary match
match item {
    PairableHistoryItem::ToolCall { id, .. } => {
        // only case
    }
}

// ✅ Better: use if let
if let PairableHistoryItem::ToolCall { id, .. } = item {
    // handle case
}
```

---

## Performance Issues

### Issue: `normalize()` is slow

**Debug Steps**:

1. Measure current speed:
```bash
cargo test history_invariant_tests --release 2>&1 | grep "test result"
```

2. Profile with time:
```bash
time cargo test history_invariant_tests
```

3. Expected: < 1 second for all tests

**If slow**:
- Check if you're iterating history multiple times
- Consider collecting IDs instead of multiple searches
- Profile with: `cargo bench` (if benchmarks exist)

---

### Issue: Memory usage growing during normalization

**Cause**: Creating too many temporary collections

**Solution**: Minimize allocations:
```rust
// ❌ Creates 3 HashSets
let call_ids: HashSet<_> = ...collect();
let output_ids: HashSet<_> = ...collect();
let missing: Vec<_> = ...collect();

// ✅ More efficient: use single pass
let mut call_ids = HashSet::new();
let mut to_add = Vec::new();
for item in &self.history {
    if let Call { id } = item {
        call_ids.insert(id);
    }
}
// Single pass, fewer allocations
```

---

## Logic Errors

### Issue: Synthetic outputs created with wrong status

**Expected behavior**:
- Missing output → status: "canceled"
- Failed execution → status: "failed"
- Timeout → status: "timeout"

**Debug**:
```rust
let synthetic = self.create_synthetic_output(
    call_id,
    OutputStatus::Canceled,  // ← Must match scenario
    "Reason message"
);
```

**Check in test**:
```rust
let report = state.validate_history_invariants();
for missing in &report.missing_outputs {
    println!("Missing call: {}", missing.call_id.0);
}
```

---

### Issue: Orphan outputs not being removed

**Debug steps**:

1. Check the retention logic:
```rust
items.retain(|item| {
    if let ToolOutput { call_id, .. } = item {
        call_ids.contains(call_id)  // Must return true to keep
    } else {
        true  // Non-output items always kept
    }
});
```

2. Test the logic:
```rust
let mut call_ids = HashSet::new();
call_ids.insert("call-1");

let output = ToolOutput { call_id: "call-2", ... };

// Should return false (not in set):
let keep = call_ids.contains("call-2");
assert!(!keep);  // Item should be removed
```

---

## Configuration Issues

### Issue: Config flags not being read

**Debug Steps**:

1. Verify config structure in `vtcode.toml`:
```toml
[context]
enforce_history_invariants = false
auto_recover_from_crash = true
warn_on_invariant_violations = true
```

2. Check loading code:
```rust
pub fn from_vtcode_toml() -> Result<Self> {
    let config = load_config()?;
    Ok(Self {
        enforce_history_invariants: config
            .context
            .enforce_history_invariants
            .unwrap_or(false),
        // ... rest
    })
}
```

3. Test with:
```bash
cat vtcode.toml | grep -A3 "\[context\]"
```

---

### Issue: Features not enabled/disabled as expected

**Solution**: Check if config is actually used:

```bash
grep -n "context_config\|ContextConfig" vtcode-core/src/
# Should see multiple references
```

If not found, add:
```rust
let config = ContextConfig::from_vtcode_toml()?;
if config.warn_on_invariant_violations {
    tracing::warn!("Invariant violation found");
}
```

---

## Git & Versioning Issues

### Issue: "Your branch has diverged from origin"

**Solution**:
```bash
# Rebase on latest main
git fetch origin
git rebase origin/main
```

### Issue: Tests fail on CI but pass locally

**Common causes**:
1. Missing dependencies in CI environment
2. Different Rust version
3. Race conditions in tests

**Solution**:
```bash
# Clean and rebuild
cargo clean
cargo test

# Or check Rust version
rustc --version
cargo --version
```

---

## Debugging Techniques

### Technique 1: Add Debug Printing

```rust
let report = state.validate_history_invariants();
dbg!(&report);  // Prints with debug formatting

// Or custom:
println!("History has {} items", state.history.len());
for (i, item) in state.history.iter().enumerate() {
    println!("  [{}] {:?}", i, item);
}
```

**Run with output**:
```bash
cargo test -- --nocapture
```

---

### Technique 2: Use RUST_LOG

```bash
# See all debug logs
RUST_LOG=debug cargo test

# See only context manager logs
RUST_LOG=vtcode_core::core::agent::state=debug cargo test

# See warnings and above
RUST_LOG=warn cargo test
```

---

### Technique 3: Conditional Compilation

```rust
#[cfg(debug_assertions)]
fn debug_validate(state: &TaskRunState) {
    let report = state.validate_history_invariants();
    if !report.is_valid() {
        eprintln!("⚠️  History invalid: {}", report.summary());
    }
}

// Only enabled in debug builds
#[cfg(test)]
mod tests {
    // Test-only code
}
```

---

## Common Mistakes & Fixes

### Mistake 1: Forgetting `mut` keyword

```rust
// ❌ Can't modify
fn normalize(&self) { ... }

// ✅ Correct
fn normalize(&mut self) { ... }
```

---

### Mistake 2: Not handling Option/Result

```rust
// ❌ Panics on None
let id = missing_outputs[0].call_id;

// ✅ Safe
if let Some(output) = missing_outputs.first() {
    let id = &output.call_id;
}
```

---

### Mistake 3: Comparing references instead of values

```rust
// ❌ Wrong: comparing references
if item == &other_item {  // May not match

// ✅ Correct: compare values
if item == other_item {
    // or
}

if std::mem::discriminant(&item) == std::mem::discriminant(&other_item) {
    // If comparing enum variants
}
```

---

## Quick Reference

| Issue | Command |
|-------|---------|
| Won't compile | `cargo check 2>&1 \| head -20` |
| Tests fail | `cargo test -- --nocapture` |
| Has warnings | `cargo clippy` |
| Wrong format | `cargo fmt` |
| Performance slow | `cargo build --release` |
| Need debug logs | `RUST_LOG=debug cargo test` |

---

## Getting Help

1. **Error message unclear?** → Run full output without grep
2. **Test logic wrong?** → Add `dbg!()` and run with `--nocapture`
3. **Feature not working?** → Check config with `RUST_LOG=debug`
4. **Stuck?** → Review CONTEXT_MANAGER_IMPLEMENTATION.md patterns
5. **Still stuck?** → Post complete error output in PR

---

**Remember**: Most issues are simple - take a methodical approach:
1. Reproduce the issue
2. Add debug output
3. Verify assumptions
4. Fix based on findings
5. Run tests to confirm

---

**Last Updated**: December 31, 2025
