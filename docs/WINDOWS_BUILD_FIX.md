# Fix: Windows Build Errors in vtcode-core

## Problem
The Windows build (x86_64-pc-windows-msvc) in the build-release.yml workflow was failing with 6 compilation errors:

1. **Format string error** - Missing format specifier
2. **Unused import** - SafetyDecision not used
3. **Missing Hash trait** - CmdletSeverity enum
4. **Unused variable** - dangerous array
5. **Unused variable** - key parameter
6. **Conditional usage** - path parameter only used on Unix

## Solutions Applied

### 1. windows_registry_filter.rs:375
**Error:** `format!` argument never used
```rust
// Before
let pattern = format!("run|runonce|services", info.path_pattern.to_lowercase());

// After
let pattern = format!("run|runonce|services|{}", info.path_pattern.to_lowercase());
```
The format string now includes the `info.path_pattern` argument.

### 2. windows_enhanced.rs:17
**Error:** Unused import
```rust
// Before
use crate::command_safety::SafetyDecision;

// After
// Removed - not used in the module
```

### 3. windows_cmdlet_db.rs:16
**Error:** CmdletSeverity doesn't implement Hash trait
```rust
// Before
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]

// After
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
```
Added `Hash` to enable use as HashMap keys (line 521).

### 4. windows_enhanced.rs:193
**Error:** Unused variable dangerous array
```rust
// Before
let dangerous = [
    "copy-item.*-destination.*powershell",
    "get-content.*-encoding.*utf8.*|.*iex",
    // ...
];

// After
// Dangerous patterns documented in comments:
// - copy-item.*-destination.*powershell
// - get-content.*-encoding.*utf8.*|.*iex
// ...
```
Converted to code comments since actual matching uses different logic.

### 5. windows_registry_filter.rs:327
**Error:** Unused variable key
```rust
// Before
for (key, info) in get_dangerous_registry_paths().iter() {

// After
for (_key, info) in get_dangerous_registry_paths().iter() {
```

### 6. skills/validation.rs:641
**Error:** path parameter unused on Windows (only used in #[cfg(unix)])
```rust
// Before
async fn check_executable_permissions(&self, path: &Path) -> CheckResult {

// After
#[allow(unused_variables)]
async fn check_executable_permissions(&self, path: &Path) -> CheckResult {
```
Used `#[allow(unused_variables)]` since the parameter is conditionally used.

## Build Status
✅ Local macOS build: Passes
✅ Release build: Completes successfully
✅ Tests: All passing

## Files Modified
- `vtcode-core/src/command_safety/windows_registry_filter.rs`
- `vtcode-core/src/command_safety/windows_enhanced.rs`
- `vtcode-core/src/command_safety/windows_cmdlet_db.rs`
- `vtcode-core/src/skills/validation.rs`

## Related Issues
- Windows build-release.yml workflow was failing on x86_64-pc-windows-msvc target
- All errors were compilation warnings treated as errors due to `-D warnings` flag
