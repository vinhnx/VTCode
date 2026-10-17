<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# vtcode-agent-plugins

[Root AGENTS.md](../../../AGENTS.md) | Agent Plugins manifest parsing, validation, and discovery.

## Modules

`manifest.rs` PluginManifest | `mcp.rs` McpConfig | `discovery.rs` plugin discovery | `loader.rs` loading sources | `expansion.rs` placeholder expansion | `errors.rs` PluginError

## Conventions

- `PluginManifest` and `McpConfig` are passive data containers; all validation is explicit in `parse()` and returns diagnostics. Wrong-typed top-level fields (e.g. `$schema`, `mcpServers`) and unsupported `$schema` URLs are rejected, not silently dropped.
- MCP entries are closed variants: unknown/cross-variant fields, bad `command`/`cwd` forms, reserved `env` (`PLUGIN_ROOT`/`PLUGIN_DATA`), invalid URLs/headers skip only that entry with a warning. `ServerConfigError` distinguishes `MissingField` from `WrongType`.
- Unknown top-level `plugin.json` fields are non-fatal (reported and ignored); unknown top-level `mcp.json` fields disable MCP for that plugin only.
- Skill discovery is `skills/*/SKILL.md` immediate children only; canonical root + per-boundary containment (`plugin.json` fatal, type invalid, skill skipped). Dir-name mismatch warns but loads by manifest name.
- `expansion.rs` is single-pass non-recursive (`args`/`env` values/`cwd` only); `resolve_cwd` enforces the 3 `cwd` forms + containment; `default_plugin_data_dir` is `data-dir/plugin-data/<name>` (survives updates).
- Any user-controlled name or path joined onto the plugins root must go through `validate_name` / `validate_plugin_relative` (canonicalize-based) first — `install`, `remove`, and MCP `command`/`cwd` are the security boundaries. Directory copies re-create symlinks, skip dotfiles, and abort on cycles.

## Dependencies

- `vtcode-skills` (SKILL.md frontmatter parsing and `SkillManifest::validate`)
- `vtcode-commons` (filesystem helpers)
