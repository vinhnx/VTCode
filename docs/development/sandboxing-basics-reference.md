# Sandboxing Basics — Reference Notes

Reference artifact for VT Code contributors and coding agents working on the sandbox/exec boundary
(`crates/codegen/vtcode-safety/src/sandboxing/`).

## Source (not vendored)

- Title: **Software sandboxing: The basics**
- Author: Emilua blog
- Published: 2025-01-12
- URL: <https://blog.emilua.org/2025/01/12/software-sandboxing-basics/>

The full text lives at the URL above and is **not reproduced here** (© Emilua, all rights reserved). What follows is an
original distillation plus a small number of short attributed quotations for orientation. When in doubt, read the
original — it is the authority; this file is only VT Code's working notes.

## Why this matters to VT Code

VT Code executes untrusted model-directed commands. The article's core thesis is that in-process checks are insufficient
and the privilege boundary must be a process boundary enforced by the kernel, with privileges that only ever decrease.
That is exactly the shape of VT Code's Linux launcher (`vtcode sandbox-exec`: Landlock filesystem rules + seccomp
syscall filtering

- `PR_SET_NO_NEW_PRIVS`) and the reason hostname allowlists and macOS Seatbelt domain filtering fail closed instead of
  degrading silently.

## Distilled principles (in our own words)

1. **Sandboxing = discretionary privilege dropping.** Three parts: done programmatically by the application, without
   needing administrator rights, and only ever reducing privileges — never raising them.
2. **Complement, don't replace, sysadmin policy.** Filesystem permissions and container tools are the administrator's
   layer; application sandboxing sits on top of it.
3. **No superuser helpers for sandboxing.** Setuid helpers and user namespaces widen kernel attack surface; namespaces
   belong to trusted container tools, not to per-command sandboxing.
4. **The boundary is the process.** Credentials live at process granularity; thread-level schemes do not survive contact
   with real kernels and libc.
5. **Compartmentalized programs are distributed programs.** One process per compartment, communicating by message
   passing; the broker owns every grant and workers never hand capabilities to each other.
6. **File descriptors behave like capabilities — except `ioctl`.** Permission is checked at creation, not at use, so
   passing an FD grants its rights; `ioctl` requests are the exception and must be filtered individually.
7. **Capsicum is the ideal to imitate.** One call that disables ambient authority, fine-grained rights reduction per FD,
   and directory-relative resolution that cannot escape beneath the granted root.
8. **On Linux the answer is Landlock + seccomp.** Allow `open` and constrain it with Landlock rather than interposing
   libc; use seccomp as hardening around the edges, not as the policy engine.
9. **Blocklists are fragile.** New syscalls, per-architecture numbering, and the x32 ABI alias bit all bypass naive
   deny-lists; duplicate blocked numbers across ABIs and kill mismatched architectures outright.
10. **Deny the `/proc/self/fd` reopen.** If a child can reopen its own FDs by path with a different mode, FDs stop being
    capabilities. Keep `/proc` and `/sys` ungranted and never grant `/dev` wholesale (`/dev/fd` aliases the same
    escape).
11. **Broker hygiene for received output.** Non-blocking reads, bounded buffers, bounded close/drain — a sandboxed child
    must not be able to hang or exhaust the broker.
12. **Interrogate the threat model.** Sandboxing code you already trust with the data may be pointless; sandboxing code
    that handles attacker data (parsers, renderers, decompressors) is where it pays.

## Short attributed quotations

> "Compartmentalised application development is, of necessity, distributed application development, with software
> components running in different processes and communicating via message passing." — Watson, Anderson, Laurie &
> Kennaway, _Capsicum: practical capabilities for UNIX_, as quoted in the article.
>
> "Seccomp is not a good mechanism for discretionary privilege dropping. Seccomp is a good mechanism for OS hardening."
> — the article, on why seccomp stays defense-in-depth behind Landlock.
>
> "If you can learn just 3 functions, you can code for the actor model: `spawn_vm`, `send`, `receive`." — the article,
> paraphrased; VT Code's analogue is broker-spawns-worker, parent↔child pipes, no worker↔worker channels.

## How VT Code applies it

| Principle                                   | VT Code location                                                           | Status                   |
| ------------------------------------------- | -------------------------------------------------------------------------- | ------------------------ |
| Only-decrease (`NO_NEW_PRIVS`)              | `sandboxing/linux_seccomp.rs::apply_seccomp_filter`                        | Enforced                 |
| No namespaces as sandbox                    | `NAMESPACE_CLONE_FLAGS` + `clone3`→`ENOSYS`                                | Enforced                 |
| Process boundary                            | `sandbox-exec` launcher, `child_spawn.rs`                                  | Enforced                 |
| Broker/worker tree, no FD forwarding        | `sandboxing/mod.rs` docs, `manager.rs`                                     | Convention               |
| FDs as capabilities, `ioctl` filtered       | `TIOCSTI`/`TIOCSCTTY` seccomp rules; Landlock `IoctlDev` unhandled for PTY | Enforced                 |
| Blocklist + x32 aliases + arch kill         | `BLOCKED_SYSCALLS`, `X32_SYSCALL_BIT`, seccompiler arch validation         | Enforced                 |
| `/proc`/`/sys` ungranted, `/dev` restricted | `VIRTUAL_FS_ROOTS`, `restrict_dev_grants`                                  | Enforced                 |
| Hostname allowlists fail closed             | `manager.rs`, `linux.rs`, `security.md`                                    | Enforced                 |
| Reusable policy families + version          | `SECCOMP_PROFILE_VERSION`, Kafel-family comments                           | Convention               |
| `LD_PRELOAD`/libc interposition broker      | —                                                                          | Deliberately not adopted |
| Full whitelist rewrite                      | —                                                                          | Future work, not started |

## Deliberately not adopted

- **`LD_PRELOAD` / `libc_service` brokering** for legacy binaries: complexity outweighs coverage while Landlock handles
  our command set.
- **Namespace-based isolation**: reserved for trusted container tools.
- **Kafel DSL import**: family labels and versioning adopted; the language itself not vendored.

## For the coding agent

Consult this file before touching `sandboxing/`, `child_spawn.rs`, `exec_env.rs`, or `manager.rs`. Preserve the
fail-closed direction on every edit: an unknown syscall, path, or architecture must deny, never allow. Keep
`syscall_number` in sync with `BLOCKED_SYSCALLS`, keep `SECCOMP_PROFILE_VERSION` bumped with blocklist edits, and keep
this table accurate. Run `cargo nextest run -p vtcode-safety` plus the Linux cross check documented in the crate's
history before claiming done.
