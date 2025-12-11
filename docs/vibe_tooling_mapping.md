## External tooling takeaways → vtcode mapping

-   **Permissions/config/state**: Reference configs bundle `permission` (ASK/ALWAYS/NEVER), `workdir`, allowlist/denylist, and expand `workdir` to an effective path; per-tool state is first-class. vtcode today centralizes policy in `ToolPolicyManager` and lacks per-tool config/state metadata. Add enum + metadata on registrations and surface effective workdir normalization.
-   **Prompt metadata**: Tools can load Markdown prompts from sibling `prompts/<tool>.md` or an override `prompt_path`. vtcode tools should carry optional prompt-path metadata on registrations for LLM context loading.
-   **Schema export**: Upstream `get_parameters()` cleans Pydantic JSON schema (drops titles/desc) so tools expose stable schemas. vtcode should expose parameter/config/state schemas on `ToolRegistration` for MCP/LLM use.
-   **Allow/deny lists**: Per-tool allow/deny matchers can short-circuit permission before ASK/ALWAYS/NEVER. vtcode should store allow/deny patterns per tool and feed them into policy evaluation.
-   **MCP proxying**: Remote tools are proxied with aliases (`alias_tool`) and server hints, using provider discovery and `input_schema` as parameters. vtcode already has an MCP client; add an adapter that registers discovered tools into the registry with aliases (`mcp::<provider>::<tool>`, `mcp_<tool>`), parameter schemas, and server hints, then route execution through the MCP client.
-   **Discovery & defaults**: Discovery scans multiple roots (workspace `.vibe/tools`, global home, configured paths) and merges defaults per tool. vtcode registry should continue using built-ins but allow MCP/discovered tools to carry defaults and prompt metadata without breaking existing registrations.
-   **Grep behavior**: Upstream grep falls back to `perg` when `rg` is missing, caps matches, and respects ignore patterns with optional hidden/binary search. vtcode already caps matches (5) and falls back to `perg`; tighten ignore defaults, add timeout/byte truncation knobs, and surface cache/history.
-   **Search/replace**: Includes block-level tools with backup options and fuzzy context hints. vtcode should add a search-replace tool (or extend apply_patch) with workspace validation, size limits, optional backup, and context matching.
-   **Backcompat/flags**: Changes should be gated via config/constants so existing registrations and tooling stay intact.

## Implemented in this pass

-   Tool metadata now carries prompt path, schemas, permissions (ASK/ALWAYS/NEVER analog), allow/deny patterns, and aliases without breaking existing registrations.
-   MCP tools are registered as first-class tools via an adapter (`mcp::<provider>::<tool>` with `mcp_<tool>` aliases) using the existing `McpClient` and exported schemas.
-   Grep gains default ignore globs, byte truncation, timeout support, and truncation flags; caching keys include the new knobs.
-   Added `search_replace` tool with workspace validation, optional backups, context hints, and safety limits; minimal regression tests cover grep truncation and search/replace.
-   Default tool policies updated for the new tool; feature remains backward compatible through constants and aliases.
