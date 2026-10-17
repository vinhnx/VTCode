# vtcode-agent-plugins

Agent Plugins manifest parsing, validation, and discovery for VT Code.

## Overview

Implements a conformant [Agent Plugins](https://agent-plugins.org/specification) client. A plugin is a directory
containing a root `plugin.json` manifest, optional `skills/*/SKILL.md` Agent Skills, and optional `mcp.json` MCP server
configuration. The crate parses and validates manifests, discovers bundled skills and MCP servers, expands plugin
environment placeholders, and enforces path containment.

## Module Groups

| Area      | Modules        | Description                                                  |
| --------- | -------------- | ------------------------------------------------------------ |
| Manifest  | `manifest.rs`  | `PluginManifest` parsing and validation                      |
| Discovery | `discovery.rs` | `LoadedPlugin` load, skill/MCP discovery                     |
| MCP       | `mcp.rs`       | Closed `ServerConfig` variants; entry-local skip             |
| Loading   | `loader.rs`    | `PluginLoader` / `PluginInstaller` traits + filesystem impls |
| Expansion | `expansion.rs` | `PLUGIN_ROOT` / `PLUGIN_DATA` placeholder expansion          |
| Errors    | `errors.rs`    | `PluginError` diagnostics                                    |

## Key Components

### Manifest

`PluginManifest::parse` requires `$schema` and `name`; all other fields are optional. The `$schema` value must be a
supported Agent Plugins schema URL; unsupported or wrong-typed values are rejected. Unknown top-level fields are
non-fatal (reported via `unknown_fields`). Names must be 1-64 chars of `a-z`, `0-9`, `-`, `.`, start and end
alphanumeric, and contain no `--` or `..`.

### LoadedPlugin

`LoadedPlugin::load_from_dir` canonicalizes the root, reads `plugin.json` (escape rejects the plugin), discovers
`skills/*/SKILL.md` (immediate children only, escapes/wrong-kind isolated per boundary), and parses `mcp.json`
(escape/wrong-kind disables MCP only). A directory-name mismatch warns but loads by manifest name per the Agent Skills
client guide. A broken skill is skipped with a warning rather than failing the whole plugin. An `mcp.json` schema
version that differs from `plugin.json` disables MCP for that plugin.

### Loader and Installer

- `PluginLoader` / `FileSystemPluginLoader` — load a plugin from a directory.
- `PluginInstaller` / `FileSystemPluginInstaller` — `install` clones a git URL (`--depth=1`) or copies a local directory
  into `~/.agents/plugins/<name>`, then loads and validates the result; `remove` deletes an installed plugin.
- Install and remove names are validated with the same rules as manifest names, so a crafted name such as `../evil`
  cannot escape the plugins root.
- Directory copies skip the source's `.git` directory and hidden files, follow symlinks by re-creating them (never
  dereferencing), and abort on symlink cycles.

### MCP

`mcp.json` is closed (`$schema` + `mcpServers`); unknown top-level fields disable MCP only. Each server entry must
match exactly one variant: unknown or cross-variant fields, bad `command`/`cwd` forms, reserved `env` keys, and
invalid remote URLs/headers skip only that entry (`ServerConfigError::MissingField` vs `WrongType`).

### Expansion

For stdio MCP servers VT Code injects `PLUGIN_ROOT` (canonical root) and `PLUGIN_DATA`
(`data-dir/plugin-data/<plugin>`, preserved across updates) and expands `${PLUGIN_ROOT}` / `${PLUGIN_DATA}` once,
non-recursively, in `args`, `env` values, and `cwd` only. `validate_plugin_relative` requires `./`-prefixed paths,
`validate_command_token` enforces single-token commands, `validate_cwd_form` enforces the three `cwd` forms, and
`resolve_cwd` enforces post-expansion containment; symlinks escaping the root or data dir are rejected.

## Runtime Integration

- Skills: `vtcode-core::skills::loader` discovers plugin skills from `<workspace>/.agents/plugins`.
- MCP: `vtcode-core::mcp::plugin_providers::discover_plugin_mcp_providers` surfaces plugin MCP servers as
  `<plugin>.<server>` providers at session startup, from both the workspace and user plugin roots. `./`-prefixed stdio
  `command` and `cwd` values are resolved eagerly to canonical absolute paths inside the plugin root at discovery time,
  so the spawned process cannot escape the sandbox through symlinks.

## See Also

- [Agent Plugins Guide](../guides/agent-plugins.md) — setup and usage
- [Agent Plugins User Guide](../user-guide/agent-plugins.md) — task-oriented quick start
- [MCP Integration Guide](../guides/mcp-integration.md) — MCP client and server modes
- [Agent Skills Guide](../skills/SKILLS_GUIDE.md) — creating and loading skills
