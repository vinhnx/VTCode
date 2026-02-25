# Command Defaults Migration Guide

## Overview

VT Code now ships with comprehensive **safe-by-default** command allowlists. This document explains what changed and how to migrate your configuration.

## What Changed

### Before

Minimal default allow_list - only basic commands:

```toml
[commands]
allow_list = [
  "ls", "pwd", "echo", "date", "whoami",
  "git", "cargo", "python", "npm"
]
```

### After

Comprehensive default allow_list - all safe development tools:

```toml
[commands]
allow_list = [
  # 100+ safe commands including build tools, language runtimes,
  # file utilities, version control, compression, and system info
]
```

## Benefits

**Non-powered users don't need configuration** - Works out of the box
**Agent access to full toolchain** - Can use `cargo test`, `npm run`, `pytest`, etc.
**Better PATH resolution** - Extended PATH with `.cargo/bin`, `.local/bin`, etc.
**Expanded glob patterns** - Patterns like `cargo *`, `npm run *` work
**Comprehensive deny rules** - More dangerous commands are blocked
**Backward compatible** - User configurations still override defaults

## Migration Steps

### Step 1: Understand Your Current Config

Check what you have in `vtcode.toml`:

```bash
grep -A 30 "\[commands\]" vtcode.toml
```

### Step 2: Decide on Your Strategy

#### Option A: Use New Defaults (Recommended)

Remove your `[commands]` section entirely and let VT Code use defaults:

```toml
# Remove these lines:
# [commands]
# allow_list = [...]
# deny_glob = [...]
# etc.
```

This gets you:

-   All 100+ safe commands enabled by default
-   Full glob patterns for development workflows
-   Automatic updates when new commands are added
-   Simpler configuration

#### Option B: Merge with Defaults

Keep your custom settings but add them on top of defaults:

```toml
[commands]
# Keep your custom allows
allow_list = [
  # ... your custom commands ...
  "my-custom-tool",
]

# Keep your custom denies
deny_glob = [
  "rm *",     # Block all rm
  "docker run *",  # Custom: block containers
]
```

The system merges your list with defaults intelligently.

#### Option C: Start Fresh with New Defaults

Copy the comprehensive defaults from `vtcode.toml.example`:

```bash
# Back up your current config
cp vtcode.toml vtcode.toml.backup

# Use new defaults as base
cp vtcode.toml.example vtcode.toml

# Then customize only what you need
vim vtcode.toml
```

### Step 3: Test Your Configuration

Validate that commands work as expected:

```bash
# Test a few safe commands
cargo --version     # Should work
git --version      # Should work
npm --version      # Should work

# Test that blocked commands are blocked
rm --version       # Should be blocked
sudo ls            # Should be blocked
```

### Step 4: Check PATH Resolution

Verify that the agent can find tools installed via package managers:

```bash
# Check if tools are in extended PATH
which cargo
which npm
which python3
```

If you have tools in non-standard locations, add them to `extra_path_entries`:

```toml
[commands]
extra_path_entries = [
  "$HOME/.custom-tools/bin",  # Add custom location
  "/usr/local/custom/bin",
]
```

### Step 5: Review Audit Logs

Monitor what commands are being executed:

```bash
ls ~/.vtcode/audit/
cat ~/.vtcode/audit/decisions.log
```

## Common Migration Scenarios

### Scenario 1: I have a minimal config

**Current:**

```toml
[commands]
allow_list = ["ls", "git", "cargo"]
```

**Migration:**
Remove the entire `[commands]` section. You'll get 100+ safe commands automatically.

**Result:** Better - now `git stash`, `cargo test`, `npm install` all work

---

### Scenario 2: I have project-specific rules

**Current:**

```toml
[commands]
allow_list = ["ls", "git", "cargo", "make", "my-build-script"]
deny_glob = ["rm *", "sudo *"]
```

**Migration:**
Keep your custom additions, remove the default duplicates:

```toml
[commands]
allow_list = [
  "my-build-script",      # Only custom additions
]
deny_glob = [
  # Remove defaults; they're now built-in
  # Just add custom denies if needed
]
```

**Result:** Better - custom rules + all 100+ safe commands

---

### Scenario 3: I have very restrictive rules (high security)

**Current:**

```toml
[commands]
allow_list = ["ls", "pwd", "git"]
allow_glob = []
deny_glob = ["*"]  # Block everything
```

**Migration:**
Explicitly set a minimal allow_list and keep restrictions:

