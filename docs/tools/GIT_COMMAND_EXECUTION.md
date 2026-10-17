# Git Command Execution Policy

## Overview

The vtcode agent now supports comprehensive git command execution with a three-tier security model:

- **Tier 1**: Safe read-only operations (always allowed)
- **Tier 2**: Safe write operations (with validation)
- **Tier 3**: Dangerous operations (always blocked)

## Supported Operations

### Tier 1: Read-Only Operations (Always Allowed)

#### Status & History

- `git status` - Show working tree status
- `git log` - Show commit history
- `git show` - Show objects
- `git diff` - Show changes between commits
- `git branch` - List branches (bare/flag-only forms; `--set-upstream-to`, `--unset-upstream`, `--edit-description`, and
  rename/copy mutators are write operations)
- `git tag` - List tags (bare/flag-only forms; creating, deleting, or forcing a tag is a write operation)
- `git remote` - List remotes (`add`/`remove`/`set-url` are write operations)

#### Object Inspection

- `git ls-tree` - List tree object contents
- `git ls-files` - List indexed files
- `git cat-file` - Inspect object contents
- `git rev-parse` - Translate revision names
- `git describe` - Describe commits

#### Configuration

- `git config` - Get configuration values (read-only)

#### Stash Operations

- `git stash list` - List stashed changes (read-only)
- `git stash show` - Show stash contents (read-only)
- `git stash pop` - Apply and remove stash (write operation, Tier 2)
- `git stash apply` - Apply stash without removing (write operation, Tier 2)
- `git stash drop` - Delete stash (write operation, Tier 2)
- `git stash clear` - Delete all stashes (write operation, Tier 3)
- `git stash create` - Create new stash (write operation, Tier 2)

Bare `git stash` is `git stash push` (a write operation), and flag-only push forms (`-k`, `-u`, `-a`, `-p`, `-m`) carry
no operand, so only the explicit `list`/`show` forms are read-only.

### Tier 2: Safe Write Operations (With Validation)

#### Adding Files

- `git add` - Stage changes for commit
  - Supports: `-u`, `--update`, `-A`, `--all`, `.`, `-p`, `--patch`, `-i`, `--interactive`, `-n`, `--dry-run`
  - Blocked: `--force`, `-f` (unsafe bypass)

#### Creating Commits

- `git commit` - Create new commit
  - Supports: `-m`, `--message`, `-F`, `--file`, `-a`, `--all`, `-p`, `--patch`, `--amend`, `--no-verify`, `-q`,
    `--quiet`

#### Resetting Changes

- `git reset` - Reset HEAD to specified state
  - Safe modes only: `--soft`, `--mixed`, `--unstage`
  - Blocked: `--hard`, `--merge`, `--keep` (destructive)

#### Checking Out

- `git checkout` - Switch branches or restore files
  - Supports: `-p`, `--patch`, file/branch specifications
  - Blocked: `--force`, `-f` (destructive)

### Tier 3: Blocked Operations (Security)

The following operations are **always blocked**:

#### Dangerous History Manipulation

- `git filter-branch` - Rewrite repository history
- `git rebase` - Reapply commits (complex history changes)
- `git cherry-pick` - Apply individual commits (risky without oversight)

#### Destructive Operations

- `git clean` - Remove untracked files (use explicit rm instead)
- `git reset --hard` - Force discard changes
- `git reset --merge` - Discard changes and merge
- `git reset --keep` - Discard changes with safety checks
- `git gc --aggressive` - Aggressive garbage collection

#### Unsafe Push

- `git push --force` or `git push -f` - Force overwrite remote

## Usage Examples

### Safe Read Operations

```bash
# Check repository status
git status

# View commit history
git log --oneline --graph --all

# Show differences
git diff HEAD~1

# List branches
git branch -a

# Examine a specific commit
git show HEAD
```

### Safe Write Operations

```bash
# Stage changes
git add .
git add src/module.rs

# Create commit
git commit -m "feat: add new feature"
git commit -a --amend

# Soft reset (safe)
git reset --soft HEAD~1

# Checkout branch
git checkout main
git checkout src/file.rs
```

### Blocked Operations (Will Error)

```bash
#   Force push (not allowed)
git push --force

#   Destructive reset (not allowed)
git reset --hard

#   Complex history rewrite (not allowed)
git rebase main
git filter-branch
```

## Flags and Parameters

### Allowed Flags by Operation

#### git log / git show

- `-n` / `--oneline` / `--graph` / `--decorate` / `--all`
- `--grep` / `-S` (pattern matching)
- `-p` / `-U` / `--stat` / `--shortstat` / `--name-status` / `--name-only`
- `--author` / `--since` / `--until` / `--date`

#### git diff

- `-p` / `-U` / `--stat` / `--shortstat` / `--name-status` / `--name-only`
- `--no-index` / `-w` / `-b` (whitespace handling)

#### git branch

- `-a` (all branches) / `-r` (remote) / `-v` (verbose)

## Security Model

### Path Validation

- All file paths are validated against the workspace root
- Path traversal attempts are blocked
- Symlink escapes are detected and prevented

### Shell Injection Prevention

- Destructive redirection and dynamic-expansion syntax are rejected; safe pipelines and `;`/`&&` chains of independently
  read-only commands are allowed (plan mode admits them)
- Only safe subcommands and flags are accepted

### Destructive Operation Prevention

- Force flags (`-f`, `--force`) are blocked except where essential
- Hard reset modes are rejected
- History-rewriting operations require explicit confirmation

## Configuration

Plan-mode read-only admission is classified in `crates/codegen/vtcode-core/src/tools/tool_intent/readonly.rs`; the exec
policy validator lives in `crates/codegen/vtcode-core/src/exec_policy/`

To modify allowed operations:

1. Edit `validate_git()` function
2. Update respective validators (e.g., `validate_git_reset()`)
3. Add tests to `#[cfg(test)] mod tests`
4. Run `cargo test --lib execpolicy`

## Testing

Comprehensive test suite included:

```bash
cargo test --lib execpolicy::tests
```

Tests cover:

- All safe read-only operations
- Safe write operations with flags
- Rejection of dangerous operations
- Path validation
- Shell injection prevention

## Integration with Agent

The agent uses git through the PTY (pseudo-terminal) interface:

```rust
// Agent can run:
let result = agent.execute_command(&["git", "status"]).await;
let result = agent.execute_command(&["git", "log", "--oneline"]).await;
let result = agent.execute_command(&["git", "add", "src/"]).await;
let result = agent.execute_command(&["git", "commit", "-m", "message"]).await;
```

All commands pass through execution policy validation before execution.

## Migration from Previous State

Previously, `git diff` was explicitly blocked. This policy now:

1. **Allows** `git diff` as a safe read-only operation
2. **Allows** most safe git operations
3. **Blocks** only genuinely dangerous operations

Existing code using `git diff` will now work without modification.

## Future Enhancements

Potential additions for future releases:

- `git merge` (with validation)
- `git tag` (create/delete)
- `git stash push` (with safety checks)
- `git revert` (explicit commit reversal)
- Partial add support (`git add -p`)

These would require additional validation layers and are intentionally blocked for now.
