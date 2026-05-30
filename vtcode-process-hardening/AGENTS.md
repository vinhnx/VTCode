# vtcode-process-hardening

[Root AGENTS.md](../AGENTS.md) | OS process hardening. Single entry: `pre_main_hardening()` — call first in `main()`.

## Platform Matrix

Linux: `prctl(PR_SET_DUMPABLE, 0)` + `RLIMIT_CORE=0` + strip `LD_*` | macOS: `ptrace(PT_DENY_ATTACH)` + `RLIMIT_CORE=0` + strip `DYLD_*` | BSD: `RLIMIT_CORE=0` + strip `LD_*` | Windows: no-op placeholder

All Unix: cap `RLIMIT_STACK` to 8 MiB if unlimited.

## Rules

- All `unsafe` must have `// SAFETY:` comments.
- Exit codes: `PRCTL_FAILED=5`, `PTRACE_DENY_ATTACH_FAILED=6`, `SET_RLIMIT_CORE_FAILED=7`.
- Platform code via `#[cfg(target_os)]` at function level.

## Gotchas

- `remove_env_var` must run before thread spawn — unsafe env mutation.
- `cap_stack_rlimit` silently returns on EINVAL — intentional.
- Windows hardening is placeholder — do not assume it works.
