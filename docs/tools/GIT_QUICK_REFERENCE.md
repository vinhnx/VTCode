# Git Commands - Quick Reference

## ✅ Allowed Operations

### View Operations
```bash
git status                    # Check repository status
git log --oneline            # View commit history
git log -p                    # Show patch details
git diff                      # Show unstaged changes
git diff HEAD~1              # Compare with previous commit
git show HEAD                # Show latest commit details
git branch -a                # List all branches
git tag                       # List tags
git cat-file -p <hash>       # Inspect objects
```

### Staging
```bash
git add .                     # Stage all changes
git add src/file.rs          # Stage specific file
git add -A                    # Stage all (new, modified, deleted)
git add -u                    # Stage modifications and deletions
git add -p                    # Interactive staging
git add --dry-run            # Preview what would be staged
```

### Committing
```bash
git commit -m "message"       # Create commit with message
git commit -a                # Commit all tracked changes
git commit --amend           # Amend previous commit
git commit --no-verify       # Skip hooks
```

### Reversing
```bash
git reset --soft HEAD~1      # Undo commit, keep changes staged
git reset --mixed HEAD~1     # Undo commit, keep changes unstaged
git reset --unstage          # Unstage specific file
```

### Switching
```bash
git checkout main            # Switch to branch
git checkout src/file.rs     # Restore file from HEAD
git checkout -p              # Interactive restoration
```

### Stash
```bash
git stash list               # Show stashed changes
git stash show               # Show latest stash
git stash pop                # Apply and remove stash
git stash apply              # Apply without removing
git stash drop               # Delete stash
```

## ❌ Blocked Operations

### Force Operations (Unsafe)
```bash
git push --force             # ❌ Force push (not allowed)
git add --force              # ❌ Force add (not allowed)
git checkout --force         # ❌ Force checkout (not allowed)
```

### Destructive Reset
```bash
git reset --hard             # ❌ Discard all changes (not allowed)
git reset --merge            # ❌ Merge mode reset (not allowed)
git reset --keep             # ❌ Keep mode reset (not allowed)
```

### Cleanup
```bash
git clean                    # ❌ Remove untracked files (not allowed)
```

### History Rewriting
```bash
git rebase                   # ❌ Reapply commits (not allowed)
git filter-branch            # ❌ Rewrite history (not allowed)
git cherry-pick              # ❌ Apply individual commits (not allowed)
```

### Maintenance
```bash
git gc --aggressive          # ❌ Aggressive GC (not allowed)
```

## Common Workflows

### Creating a Commit
```bash
git add src/changes.rs
git commit -m "feat: add new feature"
git log --oneline            # Verify
```

### Undoing Last Commit (Keep Changes)
```bash
git reset --soft HEAD~1
git log --oneline            # Verify
```

### Checking What Changed
```bash
git status                   # Overview
git diff                     # Unstaged changes
git diff HEAD~1              # Last commit
git log --oneline -5         # Recent commits
```

### Working with Stash
```bash
git stash                    # Save current work
git checkout hotfix          # Switch branch
# ... do work ...
git checkout main            # Back to main
git stash pop                # Restore work
```

### Exploring History
```bash
git log --oneline --graph --all
git show <commit-hash>       # Inspect specific commit
git diff <commit1> <commit2> # Compare commits
```

## Error Messages

### "command 'git <op>' is not permitted"
The operation is blocked for safety. Check this quick reference or the full documentation.

### "path '<path>' is outside the workspace root"
Attempted to access a file outside the project directory. Use relative paths within your workspace.

### "git argument contains suspicious shell metacharacters"
Avoid special shell characters. Use plain arguments only.

## Notes

- All paths are validated against workspace root
- Shell injection is prevented
- Destructive operations require explicit design review
- Force flags are disabled to prevent accidents

For full details, see: `docs/tools/GIT_COMMAND_EXECUTION.md`
