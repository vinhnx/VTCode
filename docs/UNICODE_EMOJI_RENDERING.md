# Unicode and Emoji Rendering in VTCode TUI

## Issue

Unicode characters (especially emoji like ✓ ) were appearing corrupted as "â" or other mojibake when displayed in git diff output or other TUI rendering.

## Root Cause

The corruption was caused by:
1. **Git pager interference**: The `delta` pager (if configured as git's pager) outputs ANSI-coded text with UTF-8 characters, but when captured via `Command::output()`, the pager can introduce encoding issues
2. **UTF-8 handling in ANSI parsing**: The `ansi-to-tui` crate's handling of multi-byte UTF-8 characters interspersed with ANSI escape sequences can sometimes cause character corruption

## Solution

### Primary Fix: Disable Git Pager for Programmatic Diff

In `src/agent/runloop/git.rs`, the git diff command now explicitly disables the pager by setting `GIT_PAGER=cat`:

```rust
let output = std::process::Command::new("git")
    .args(["diff", file])
    .env("GIT_PAGER", "cat")  // Bypass delta pager to avoid encoding issues
    .output()?;
```

This ensures that git outputs raw diff text without external pager processing, which can corrupt UTF-8 encoding.

### Secondary Fix: ANSI Text Handling

Added clarifying comments in `vtcode-core/src/utils/ansi.rs` about UTF-8 handling in the `convert_plain_lines` function. The `ansi-to-tui` crate should preserve UTF-8 strings (since Rust's `String` and `Cow<str>` types are always valid UTF-8), but explicit documentation of this assumption helps prevent future regressions.

## Best Practices

To avoid Unicode/emoji rendering issues in the TUI:

1. **Disable external pagers** for programmatic command execution (e.g., git, diff, etc.)
2. **Use `strip_ansi_codes` carefully** - the implementation in `streams.rs` properly handles UTF-8 character boundaries
3. **Ensure terminal LANG** is set to UTF-8 (typically `en_US.UTF-8` or similar)
4. **Test with emoji** when adding new output that should display special characters

## Files Modified

- `src/agent/runloop/git.rs` - Disabled pager for git diff commands
- `vtcode-core/src/utils/ansi.rs` - Added clarifying comments about UTF-8 handling

## Terminal Requirements

The terminal must be configured with a UTF-8 locale. On most modern systems, this is automatic:
- macOS: Uses UTF-8 by default
- Linux: Set `LANG=en_US.UTF-8` or equivalent
- Windows (with WSL): Set WSL terminal to UTF-8

You can verify your terminal's encoding with:
```bash
locale
echo "✓  UTF-8 works!"
```
