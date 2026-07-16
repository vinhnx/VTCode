# Tool Registry Guide

The tool registry coordinates every tool that the agent can call. It now exposes a
structured registration API that makes adding new tools predictable and testable.
This guide explains how registrations work and the expected workflow when you add
custom tooling.

## Registry architecture

The registry owns a collection of `ToolRegistration` entries. Each registration
contains the following metadata:

-   `name`: canonical tool identifier (defined in `config::constants::tools`).
-   `capability`: minimum `CapabilityLevel` required for LLM exposure.
-   `uses_pty`: whether the tool consumes a PTY session.
-   `expose_in_llm`: opt-in flag for including the tool in generated function
    declarations.
-   `behavior`: small built-in execution metadata for tool surface kind,
    mutability classification, parallel-call support, and safe-mode prompting.
-   `handler`: either a registry executor function (for built-in tools) or an
    `Arc<dyn Tool>` instance.

Ownership-first rule: when you still own the concrete tool instance, prefer
`ToolRegistration::from_tool_instance()` or a native CGP wrapper. Reserve
`Arc<dyn Tool>` registrations for tools that already have genuine shared
ownership at runtime.

`ToolRegistry::register_tool` accepts a `ToolRegistration` and updates the
internal index as well as the policy manager. Built-in registrations live in
`ToolRegistry::builtin_tool_registrations`, so the registry can initialise its
state from a single source of truth.

The canonical default public surface is `exec_command`, `write_stdin`, and
`apply_patch`. Advanced VT Code profiles may also expose `code_search` for one
literal query with optional path, file-type, result-type, and result-limit
filters. It returns recognised definitions, syntactic usages, literal text, and
matching paths. Legacy internal dispatcher and file helper names must not gain
a second public declaration path.

## Shell prompt profiles

VT Code selects a shell prompt profile for model-facing command examples:

| Platform | Default prompt profile | Notes |
|---|---|---|
| Linux | `unix_like` | Use Unix-like command syntax in `exec_command.cmd`. |
| macOS | `unix_like` | Use BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags. |
| WSL | `unix_like` | WSL is the recommended route for Unix-like workflows on Windows. |
| Native Windows | `powershell` | Use native PowerShell syntax such as `Get-ChildItem`, `Select-String`, and `Get-Content`. |

Set `agent.shell_prompt_profile` to `auto`, `unix_like`, or `powershell` to
override the prompt profile. This setting controls prompt examples and expected
command syntax only. Command approval, sandboxing, and allow-list policy remain
runtime checks. VT Code does not translate GNU-to-BSD, BSD-to-GNU,
Unix-to-PowerShell, or PowerShell-to-Unix command flags.

## Adding a new tool

First decide whether this should be a built-in VT Code tool at all.

- If the capability is external or org-specific, prefer MCP or a plugin/skill
  manifest instead of adding a new compile-time tool trait implementation.
- Treat this as an extension-boundary rule, not just an implementation
  preference: public Rust trait seams create the same "everyone must target the
  first trait" pressure that Rust coherence makes hard to unwind later.
- Add a built-in registry tool when VT Code must own the runtime behavior,
  policy surface, or UX directly.

1.  Implement the tool logic (usually by implementing the `Tool` trait or by
    exposing an async helper on `ToolRegistry`).
2.  Create a `ToolRegistration`:

    ```rust
    use vtcode_core::tools::{ToolRegistration, ToolRegistry};
    use vtcode_core::config::constants::tools;
    use vtcode_core::config::types::CapabilityLevel;

    let registration = ToolRegistration::from_tool_instance(
        tools::CREATE_FILE,
        CapabilityLevel::Editing,
        MyCreateFileTool::new(),
    )
    .with_llm_visibility(true);
    ```

3.  Register the tool. For built-in tooling update
    `ToolRegistry::builtin_tool_registrations`. For runtime additions invoke
    `ToolRegistry::register_tool` from your initialisation code.
4.  Verify the tool appears through the session catalog projections
    (`model_tools`, `schema_entries`, `acp_tools`, or `public_tool_names`) rather
    than adding a second declaration path or sidecar router mapping.
5.  Add tests that cover both registration (`available_tools`/`has_tool`) and
    execution via `ToolRegistry::execute_tool`.

### Example: GET_ERRORS

The `get_errors` tool is a built-in diagnostic tool that aggregates recent errors
from session archives and returns concise suggestions and recent error samples.
Register it as a builtin with `tools::GET_ERRORS` so the catalog can surface it
through the shared projections. When used, agents should prefer `get_errors`
output to guide self-diagnostic and self-fix logic.

## Safety guidelines

-   Prefer the canonical public tools in prompts and docs:
    `exec_command`, `write_stdin`, `apply_patch`, and advanced `code_search`
    where the profile exposes it.
-   Prefer MCP or manifest-driven extension for third-party capabilities before
    expanding VT Code's compile-time tool surface.
-   For file inspection, prefer shell commands through `exec_command.cmd`.
    For file edits, prefer `apply_patch`.
-   Tune the `[timeouts]` table in `vtcode.toml` when integrating long-running
    tooling. VT Code raises an inline warning once execution crosses the
    `warning_threshold_percent` so you can cancel runaway commands before they
    hit the ceiling.

## Testing checklist

After modifying registrations or adding new tools run the following commands
from the repository root:

-   `cargo fmt`
-   `cargo check --locked`
-   `cargo nextest run` when nextest is available

These checks validate formatting, lint rules, and runtime behaviour of the new
registry entries.
