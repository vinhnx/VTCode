# VT Code Configuration

VT Code configuration gives you fine-grained control over the model, execution environment, and integrations available
to the CLI. Use this guide alongside the workflows in the extension, the participant system, and the tool approval
mechanisms available in the application.

VT Code uses a configuration file named `vtcode.toml` that can be placed at the root of your project workspace to
customize behavior. Interactive sessions poll the active workspace, project, user, and explicit config layers and apply
safe changes without restarting.

## Quick navigation

- [Interface and session settings](./interface.md): settings palette, live reload, feature flags, external editor,
  fullscreen interaction, and diff preview layout.
- [Model selection and providers](./models.md): providers, custom providers, model-list overrides, whitelisting,
  model settings, and native Claude speed.
- [Agent guidance, memory, and context](./agent-memory.md): instruction guidance, persistent memory, context
  compaction, and the participant system.
- [Execution, integrations, and security](./execution-security.md): execution environment, MCP integration, WebMCP
  browser bridge, and security and approvals.
- [Profiles, operations, and reference](./profiles-reference.md): profiles and overrides, telemetry, authentication,
  CLI configuration, validation, and the config reference table.
- [User data directories](../guides/user-data-directories.md)
- [Permissions guide](../guides/permissions.md)
- [Generated field reference](./CONFIG_FIELD_REFERENCE.md)

VT Code supports several mechanisms for setting config values:

- The canonical platform config directory's `vtcode.toml` (for example, `$XDG_CONFIG_HOME/vtcode/vtcode.toml` on
  Linux/BSD). See the [user data directories guide](../guides/user-data-directories.md) for all category roots and
  overrides.
- The legacy `$VTCODE_HOME/vtcode.toml` file, defaulting to `~/.vtcode/vtcode.toml`, remains a read-compatibility and
  migration source.
- The workspace-level `vtcode.toml` file that can be placed at the root of your project (similar to `AGENTS.md` in the
  OpenAI Codex).
- Environment variables that can override certain configuration options.

Both the workspace `vtcode.toml` and the main `vtcode.toml` file support the following options:
