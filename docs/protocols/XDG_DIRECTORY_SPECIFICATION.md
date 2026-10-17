# VT Code user directories

VT Code resolves all VT Code-owned user-level files through `vtcode_commons::VtCodePaths`. The resolver follows the XDG
Base Directory Specification on Linux and BSD, and native application-data conventions on macOS and Windows.
Workspace-local `.vtcode` state is deliberately unchanged.

For practical setup, diagnostics, and rollback instructions, see the
[user data directories guide](../guides/user-data-directories.md).

## Directory categories

On Linux and BSD, the defaults below are relative to the user's home directory. The application name is `vtcode`.

| Category   | Environment variable | Default                   | Typical contents                                                                                       |
| ---------- | -------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------ |
| Config     | `XDG_CONFIG_HOME`    | `~/.config/vtcode`        | `vtcode.toml`, update settings, agents, rules, MCP/tool policy, plugin manifests, auth files           |
| Data       | `XDG_DATA_HOME`      | `~/.local/share/vtcode`   | installed plugins and skills, durable downloads, catalogs and assets                                   |
| State      | `XDG_STATE_HOME`     | `~/.local/state/vtcode`   | sessions, persistent memory, scheduler/pod state, audit/debug logs, migration reports and backups      |
| Cache      | `XDG_CACHE_HOME`     | `~/.cache/vtcode`         | model/prompt/approval caches, web-fetch files, ast-grep data, large-output spools and update snapshots |
| Runtime    | `XDG_RUNTIME_DIR`    | `$XDG_RUNTIME_DIR/vtcode` | transient sockets, locks and process coordination; falls back to `state/runtime`                       |
| Executable | `XDG_BIN_HOME`       | `~/.local/bin`            | managed helper binaries and updater-installed tools                                                    |

`VTCODE_CONFIG` and `VTCODE_DATA` override the corresponding canonical VT Code roots. `VTCODE_HOME` names the legacy
root used for compatibility and migration; it does not replace the canonical XDG roots. Empty XDG values and relative
XDG values are ignored. Absolute explicit VT Code overrides are validated and fail closed when unsafe.

System search roots follow XDG ordering. `XDG_CONFIG_DIRS` and `XDG_DATA_DIRS` are parsed as ordered, absolute path
lists; invalid entries are ignored. Their defaults are `/etc/xdg` and `/usr/local/share:/usr/share`. `/etc/vtcode`
remains an explicit compatibility configuration location.

Configuration is merged from lowest to highest precedence:

1. `/etc/vtcode` and `XDG_CONFIG_DIRS` system candidates;
2. the legacy user config under `VTCODE_HOME` or `~/.vtcode`;
3. the canonical user config under the resolved `VTCODE_CONFIG` or XDG config directory;
4. project and workspace config layers;
5. an explicit `VTCODE_CONFIG_PATH` file as the highest file-based override;
6. runtime and CLI overrides.

User writes always use the canonical user config path, even when a legacy or system file supplied the value that was
read. Optional inaccessible system and user candidates are skipped. A present malformed file, an explicit
`VTCODE_CONFIG_PATH`, or a workspace-file error remains actionable and includes the path in its diagnostic.

## Platform roots

- Linux/BSD use the six XDG categories above.
- macOS uses Application Support for config/data, a native cache directory for cache, a private state subdirectory for
  mutable state, and the managed binary directory selected by VT Code.
- Windows uses native application-data and local-app-data roots, with separate config, data, state, cache and binary
  children.

Run `vtcode --version` to print the resolved config, data, state, cache, runtime, executable, legacy and migration
paths.

## Legacy migration and rollback

At startup, before normal configuration loading, VT Code scans the legacy root once. It maps legacy files by category
into canonical roots, copies only regular files and directories, never follows symlinks or special files, and never
overwrites an existing destination. Files are published through private temporary siblings and an atomic no-replace
operation. The legacy directory is never deleted, so it remains a rollback-safe backup.

The mapping is explicit: configuration files, `AGENTS.md`/`CLAUDE.md`, rules, prompts, commands, tool policy, MCP
configuration and auth move to config; plugins, skills, durable assets, legacy `tools/` and `bin/` move to data or the
executable directory; projects, sessions, memory, scheduler/pod state, logs, audits and backups move to state; and
model/prompt/approval caches, installer state, web-fetch files and large-output spools move to cache. Unknown top-level
legacy entries are retained and recorded as unmapped rather than silently reclassified.

Migration writes a versioned marker and a JSON report under the state directory only after the scan completes. The
report records copied, skipped, conflicting and failed entries. Individual copy failures are nonfatal; VT Code continues
using the legacy fallback where a destination was not created. If a retryable failure occurs, the report is still
written but the completion marker is withheld, so the next startup retries the failed entries. Once the marker is
published, later runs are idempotent.

Legacy `tmp/` content is intentionally not migrated. New transient files use the runtime/cache policy, while the old
directory remains available only for compatibility and is not automatically removed.

To roll back a migration, stop VT Code, keep the new roots as a backup, remove the migration marker from
`<state>/migration/legacy-v1.complete`, and start with `VTCODE_HOME` pointing at the preserved legacy directory. Resolve
any destination conflicts manually; VT Code will not overwrite them.

Authentication lookup also checks legacy `auth.json` locations when startup migration is bypassed. New credentials are
always written under the canonical private config/auth directory. Auth directories use mode `0700` and auth files use
mode `0600` on Unix. Newly created user roots are private; existing directories are not made more restrictive unless
they hold security-sensitive runtime or credential data.

## Compatibility boundaries

The following roots are external compatibility surfaces and are not VT Code migration targets:

- workspace `.vtcode` files such as IPC, code-temp, dynamic context, plans and workspace sessions;
- `.agents`, `.codex`, `.claude`, `CODEX_HOME`, and system skill directories.

VT Code may read those locations for interoperability, but it does not move or reclassify their contents.

## References

- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/)
- [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md)
- [User data directories guide](../guides/user-data-directories.md)
- [Security model](../development/COMMAND_SECURITY_MODEL.md)
