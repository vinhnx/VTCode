# Configuration Precedence in VT Code

This document summarizes how VT Code discovers configuration at startup and how default values and runtime validation interact with user-provided settings.

## Resolution Order

When the CLI starts it looks for `vtcode.toml` in the following locations. The first file that exists is loaded and validated.

1. **Workspace root** – `<workspace>/vtcode.toml`
2. **Workspace-specific directory** – `<workspace>/.vtcode/vtcode.toml`
3. **User home directory** – `~/.vtcode/vtcode.toml`
4. **Project profile** – `<workspace>/.vtcode/<project>/vtcode.toml`
5. **Built-in defaults** – if no file is found, the compiled default configuration is used

This precedence allows local overrides while still falling back to organization-level or user-level defaults.

## Default Values

Layered defaults are defined in the Rust sources so the application can generate a baseline configuration and reason about missing fields:

- **Global configuration defaults** live in `vtcode-core/src/config/defaults/`
- **Syntax highlighting defaults** are centralized in `syntax_highlighting.rs` and reused by the loader and serde
- **Context, router, and tooling defaults** remain close to their owning modules but consume the shared constants exported by the defaults module

The CLI uses these defaults when generating sample configs (`vtcode init`) and when no user configuration is present.

## Validation

Every configuration loaded from disk now goes through `VTCodeConfig::validate`. The validator performs:

- Syntax highlighting checks (minimum file size, timeout, language entries)
- Context subsystem checks (ledger limits, token budget thresholds, curation limits)
- Router checks (heuristic thresholds and required model identifiers)

Validation is applied both to user-provided files and the built-in defaults. Any validation error is surfaced with contextual messaging that includes the offending file path.

## Environment Variables

Environment variables such as `GEMINI_API_KEY` and `VTCode_CONFIG_PATH` still participate in runtime behavior (API key selection, workspace overrides), but they do not bypass validation—once the configuration is constructed, the same validation rules are applied.

## Developer Tips

- Prefer updating the shared defaults module when adding new configuration knobs so CLI bootstrapping and serde defaults stay aligned.
- Add focused validation routines next to the structs that own the data to keep error messages specific and maintainable.
- Update unit tests in `vtcode-core/src/config/loader/mod.rs` when adjusting precedence rules or default values to avoid regressions.
