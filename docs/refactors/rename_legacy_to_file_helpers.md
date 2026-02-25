# Refactor: Rename legacy.rs to file_helpers.rs

## Motivation

The file `vtcode-core/src/tools/registry/legacy.rs` had a misleading name that suggested it contained deprecated code to be removed. In reality, it contains:

1. **Active, essential code**: The `edit_file` tool implementation
2. **Convenience methods**: Wrappers for file operations (read, write, create, delete, grep, list)
3. **Recently fixed code**: Just fixed 3 critical bugs in `edit_file`

## Changes Made

### File Renames
-  `legacy.rs` → `file_helpers.rs`
-  `legacy_tests.rs` → `file_helpers_tests.rs`

### Module Updates
-  Updated `mod.rs` to import `file_helpers` instead of `legacy`
-  Added comprehensive module documentation explaining purpose

### Documentation Added
```rust
//! File operation helpers and the edit_file tool
//!
//! This module provides convenience methods for common file operations and implements
//! the `edit_file` tool, which is optimized for small, surgical edits (≤800 chars, ≤40 lines).
//! For larger or multi-file changes, use `apply_patch` instead.
```

## Why "file_helpers"?

The new name accurately reflects the module's purpose:
- **"file"**: All methods deal with file operations
- **"helpers"**: Provides convenience wrappers and utilities
- **Clear intent**: Immediately understandable, not misleading

## Verification

```bash
cargo check --package vtcode-core
#  Compiles successfully with 0 errors
```

## Impact

- **No breaking changes**: Internal module rename only
- **Better code clarity**: Name now matches purpose
- **Easier maintenance**: New contributors won't think it's deprecated
- **Consistent with recent work**: Aligns with the 3 bug fixes just completed

## Files Changed

1. `vtcode-core/src/tools/registry/file_helpers.rs` (renamed from legacy.rs)
2. `vtcode-core/src/tools/registry/file_helpers_tests.rs` (renamed from legacy_tests.rs)
3. `vtcode-core/src/tools/registry/mod.rs` (updated import)

## Related Work

This refactor complements the recent edit_file bug fixes:
- Bug #1: Newline handling
- Bug #2: Fuzzy matching
- Bug #3: Trailing newline preservation

