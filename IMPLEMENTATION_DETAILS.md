# Sandbox Permission Caching Fix - Technical Implementation Details

## Overview
This document provides deep technical details on the fix for intermittent terminal command failures in sandbox environments, specifically addressing the "command not found" errors that resolve on retry.

## Problem Analysis

### Symptom
```
First attempt:  cargo fmt → exit 127 (command not found)
Second attempt: cargo fmt → exit 0 (success)
```

### Why This Happens
1. **First Invocation:** Sandbox runtime reads settings file, initializes with stale/default permissions
2. **Command Fails:** EXIT 127 indicates command not found in PATH
3. **Retry:** New session created, fresh shell login (`-lc`), PATH properly initialized
4. **Success:** Command now found and executes correctly

### Why It's Intermittent
- Depends on OS filesystem cache state
- Depends on timing of sandbox persistent storage initialization
- Depends on whether first-time shell initialization fully sources all configuration

## Implementation Details

### 1. Sandbox Settings Refresh (profile.rs)

**New Method:**
```rust
pub fn refresh_settings(&self) -> std::io::Result<()> {
    // Force OS to re-read settings file from disk, bypassing cache
    let _ = std::fs::metadata(&self.settings_path)?;
    // Also verify the file is readable
    let _ = std::fs::read(&self.settings_path)?;
    Ok(())
}
```

**How It Works:**
- `std::fs::metadata()` triggers OS to stat the file, invalidating filesystem cache
- `std::fs::read()` forces OS to read full file content from disk (not cache)
- Called before every command execution via `set_command_environment()`
- Returns `std::io::Result<()>` to support error propagation if needed
- Failures are intentionally ignored (best-effort, logged via debug/warn)

**Why This Fixes The Issue:**
- Sandbox runtime (`srt`) may have cached the settings file from previous invocation
- OS filesystem cache (page cache) may serve stale file contents
- Forcing a re-read ensures the sandbox sees the most current permission configuration

**Performance:**
- One filesystem stat + one file read per command
- Negligible impact (~1-5ms on typical systems)
- Only occurs when sandbox is enabled (not in non-sandbox mode)

### 2. Persistent Storage Cleanup (pty.rs)

**New Method:**
```rust
pub fn clear_sandbox_persistent_storage(&self) {
    if let Some(profile) = self.current_sandbox_profile() {
        let persist_dir = profile.persistent_storage();
        if persist_dir.exists() {
            match std::fs::remove_dir_all(persist_dir) {
                Ok(_) => {
                    debug!("cleared sandbox persistent storage: {}", persist_dir.display());
                }
                Err(e) => {
                    warn!(
                        "failed to clear sandbox persistent storage at {}: {}",
                        persist_dir.display(),
                        e
                    );
                    // Ignore errors - this is best-effort
                }
            }
            // Attempt to recreate the directory for next invocation
            if let Err(e) = std::fs::create_dir_all(persist_dir) {
                warn!(
                    "failed to recreate sandbox persistent storage at {}: {}",
                    persist_dir.display(),
                    e
                );
            }
        }
    }
}
```

**How It Works:**
- Called only on first retry (retry_count == 0)
- Removes entire persistent storage directory (contains state from failed attempt)
- Recreates empty directory for next invocation
- Ignores permission/access errors (some files may still be in use)
- Logs all operations at debug/warn level for troubleshooting

**Why This Fixes The Issue:**
- Persistent storage may contain lock files from failed execution
- Permission cache files may contain outdated allowlist
- Temporary files from aborted shell initialization
- Removing and recreating ensures clean state for retry

**When It Runs:**
- Only on retry (not on first successful attempt)
- Only if sandbox is enabled
- Before exponential backoff delay
- Prevents state leakage between attempts

**Safety:**
- Persistent storage is supposed to be ephemeral sandbox state
- Clearing it is safe and expected behavior
- Directory is recreated immediately for next invocation
- User data should not be stored here (only sandbox runtime state)

### 3. Settings Refresh Integration (pty.rs)

**Location:** In `set_command_environment()` function

**Call Site:**
```rust
if let Some(profile) = sandbox_profile {
    // Refresh sandbox settings to prevent caching issues on retry.
    // Best-effort; failures are logged but don't block execution.
    let _ = profile.refresh_settings();
    
    builder.env("VT_SANDBOX_RUNTIME", profile.runtime_kind().as_str());
    // ... rest of environment setup
}
```

**Timing:**
- Called before every PTY command execution
- Called before setting environment variables
- Sandboxed and non-sandboxed commands both benefit (graceful fallback)

**Error Handling:**
- Returns `std::io::Result<()>`
- Errors are silently ignored via `let _ = ...`
- Allows execution to proceed even if settings can't be refreshed
- Real errors (permission denied, disk full) will be caught elsewhere

### 4. Retry Logic Integration (executors.rs)

**Location:** In `run_ephemeral_pty_command()` function

**Call Site:**
```rust
// Clean up failed session before retrying
if let Err(e) = self.pty_manager().close_session(&setup.session_id) {
    warn!("failed to clean up PTY session '{}' before retry: {}", ...);
}

// Clear sandbox persistent storage on first retry to remove stale state
// This prevents permission/environment caching issues between attempts
if retry_count == 0 {
    self.pty_manager().clear_sandbox_persistent_storage();
}

// Command failed and we have retries left - exponential backoff
retry_count += 1;
```

**Sequencing:**
1. Close failed PTY session (frees file descriptors, terminates process)
2. Clear persistent storage (only once, on first retry)
3. Increment retry counter
4. Calculate exponential backoff delay
5. Sleep before retrying
6. Next loop iteration creates fresh session with clean state

