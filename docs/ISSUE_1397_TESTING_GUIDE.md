# Testing Guide: Issue #1397 Fix - Large Output Performance

## Quick Test

To quickly verify the fix works:

```bash
# 1. Build the project
cargo build --release

# 2. Run a command that produces large output
./target/release/vtcode exec "git log --all --oneline --graph"

# 3. Watch for the overflow warning at 50MB limit
```

## Comprehensive Test Suite

### Test 1: Byte Limit Enforcement

**Objective**: Verify PTY scrollback stops accepting data at 50MB

```bash
# Generate 100MB of output
dd if=/dev/zero bs=1M count=100 | base64

# Expected: Warning at 50MB, no hang
```

**Expected Output**:
```
[  Output size limit exceeded (50 MB). Further output truncated. Use 'spool to disk' for full output.]
```

### Test 2: Large Git Diff

**Objective**: Ensure `git diff` on large repos doesn't hang

```bash
# Create test repo with large changes
git init /tmp/test-large-diff
cd /tmp/test-large-diff

# Generate 1000 files with changes
for i in {1..1000}; do
  echo "Line $i" > file$i.txt
  git add file$i.txt
done
git commit -m "Initial commit"

# Modify all files
for i in {1..1000}; do
  echo "Modified line $i" >> file$i.txt
done

# Run diff (should not hang)
git diff
```

**Expected**: Output truncates at 50MB with warning, no hang

### Test 3: Huge Log Output

**Objective**: Test memory limits on git log

```bash
# In a repo with thousands of commits
git log --all --oneline --no-abbrev-commit

# Expected: Caps at 50MB, shows warning
```

### Test 4: Circular Buffer Behavior

**Objective**: Verify oldest lines are dropped when line limit exceeded

```bash
# Configure small line limit for testing
# In vtcode.toml:
# [pty]
# scrollback_lines = 100
# max_scrollback_bytes = 1000000

# Generate 200 lines
for i in {1..200}; do echo "Line $i"; done

# Expected: Only last 100 lines retained
```

### Test 5: Normal Operations Still Work

**Objective**: Ensure fix doesn't break normal command execution

```bash
# Test various common commands
cargo check
cargo test
git status
git diff src/main.rs
ls -la
cat README.md

# Expected: All work normally, no warnings
```

### Test 6: Unit Tests

**Objective**: Run newly added unit tests

```bash
# Run PTY-specific tests
cargo test --package vtcode-core pty::tests::scrollback

# Expected: All 6 tests pass
# - scrollback_enforces_byte_limit
# - scrollback_circular_buffer_drops_oldest
# - scrollback_tracks_bytes_correctly
# - scrollback_drops_oldest_when_line_limit_exceeded
# - scrollback_no_overflow_under_limit
# - scrollback_pending_operations
```

## Manual Integration Test

### Scenario: Large Git Diff in Real Project

```bash
# 1. Clone a large project
git clone https://github.com/rust-lang/rust.git /tmp/rust
cd /tmp/rust

# 2. Create large diff
git checkout HEAD~100
# Make some changes to trigger rebuild
touch dummy.txt
git add dummy.txt

# 3. Run vtcode with large diff command
vtcode exec "git diff HEAD~10"

# 4. Verify behavior:
#    - No hang/freeze
#    - Warning appears at 50MB
#    - Program remains responsive
#    - Can cancel with Ctrl+C
```

### Scenario: Continuous Large Output

```bash
# Stream continuous large output
vtcode exec "yes 'test line' | head -n 1000000"

# Expected:
# - Truncates at 50MB
# - Shows warning
# - Returns promptly
# - No memory spike
```

## Performance Verification

### Memory Monitor Test

```bash
# Terminal 1: Run vtcode with large output
vtcode exec "find / -type f 2>/dev/null"

# Terminal 2: Monitor memory usage
watch -n 1 'ps aux | grep vtcode | head -n 1'

# Expected:
# - Memory stabilizes at ~100-150MB
# - Does not grow indefinitely
# - Drops to normal after command completes
```

### Response Time Test

```bash
# Measure response time for large output
time vtcode exec "git log --all --oneline | head -n 100000"

# Before fix: 30+ seconds or hang
# After fix: <5 seconds with warning
```

## Configuration Testing

### Test Custom Limits

Edit `vtcode.toml`:

```toml
[pty]
scrollback_lines = 1000
max_scrollback_bytes = 10000000  # 10MB limit for testing
large_output_threshold_kb = 1000  # 1MB threshold
```

Run test:
```bash
# Should hit 10MB limit faster
dd if=/dev/zero bs=1M count=20 | base64
```

**Expected**: Warning at 10MB instead of 50MB

### Test Default Values

Remove PTY config from `vtcode.toml`:

```bash
# Defaults should apply
# - 50MB max_scrollback_bytes
# - 5000KB large_output_threshold_kb
```

## Regression Testing

Ensure fix doesn't break existing functionality:

```bash
# 1. Normal commands still work
cargo check
git status
ls -la

# 2. PTY sessions still work
vtcode  # Interactive mode
# Type some commands, verify normal operation

# 3. Error handling still works
vtcode exec "nonexistent_command"
# Expected: Proper error message, no hang

# 4. Command history still works
# Use up/down arrows in interactive mode
```

## Automated Test Script

```bash
#!/bin/bash
# test_large_output.sh

echo "Testing Issue #1397 Fix..."

# Test 1: Byte limit
echo "Test 1: Byte limit enforcement"
timeout 30 vtcode exec "dd if=/dev/zero bs=1M count=100 | base64" 2>&1 | grep -q "Output size limit exceeded"
if [ $? -eq 0 ]; then
    echo " Test 1 passed"
else
    echo " Test 1 failed"
    exit 1
fi

# Test 2: Normal operation
echo "Test 2: Normal operation"
timeout 10 vtcode exec "echo 'Hello World'" > /dev/null
if [ $? -eq 0 ]; then
    echo " Test 2 passed"
else
    echo " Test 2 failed"
    exit 1
fi

# Test 3: Unit tests
echo "Test 3: Unit tests"
cargo test --package vtcode-core pty::tests::scrollback --quiet
if [ $? -eq 0 ]; then
    echo " Test 3 passed"
else
    echo " Test 3 failed"
    exit 1
fi

echo "All tests passed! "
```

## Expected Results Summary

| Test | Before Fix | After Fix |
|------|-----------|-----------|
| Large git diff | Hangs indefinitely | Truncates at 50MB with warning |
| Memory usage | Grows to GB | Caps at ~150MB |
| Response time | 30s+ or hang | <5s |
| UI responsiveness | Frozen | Always responsive |
| Normal commands | Works | Works (unchanged) |
| Unit tests | N/A | 6/6 passing |

## Troubleshooting

### If tests fail:

1. **Check configuration**:
   ```bash
   grep -A 5 "^\[pty\]" vtcode.toml
   ```

2. **Verify build**:
   ```bash
   cargo clean
   cargo build --release
   ```

3. **Check defaults**:
   ```bash
   # Ensure config loading works
   vtcode --version
   ```

4. **Run with debug logging**:
   ```bash
   RUST_LOG=debug vtcode exec "your_command"
   ```

## Success Criteria

  All 6 unit tests pass  
  Large git diff completes without hang  
  Memory usage remains bounded  
  Warning message appears at 50MB  
  Normal commands work unchanged  
  Program remains responsive during large output  
  Can interrupt with Ctrl+C

---

**Status**: Ready for testing  
**Issue**: #1397  
**Related**: `docs/ISSUE_1397_FIX_SUMMARY.md`
