# VT Code user data directories

VT Code keeps workspace state and user-global state separate. Workspace files remain in the project, while user-global
files are resolved through one platform-aware path policy.

For the complete contract and implementation-level details, see the
[XDG directory specification](../protocols/XDG_DIRECTORY_SPECIFICATION.md).

## Find the active paths

Run:

```bash
vtcode --version
```

The extended version output shows the resolved config, data, state, cache, runtime, executable, and legacy directories,
plus the migration marker and report paths. It also prints the relevant environment variables so a sandbox, service, or
child process can be diagnosed without guessing which root is in use.

## Directory categories

On Linux and BSD, VT Code follows the XDG Base Directory Specification. The defaults are:

| Category   | Environment variable | Default                   | Use                                                                               |
| ---------- | -------------------- | ------------------------- | --------------------------------------------------------------------------------- |
| Config     | `XDG_CONFIG_HOME`    | `~/.config/vtcode`        | `vtcode.toml`, rules, agents, MCP/tool policy, plugin manifests, and auth files   |
| Data       | `XDG_DATA_HOME`      | `~/.local/share/vtcode`   | Installed plugins and skills, durable downloads, catalogs, and assets             |
| State      | `XDG_STATE_HOME`     | `~/.local/state/vtcode`   | Sessions, memory, scheduler/pod state, logs, audits, and migration backups        |
| Cache      | `XDG_CACHE_HOME`     | `~/.cache/vtcode`         | Model/prompt/approval caches, installer state, web-fetch files, and output spools |
| Runtime    | `XDG_RUNTIME_DIR`    | `$XDG_RUNTIME_DIR/vtcode` | Private sockets, locks, and transient process coordination                        |
| Executable | `XDG_BIN_HOME`       | `~/.local/bin`            | Managed helper binaries and updater-installed tools                               |

On macOS and Windows, VT Code uses native application-data and cache roots. The categories remain separate where the
platform supports them, but the exact native root is intentionally delegated to the operating system. Use
`vtcode --version` rather than hard-coding a platform path.

## Environment variables

The following variables control resolution:

| Variable                                                               | Effect                                                                                         |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `VTCODE_CONFIG`                                                        | Absolute override for the canonical user config directory                                      |
| `VTCODE_DATA`                                                          | Absolute override for the canonical user data directory                                        |
| `VTCODE_CONFIG_PATH`                                                   | Explicit config file layer; this takes precedence over normal file layers                      |
| `VTCODE_HOME`                                                          | Legacy `~/.vtcode` source used for compatibility and migration; it is not the new storage root |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME` | XDG category roots on Linux/BSD                                                                |
| `XDG_RUNTIME_DIR`                                                      | Parent of the private VT Code runtime directory on Linux/BSD                                   |
| `XDG_BIN_HOME`                                                         | Managed executable directory on Linux/BSD                                                      |
| `XDG_CONFIG_DIRS`, `XDG_DATA_DIRS`                                     | Ordered system search roots                                                                    |

Empty or relative XDG values are ignored. Explicit `VTCODE_CONFIG` and `VTCODE_DATA` overrides must be absolute; unsafe
values fail closed. System directory lists accept only absolute entries. `/etc/vtcode` remains a Unix compatibility
configuration location.

The `--config` option has two forms:

```bash
# Select an explicit file layer
vtcode --config /path/to/vtcode.toml

# Apply an inline key/value override
vtcode --config agent.provider=ollama
```

Inline key/value overrides are applied above file-based layers. The `VTCODE_CONFIG_PATH` environment variable selects
the same explicit-file layer when a command-line path is not supplied. Relative paths and `~` are resolved identically
for both forms. The resolved file is captured as a session override: configuration reloads and session config writes
made during that session (settings palette, slash-command persistence, live reload) target the same explicit file.
Global-only operations such as `vtcode mcp login` always use the canonical user config file and ignore the session
override.

## Configuration precedence

Global configuration is merged from low to high precedence:

1. `/etc/vtcode` and the ordered `XDG_CONFIG_DIRS` candidates;
2. the legacy `VTCODE_HOME/vtcode.toml` file;
3. the canonical user config file;
4. project and workspace config layers;
5. `VTCODE_CONFIG_PATH` or an explicit `--config /path/to/file.toml`;
6. inline `--config key=value` and runtime CLI overrides.

Optional inaccessible system and user candidates are skipped. A present malformed file, an explicit config path, or a
workspace config error remains an actionable error. Writes always target the canonical user config directory, never
whichever file happened to be read first.

The reset command is the exception by design: `vtcode config reset` clears the active workspace file (or the explicit
file selected with `--config`), while `vtcode config reset --global` clears the canonical user file and
`vtcode config reset --project` clears the current project profile. Only the selected file is cleared; lower-precedence
layers and credentials are retained. The settings palette shows its target path and asks for confirmation before running
the same reset service.

## Migration from `~/.vtcode`

Migration runs once during startup, before normal configuration loading.

- The source is `VTCODE_HOME`, or `~/.vtcode` when it is unset.
- Only validated regular files and directories are copied.
- Symlinks and special files are skipped or reported; they are never followed.
- Existing destination files are never overwritten.
- Files are staged in private temporary siblings and published atomically.
- The legacy directory is preserved as a rollback-safe backup.
- `tmp/` is not migrated; new transient files use runtime/cache policy.

The scan report is written to `<state>/migration/legacy-v1.json`. The completion marker is
`<state>/migration/legacy-v1.complete`. The marker is published only after the scan and report succeed. Individual copy
failures are nonfatal, but they keep the marker from being published so the next startup retries them. Repeated
successful starts are idempotent.

### Rollback

To roll back after stopping VT Code:

1. Keep the new XDG/native roots as a backup.
2. Remove the completion marker from the resolved state directory.
3. Set `VTCODE_HOME` to the preserved legacy directory.
4. Start VT Code and resolve any destination conflicts manually.

VT Code does not delete the legacy source or overwrite conflicting new files.

## Workspace versus user-global state

Workspace-local state is intentionally unchanged:

| Scope                     | Examples                                                                                    |
| ------------------------- | ------------------------------------------------------------------------------------------- |
| Workspace `.vtcode`       | IPC, code-temp files, dynamic context, plans, workspace sessions, and project configuration |
| User-global config        | Authored agents/rules, MCP and tool policy, plugin manifests, and credentials               |
| User-global data          | Installed plugins/skills and durable assets                                                 |
| User-global state         | Persistent memory, cross-workspace sessions, logs, audits, scheduler/pod state, and backups |
| User-global cache/runtime | Re-creatable caches, fetch files, output spools, locks, sockets, and coordination files     |

`.agents`, `.codex`, `.claude`, `CODEX_HOME`, and system skill directories are external compatibility surfaces. VT Code
may read them, but migration does not move or reclassify their contents.

## Permissions and troubleshooting

New user directories are created privately (`0700` on Unix). Runtime and auth directories are private, and auth files
use `0600` on Unix. Existing directory permissions are preserved unless a security-sensitive file requires stricter
handling.

If files appear in an unexpected location, run `vtcode --version` and inspect:

1. the resolved category paths;
2. `VTCODE_CONFIG`, `VTCODE_DATA`, and `VTCODE_HOME`;
3. the XDG variables and system search lists;
4. the migration report path and its recorded failures or conflicts.

For configuration-layer details, see [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md). For security
boundaries, see the [security guide](security.md).
