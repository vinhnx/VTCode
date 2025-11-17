# VTCode Command Security - Quick Reference

## What Works Out of the Box

All these commands are **enabled by default** for non-powered users:

### File & Text Operations
```
ls, pwd, cat, head, tail, grep, find, wc, sort, uniq, cut, awk, sed
echo, printf, date, which, file, stat, tree, diff, patch
```

### Version Control
```
git (all subcommands)
hg, svn, git-lfs
```

### Build & Compilation
```
cargo, cargo build, cargo test, cargo run, cargo clippy, cargo check, cargo fmt
rustc, rustfmt, rustup
make, cmake, ninja, meson, bazel
```

### Languages & Runtimes
```
python, python3, pip, pip3, virtualenv, pytest, black, flake8, mypy, ruff
node, npm, yarn, pnpm, bun, npx
go, gofmt, golint
java, javac, mvn, gradle
gcc, g++, clang, clang++
```

### Compression
```
tar, zip, unzip, gzip, gunzip, bzip2, bunzip2, xz, unxz
```

### System Information
```
ps, top, htop, df, du, whoami, hostname, uname
```

### Containers
```
docker, docker-compose (with restrictions on docker run)
```

## What's ALWAYS Blocked

These commands **cannot be run** regardless of configuration:

### Dangerous Patterns
- `rm -rf /`, `rm -rf ~`, `rm -rf /*` - Filesystem destruction
- `shutdown`, `reboot`, `halt`, `poweroff` - System shutdown
- `sudo *` - Privilege escalation
- `chmod *`, `chown *` - Permission/ownership changes
- `kill *`, `pkill *` - Process termination
- `systemctl *` - Service management
- `mount *`, `umount *` - Filesystem mounting
- `mkfs *`, `fdisk *`, `dd *` - Disk operations
- `eval` - Code evaluation
- `:(){ :|:& };:` - Fork bomb
- `kubectl *` - Kubernetes operations

## Wildcard Patterns (Enabled by Default)

These patterns enable entire command families:

```
git *              → git status, git pull, git commit, git reset, etc.
cargo *            → cargo build, cargo test, cargo run, cargo fmt, etc.
npm run *          → npm run build, npm run test, npm run dev, etc.
python *           → python -m pytest, python -c "...", etc.
go *               → go build, go test, go run, etc.
docker *           → docker ps, docker logs, docker build, etc. (except docker run)
```

## How It Works

1. **Safe by default** - All development tools work without asking
2. **Blocked first** - Dangerous commands are blocked before allowed ones are checked
3. **Patterns work** - Wildcard patterns like `git *` enable entire command families
4. **Logged** - All decisions are logged to `~/.vtcode/audit/`

## Customizing (If Needed)

Edit `vtcode.toml` in your project:

### Allow a new command
```toml
[commands]
allow_list = [
  "ls",
  "my-custom-tool",  # Add here
]
```

### Allow a command pattern
```toml
allow_glob = [
  "my-tool *",  # Enables: my-tool build, my-tool test, etc.
]
```

### Block something
```toml
deny_glob = [
  "docker run *",  # Blocks: docker run anything
]
```

## Common Scenarios

### I need to run `docker build`
✅ Works - It's in `docker *` pattern

### I need to run `docker run`
❌ Blocked - Requires explicit review (container creation is restricted)

### I need to run `git reset --hard`
✅ Works - It's in `git *` pattern (but agent may ask for confirmation on destructive ops)

### I need to run `npm install`
✅ Works - It's in `npm *` pattern

### I need to run `rm`
❌ Blocked - Deletion is always denied to prevent accidents

### I need to run `cargo test`
✅ Works - It's in `cargo *` pattern

### I need to run `python script.py`
✅ Works - It's in `python *` pattern

### I need to run `sudo something`
❌ Blocked - Privilege escalation is always denied

## Troubleshooting

### Command is blocked unexpectedly

Check the deny patterns in `vtcode.toml`:

```toml
[commands]
deny_glob = [
  "rm *",      # This blocks all rm variations
  "sudo *",    # This blocks all sudo
]
```

If you need it, either:
1. Remove it from `deny_glob`
2. Add explicit allow to `allow_list`

### I added a command but it's still blocked

Commands are checked in this order:
1. deny_list (exact match) - Blocks if found
2. deny_glob (pattern) - Blocks if found
3. deny_regex (regex) - Blocks if found
4. allow_list (exact match) - Allows if found
5. allow_glob (pattern) - Allows if found
6. allow_regex (regex) - Allows if found

If a command matches ANY deny pattern, it's blocked. Example:

```toml
allow_list = ["rm"]
deny_glob = ["rm *"]  # This takes precedence!
```

In this case, `rm` would still be blocked because `deny_glob` matches.

### I want to audit what commands ran

Check the audit logs:

```bash
ls ~/.vtcode/audit/
cat ~/.vtcode/audit/decisions.log
```

## See Also

- **[COMMAND_SECURITY_MODEL.md](./COMMAND_SECURITY_MODEL.md)** - Full documentation
- **[vtcode.toml.example](../vtcode.toml.example)** - Complete config examples
- **[EXECUTION_POLICY.md](./EXECUTION_POLICY.md)** - Overall security policy
