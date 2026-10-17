# Sem local code discovery

[Sem](https://github.com/Ataraxy-Labs/sem) exposes code-entity search, callers, dependency information,
and structural diffs through a local stdio MCP server. This optional example uses VT Code's existing MCP client;
it does not replace built-in file tools or change shipped defaults. No Sem account or API key is required.

## Setup

1. Install Node.js/npm and Git. The pinned npm package downloads a native Sem binary on first installation.
   The example targets macOS and Linux on x64 or ARM64.
2. Open the [example configuration](../examples/sem-mcp.toml) and replace `SEM_REPO` with the absolute path
   to an existing, trusted Git repository. This is the repository Sem will inspect, not the plugin install directory.
3. Merge the example into your existing `vtcode.toml`. If `[mcp]` already exists, update its `enabled` flag
   and append the provider entry; do not duplicate the table or remove other providers and approval settings.
4. Inspect configuration with `vtcode mcp get sem` and `vtcode mcp list`. These commands do not prove a live
   connection. Start a new VT Code session and check that the Sem tools are discovered.

If transport requirements are enforced, permit `npx` in your existing `mcp.requirements.allowed_stdio_commands`.
If tool allowlists are enforced, add only the Sem tools you need to the provider-specific allowlist.
See [MCP integration](mcp-integration.md#allowlist-behaviour); do not disable existing approval controls.

## Try a bounded review

Ask the agent, substituting a symbol that exists in the selected repository:

> Use Sem to locate `parse_config`, inspect its callers, and explain the relevant code. Do not edit files or run tests.

Then, for a repository with tracked local changes:

> Use Sem to inspect the structural diff. Report what changed and what remains uncertain; do not run validation commands.

Inspect the discovered schemas before calling tools directly. Entity and relation coverage depends on the language
and source constructs; missing graph results do not prove that there are no callers or dependencies. Retain file
reads and text search for unresolved code, configuration, and other non-entity content.

## Execution and privacy

- `SEM_NO_NETWORK=1` disables Sem cloud access and `SEM_NO_TELEMETRY=1` disables Sem telemetry recording.
  Initial npm installation still needs network access to download the package and native binary.
- Tool results enter the agent's context and may reach its configured model provider. This is not an offline-model setting.
- `sem_check` can execute repository build/test commands. Only connect trusted repositories and approve execution
  deliberately. Neither `SEM_REPO` nor the network flag is an operating-system sandbox for those commands.
- Sem maintains a local index. Static structural information is not proof that a change compiles or passes tests.

## Troubleshooting and removal

- No repository: verify the absolute `SEM_REPO` path points to a Git checkout accessible to the launched process.
- No executable: check Node/npm on VT Code's PATH and that npm install scripts were allowed to download Sem.
- First-start timeout: install the pinned package separately with `npm install -g @ataraxy-labs/sem@0.27.0`,
  then retry. Keep the same version when comparing results.
- Disable this integration with `enabled = false` on the `sem` provider or remove that provider only.
  Leave global MCP enabled if other providers need it. Removing the provider does not erase existing conversations
  or Sem's local cache.
