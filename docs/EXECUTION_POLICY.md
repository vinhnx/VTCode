# VT Code Execution Policy

This document describes what commands and operations the VT Code agent can execute without prompting, and which require confirmation.

## Summary

The execution policy is designed to allow engineers typical software development workflows while maintaining safety boundaries. Common development tools work automatically, while dangerous operations require confirmation.

## Auto-Allowed Commands

### Version Control (Git)
- **Read operations**: `status`, `log`, `show`, `diff`, `branch`, `tag`, `remote`
- **Tree inspection**: `ls-tree`, `ls-files`, `cat-file`, `rev-parse`, `describe`
- **Additional inspection**: `blame`, `grep`, `shortlog`, `format-patch`
- **Safe writes**: `add`, `commit`, `reset`, `checkout`, `switch`, `restore`, `merge`, `stash` (select ops)
- **Blocked**: `push --force`, `clean`, `rebase`, `cherry-pick`, `filter-branch`

### Build Tools (Cargo)
- **Safe operations**: `build`, `check`, `test`, `doc`, `clippy`, `fmt`, `run`, `bench`
- **Additional safe**: `tree`, `metadata`, `search`, `cache`, `expand`
- **Blocked**: `clean`, `install`, `uninstall`, `publish`, `yank`

### Languages
- **Python** (`python`, `python3`): Script execution, module runs with `-m`
- **Node.js** (`node`): Script execution
- **NPM** (`npm`): Install, test, build, run, start, list, view, search
- **Blocked for NPM**: `publish`, `unpublish`

### File Operations
- **Read**: `cat`, `head`, `tail`, `ls`, `grep`, `find`, `rg` (ripgrep)
- **Write**: `sed` (with workspace validation)
- **Copy**: `cp` (with workspace validation)
- **Count**: `wc`

### System Info
- `pwd`, `whoami`, `hostname`, `uname`, `date`, `echo`, `which`, `printenv`

## Tool Policies

| Tool | Policy | Notes |
|------|--------|-------|
| `list_files` | Allow | File discovery |
| `read_file` | Allow | File reading |
| `write_file` | Allow | New file creation |
| `edit_file` | Allow | Surgical edits |
| `create_file` | Allow | File creation |
| `grep_file` | Allow | Pattern search |
| `task_tracker` | Allow | TODO planning |
| `create_pty_session` | Allow | Terminal session |
| `read_pty_session` | Allow | Read terminal |
| `run_command` | **Prompt** | Terminal execution (safeguarded) |
| `send_pty_input` | **Prompt** | Send to terminal |
| `apply_patch` | **Prompt** | Complex diffs |
| `delete_file` | **Prompt** | File deletion |
| `web_fetch` | **Prompt** | External fetch |

## Key Safety Features

1. **Workspace Boundary**: All file operations are confined to `WORKSPACE_DIR`; cannot escape or touch system files
2. **Command Whitelisting**: Only specific commands are allowed; unknown commands are blocked
3. **Argument Validation**: Common flags are validated (e.g., git force-push is blocked)
4. **Confirmation Required**: Destructive operations (delete_file, apply_patch) still require user confirmation
5. **Two-Layer Control**: 
   - Tool-level: Which tools can be used
   - Command-level: Which specific commands and flags are permitted

## Dangerous Operations (Blocked)

- `rm -rf` (recursively remove)
- `sudo` (privilege escalation)
- `kubectl` (Kubernetes operations)
- `chmod`, `chown` (permission changes)
- Git force-push, clean, rebase, cherry-pick
- Cargo install, publish, clean
- NPM publish, unpublish
- File deletion (`delete_file` requires confirmation)
- Apply complex patches (`apply_patch` requires confirmation)

## Use Cases

### Typical Engineer Workflow
```
 git status, git diff, git log, git checkout
 cargo test, cargo build, cargo check
 npm install, npm test, npm run build
 python scripts/setup.py
 Editing files, reading logs, viewing diffs
```

### Blocked Without Confirmation
```
 Deleting files (requires confirmation)
 Applying complex patches (requires confirmation)
 Git force-push or history rewrites
 Publishing to registries
```

## Configuration

Policies are defined in:
- **Core defaults**: `vtcode-config/src/core/tools.rs` (tool policies)
- **Command validation**: `vtcode-core/src/execpolicy/mod.rs` (command whitelisting)
- **User overrides**: `vtcode.toml` in project root or `~/.vtcode/` directory

Override examples in `vtcode.toml`:
```toml
[tools.policies]
apply_patch = "allow"  # Allow patches without prompt
delete_file = "allow"  # Allow file deletion
run_command = "allow"  # Allow all commands (less safe)
```

Override command allow-list:
```toml
[commands]
allow_list = [
    "ls", "pwd", "date", "git", "cargo", "python"
]
allow_glob = [
    "git *", "cargo *", "python *"
]
```
