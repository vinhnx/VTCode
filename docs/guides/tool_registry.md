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
-   `handler`: either a registry executor function (for built-in tools) or an
    `Arc<dyn Tool>` instance.

`ToolRegistry::register_tool` accepts a `ToolRegistration` and updates the
internal index as well as the policy manager. Built-in registrations live in
`ToolRegistry::builtin_tool_registrations`, so the registry can initialise its
state from a single source of truth.

## Adding a new tool

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
4.  Provide a `FunctionDeclaration` entry (typically in
    `build_function_declarations`) to document the schema exposed to LLMs.
5.  Add tests that cover both registration (`available_tools`/`has_tool`) and
    execution via `ToolRegistry::execute_tool`.

### Example: GET_ERRORS

The `get_errors` tool is a built-in diagnostic tool that aggregates recent errors
from session archives and returns concise suggestions and recent error samples.
Register it as a builtin with `tools::GET_ERRORS` and add a `FunctionDeclaration`
so LLMs can discover it. When used, agents should prefer `get_errors` output to
guide self-diagnostic and self-fix logic.

## Safety guidelines

-   Prefer `edit_file`/`write_file` for high-impact assets. Reach for
    `apply_patch` only when you have reviewed the diff locally or staged a
    backup—failed patches can leave partially rewritten files.
-   Tune the `[timeouts]` table in `vtcode.toml` when integrating long-running
    tooling. VTCode raises an inline warning once execution crosses the
    `warning_threshold_percent` so you can cancel runaway commands before they
    hit the ceiling.

## Testing checklist

After modifying registrations or adding new tools run the following commands
from the repository root:

-   `cargo fmt`
-   `cargo clippy`
-   `cargo nextest run`

These checks validate formatting, lint rules, and runtime behaviour of the new
registry entries.
