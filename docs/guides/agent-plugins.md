# Agent Plugins

VT Code supports the [Agent Plugins](https://agent-plugins.org/specification) portable package format. A plugin is a
directory containing a root `plugin.json` manifest, optional `skills/*/SKILL.md` Agent Skills, and optional `mcp.json`
MCP server configuration.

For a task-oriented walkthrough, see the [Agent Plugins User Guide](../user-guide/agent-plugins.md).

## Directory layout

```text
my-plugin/
├── plugin.json
├── skills/
│   └── my-skill/
│       ├── SKILL.md
│       ├── scripts/
│       └── references/
├── mcp.json
└── com.vtcode.client/
    └── hooks/
```

## Manifest

`plugin.json` requires two fields:

- `$schema` — must reference the Agent Plugins schema (`https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`)
- `name` — 1-64 characters of `a-z`, `0-9`, `-`, `.`; must start and end alphanumeric and must not contain `--` or `..`

Other recognized fields (`version`, `description`, `author`, `homepage`, `repository`, `license`, `keywords`,
`extensions`) are optional. Unknown top-level fields are non-fatal and reported.

## Discovery roots

VT Code discovers plugins from two roots:

- **Project plugin root**: `<workspace>/.agents/plugins/`
- **User plugin root**: `~/.agents/plugins/`

Each immediate child directory containing a valid root `plugin.json` is treated as a plugin. Directories without a valid
`plugin.json` fall back to generic skill discovery (preserving backward compatibility with existing `.agents/plugins`
layouts).

The two roots are used differently:

- **Skills** are discovered from both roots: the project root takes precedence over the user root.
- **MCP servers** are discovered from both roots at session startup.
- **`plugins list` and `plugins info`** scan both roots.

## Example usage

[`vinhnx/vtcode-plugins`](https://github.com/vinhnx/vtcode-plugins) is a real, open-source plugin that bundles nine
general-purpose Agent Skills extracted from VT Code itself (Rust coding rules, codemod migrations, code review,
workspace analysis, and more).

```bash
# Install from GitHub
vtcode plugins add https://github.com/vinhnx/vtcode-plugins.git

# Confirm the install
vtcode plugins list
vtcode plugins info vtcode-plugins

# Use a bundled skill in a session
# Ask the agent: "Use the rust-skills skill"
```

After installing, the skills are loaded at the next session start and are available to the agent like any other skill.
Installable project-locally instead:

```bash
mkdir -p .agents/plugins
git clone https://github.com/vinhnx/vtcode-plugins.git .agents/plugins/vtcode-plugins
```

## Skills

Plugin skills are discovered from `skills/*/SKILL.md` (immediate children of `skills/` only, no recursive descent).
A missing `skills/` directory is valid absence; a `skills` path that is not a directory or escapes the plugin root
disables only skills. Each `SKILL.md` must resolve within the plugin root (symlink escapes are skipped) and pass
Agent Skills validation. A directory-name mismatch warns but loads by manifest name so cross-client renames still
work; other broken skills are skipped with a warning, never fatal to the plugin.

## MCP servers

Plugin MCP servers are declared in `mcp.json` (`$schema` + `mcpServers` only; unknown top-level fields disable MCP
for that plugin). Each entry is a closed variant and is skipped independently when invalid:

- `stdio`: `type`, `command` (single token: bare name or `./`-relative, no shell strings), optional `args`, `env`
  (string map, must not set `PLUGIN_ROOT`/`PLUGIN_DATA`), `cwd` (`./...`, `${PLUGIN_ROOT}[/...]`, or
  `${PLUGIN_DATA}[/...]` only).
- `streamable-http`: `type`, `url` (absolute http/https, no userinfo/fragment, non-loopback requires https),
  optional literal `headers` (valid field names, case-insensitive duplicates rejected, no expansion).
- `sse`: parsed but skipped (unsupported by VT Code's MCP client).

A `mcp.json` schema version that differs from `plugin.json` disables MCP for that plugin; other components still load.

At session startup each plugin MCP server is exposed as an MCP provider named `<plugin-name>.<server-name>` (for
example, `my-plugin.local`), available through `/mcp` like any other configured server.

### Environment expansion

For stdio servers, VT Code injects `PLUGIN_ROOT` (canonical plugin root) and `PLUGIN_DATA` (client-managed
`data-dir/plugin-data/<plugin>`, preserved across updates) and expands `${PLUGIN_ROOT}`/`${PLUGIN_DATA}` once,
non-recursively, in `args`, `env` values, and `cwd` only — never in `command`, `env` keys, URLs, or headers.
`PLUGIN_DATA` is created automatically before launch. Unrecognized placeholders stay literal.

### Path containment

Every package file read or executed must resolve within the filesystem-resolved plugin root (symlinks followed).
`plugin.json` escape rejects the plugin; `skills/` or `mcp.json` escape/wrong-kind disables only that type;
`SKILL.md` escape skips only that skill; stdio `command`/`cwd` escapes skip only that server. `./`-prefixed paths
are canonicalized eagerly before spawn. Names passed to `plugins add --name` and `plugins remove` must satisfy the
same rules as manifest names, so a crafted name cannot escape the plugins directory.

### Portable vs VT Code-owned

Portable core is `plugin.json` + `skills/` + `mcp.json` + `${PLUGIN_ROOT}`/`${PLUGIN_DATA}`. Installation sources
(git/local), `plugins list/info/add/remove` UX, trust/sandboxing, skill presentation, and the legacy
`.vtcode-plugin/` template (`commands/`, `agents/`, `hooks/`, `.mcp.json`, `${VTCODE_PLUGIN_ROOT}`) are VT Code-owned
client behavior under `com.vtcode.*` and are not portable.

## CLI

```bash
vtcode plugins list
vtcode plugins info <name>
vtcode plugins validate <path>
vtcode plugins add <git-url-or-local-path> --name <id>
vtcode plugins remove <name>
```

`plugins add` installs to `~/.agents/plugins/<name>`: git URLs are cloned with `git clone --depth=1`, and local
directories are copied after verifying they contain a valid `plugin.json`. `plugins remove` uninstalls from the user
plugin root.

## Config

Plugin behavior is controlled by the existing `tools.plugins` section in `vtcode.toml`:

```toml
[tools.plugins]
enabled = true
default_trust = "sandbox"
allow = ["my-plugin"]
deny = []
auto_reload = true
```

`default_trust` accepts `sandbox` (default), `trusted`, or `untrusted`. See the
[Configuration Field Reference](../config/CONFIG_FIELD_REFERENCE.md) for the full set of `tools.plugins.*` options.

## Conformance

VT Code is a conformant Agent Plugins client for both component types (skills + stdio/streamable-http MCP). See the
[Agent Plugins specification](https://agent-plugins.org/specification) for the portable package contract.

## See also

- [Agent Plugins User Guide](../user-guide/agent-plugins.md) — task-oriented quick start
- [vtcode-agent-plugins module](../modules/vtcode_agent_plugins.md) — crate reference