```toml
[commands]
allow_list = [
  "ls", "pwd", "git",
  # Add minimal safe set
]
deny_glob = ["*"]  # Still blocks everything by default
allow_glob = []    # No patterns
```

**Result:** Same - your restrictions are preserved

---

### Scenario 4: I have permissive rules (for trusted environments)

**Current:**

```toml
[commands]
allow_glob = [
  "git *",
  "cargo *",
  "npm *",
]
deny_glob = ["rm *", "sudo *"]
```

**Migration:**
Already compatible! Just ensure deny rules are updated:

```toml
[commands]
# Keep your allow patterns
allow_glob = [
  "git *",
  "cargo *",
  "npm *",
]
# Add any additional deny rules
deny_glob = [
  "rm *",
  "sudo *",
  "docker run *",  # Add more if needed
]
```

**Result:** Compatible - works as-is with new defaults

## Before/After Examples

### Example 1: Python Development

**Before (minimal config):**

```toml
[commands]
allow_list = ["ls", "pwd", "python", "pip"]
```

-   Only 4 commands work
-   No `pytest`, `black`, `flake8`, `mypy`
-   No version control
-   No file operations

**After (new defaults):**

```toml
# No config needed - or minimal additions:
[commands]
allow_list = [
  # 100+ safe commands automatically
  "black",
  "flake8",
  "pytest",
  # ... plus all standard tools
]
```

-   Full Python toolchain works
-   Git, make, node.js all available
-   Can use `grep`, `find`, `tar`, etc.
-   Agent can run full workflows

---

### Example 2: Full-Stack Development

**Before (custom):**

```toml
[commands]
allow_list = [
  "git", "cargo", "npm", "python",
  "docker", "docker-compose",
]
deny_glob = ["rm *", "sudo *"]
```

**After (new defaults):**

```toml
[commands]
# Everything is enabled by default
# Only customize if needed:
deny_glob = [
  "rm *",           # Still blocked
  "sudo *",         # Still blocked
  "docker run *",   # Add custom restriction
]
```

Result:

-   Git workflows work fully (`git stash`, `git tag`, etc.)
-   Cargo full toolchain (`cargo test`, `cargo doc`)
-   Node.js packages (`yarn`, `pnpm`, `bun`)
-   Docker builds work (`docker build`, `docker logs`)
-   But container execution is blocked (`docker run`)

## Configuration Precedence

Settings are merged in this order:

1. **Code defaults** (in `vtcode-config/src/core/commands.rs`)
2. **User's `vtcode.toml`** (overrides defaults)
3. **Project-level overrides** (if using different toml)

Your `vtcode.toml` always takes precedence.

## Rollback Plan

If you need to revert to a minimal config:

```bash
# Restore your backup
cp vtcode.toml.backup vtcode.toml

# Or manually specify minimal set
[commands]
allow_list = ["ls", "pwd", "echo"]
deny_glob = ["*"]  # Block everything else
```

## Testing After Migration

Run these tests to ensure your configuration works:

```bash
# Test safe commands work
./run.sh
# Try: cargo --version, git --version, npm --version

# Test blocked commands fail gracefully
# Try: rm --version, sudo ls, shutdown
# These should be blocked without crashing

# Check audit logs
ls -la ~/.vtcode/audit/

# Validate config syntax
cargo check
```

## Troubleshooting

### "Command not found" after migration

**Cause:** PATH not extended properly

**Fix:**

```toml
[commands]
extra_path_entries = [
  "$HOME/.cargo/bin",
  "$HOME/.local/bin",
  "/opt/homebrew/bin",
]
```

---

### "Command blocked unexpectedly"

**Cause:** Deny rule takes precedence

**Fix:**

1. Check `deny_list` and `deny_glob` in `vtcode.toml`
2. Remove if overly restrictive
3. Add to `allow_list` if truly safe

---

### "Agent can't find my tool"

**Cause:** Tool not in extended PATH

**Fix:**

```bash
# Find where tool is installed
which my-tool

# Add that directory to PATH
[commands]
extra_path_entries = [
  "/path/to/my-tool/bin",
]
```

## See Also

-   [COMMAND_SECURITY_MODEL.md](./COMMAND_SECURITY_MODEL.md) - Full command security details
-   [vtcode.toml.example](../vtcode.toml.example) - Example configuration with all safe defaults
-   [AGENTS.md](../AGENTS.md) - Agent guidelines