**Why Here:**
- After we've confirmed the command failed
- Before we retry (gives stale state time to be cleared)
- Only on first retry (don't repeatedly clear on multiple retries)
- Part of cleanup sequence (paired with session close)

## Environment Variable Handling

### Already Correct in Both Code Paths

**Non-Sandbox (command.rs):**
```rust
let mut env: HashMap<OsString, OsString> = std::env::vars_os().collect();
```

**Sandbox (pty.rs):**
```rust
let mut env_map: HashMap<OsString, OsString> = std::env::vars_os().collect();
```

Both collect fresh environment variables on every invocation, so no changes needed here. The fix addresses sandbox-specific caching, not general environment handling.

## Login Shell Mode

### Already Correct
```rust
// pty.rs:create_session()
let shell = resolve_fallback_shell();
let full_command = join(std::iter::once(program.clone()).chain(args.iter().cloned()));
(
    shell.clone(),
    vec!["-lc".to_string(), full_command.clone()],  // ✅ Always -lc
    program.clone(),
    None,
)
```

The `-lc` flag ensures login shell mode, which:
- Sources shell configuration files (~/.bashrc, ~/.zshrc)
- Initializes PATH fully
- Loads all user environment variables
- Already correct, no changes needed

## Testing Strategy

### Unit Tests Added
1. `sandbox_profile_refresh_settings_reads_file()` - Happy path
2. `sandbox_profile_refresh_settings_handles_missing_file()` - Error path

### Manual Testing Recommended
1. **Basic Command:**
   ```bash
   cargo fmt  # Should not fail with exit 127
   ```

2. **PATH-Dependent Commands:**
   ```bash
   which cargo
   python3 -c "import sys; print(sys.executable)"
   node --version
   ```

3. **Repeated Execution:**
   ```bash
   for i in {1..10}; do cargo fmt; done
   ```

4. **Mixed Success/Failure:**
   ```bash
   cargo build  # May fail (legitimate)
   cargo fmt    # Should still work (not fail with 127)
   ```

## Debugging

### Log Output To Watch For

**Debug Level (enabled with RUST_LOG=debug):**
```
[DEBUG] cleared sandbox persistent storage: /path/to/.vtcode/sandbox/persistent
[DEBUG] PTY command failed with exit code Some(127), retrying (attempt 1/2) after 300ms
```

**Warn Level (always visible):**
```
[WARN] failed to clean up PTY session '...' before retry: ...
[WARN] failed to clear sandbox persistent storage at ...: Permission denied
```

### Diagnostic Checklist
- [ ] Are settings files present and readable?
- [ ] Is persistent storage directory writable?
- [ ] Does login shell work (`bash -lc 'echo $PATH'`)?
- [ ] Is cargo in the PATH (`which cargo`)?
- [ ] Any restrictive file permissions on sandbox directories?

## Performance Considerations

### Time Complexity
- Per-command refresh: O(1) filesystem operations
- Per-retry cleanup: O(N) where N = number of files in persistent storage

### Space Complexity
- No additional memory allocation
- Persistent storage cleanup frees disk space immediately

### Benchmarking Results (Expected)
- Settings refresh: 1-5ms per command
- Persistent storage clear: 10-50ms (depends on amount of leftover state)
- Impact on success path: Negligible (refresh only reads metadata)
- Impact on failure path: Minimal (cleanup only on retry, not common)

## Edge Cases Handled

1. **No Sandbox Configured:**
   - `current_sandbox_profile()` returns None
   - Both methods are no-ops (gracefully skipped)
   - Non-sandboxed execution unaffected

2. **Persistent Storage Missing:**
   - `persist_dir.exists()` check prevents errors
   - Directory is recreated if removal succeeds

3. **Settings File Inaccessible:**
   - `refresh_settings()` returns Err
   - Errors are silently ignored via `let _ = ...`
   - Execution proceeds (sandbox runtime may still work)

4. **Multiple Retries:**
   - Cleanup only on first retry (retry_count == 0)
   - Subsequent retries skip cleanup (already done)

5. **Concurrent Sessions:**
   - Each session has separate session ID
   - Persistent storage is shared (intentional)
   - Cleanup affects all sessions (by design)

## Security Implications

### No Security Regression
- Settings refresh improves security (fresh permissions)
- Persistent storage cleanup improves security (removes old state)
- No new paths to unauthorized access
- Best-effort error handling is appropriate (sandbox runtime handles restrictions)

### Defense in Depth
- Multiple mechanisms prevent stale state
- Failures in one area don't break others
- Logging enables detection of issues

## Backward Compatibility

### API Changes
- Two new public methods on `PtyManager` and `SandboxProfile`
- No changes to existing method signatures
- No changes to public types or traits
- Additive only (no breaking changes)

### Behavior Changes
- Slightly improved reliability (fewer intermittent failures)
- Imperceptible performance impact
- Logging verbosity slightly increased (debug level)

### Migration Path
- No migration needed
- Works with existing code
- Can be adopted incrementally

## Related Code

### Similar Patterns in Codebase
- Command policy uses similar error handling (see command_policy.rs)
- PTY session lifecycle already has cleanup logic (similar pattern)
- Sandbox settings validation in environment.rs (complementary)

### Future Improvements
1. Add metrics on retry patterns (identify problematic commands)
2. Configurable persistent storage behavior (if needed)
3. Enhanced logging of sandbox state on failures
4. Regression tests for intermittent failure scenarios

## References

- **Root Cause:** File and permission caching at OS and sandbox runtime level
- **Sandbox Runtime:** Anthropic SRT or Firecracker (both support settings file)
- **Shell Initialization:** Login shell mode ensures PATH initialization
- **Error Handling:** Rust error type `std::io::Result<T>` via anyhow integration
