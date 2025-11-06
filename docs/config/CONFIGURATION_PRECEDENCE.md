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

### Inline CLI overrides

Inspired by [OpenAI Codex CLI](https://github.com/openai/codex), VT Code now accepts
`-c/--config key=value` overrides directly on the command line. These overrides
apply **after** the configuration file is loaded, making them the highest
precedence layer. Use multiple flags to set several keys during a single run.
For example:

```
vtcode --workspace ~/repo \
  --config agent.provider="openai" \
  --config context.curation.enabled=false
```

Relative config paths passed via `--config path/to/vtcode.toml` remain supported
and are resolved against the workspace before falling back to the current
working directory.

## Default Values

Layered defaults are defined in the Rust sources so the application can generate a baseline configuration and reason about missing fields:

-   **Global configuration defaults** live in `vtcode-core/src/config/defaults/`
-   **Syntax highlighting defaults** are centralized in `syntax_highlighting.rs` and reused by the loader and serde
-   **Context, router, and tooling defaults** remain close to their owning modules but consume the shared constants exported by the defaults module

The CLI uses these defaults when generating sample configs (`vtcode init`) and when no user configuration is present.

## Validation

Every configuration loaded from disk now goes through `VTCodeConfig::validate`. The validator performs:

-   Syntax highlighting checks (minimum file size, timeout, language entries)
-   Context subsystem checks (ledger limits, token budget thresholds, curation limits)
-   Router checks (heuristic thresholds and required model identifiers)
-   Lifecycle hooks validation (matcher patterns, command syntax, timeout values)

Validation is applied both to user-provided files and the built-in defaults. Any validation error is surfaced with contextual messaging that includes the offending file path.

## Environment Variables

Environment variables such as `GEMINI_API_KEY` and `VTCode_CONFIG_PATH` still participate in runtime behavior (API key selection, workspace overrides), but they do not bypass validation—once the configuration is constructed, the same validation rules are applied.

## Lifecycle Hooks Configuration

Lifecycle hooks are configured under the `[hooks.lifecycle]` section in `vtcode.toml` and allow you to execute shell commands in response to agent events. For detailed information about hook types, configuration options, and practical examples, see the [Lifecycle Hooks Guide](../../docs/guides/lifecycle-hooks.md).

## Experimental Features

### Smart Conversation Summarization

**Status:** EXPERIMENTAL - Disabled by default

Smart summarization automatically compresses conversation history when context grows too large. This feature uses advanced algorithms for intelligent compression while preserving critical information.

**Configuration:**

```toml
[agent.smart_summarization]
enabled = false  # Experimental feature, disabled by default
min_summary_interval_secs = 30
max_concurrent_tasks = 4
min_turns_threshold = 20
token_threshold_percent = 0.6
max_turn_content_length = 2000
aggressive_compression_threshold = 15000
```

**Environment Variables** (override TOML config):

-   `VTCODE_SMART_SUMMARIZATION_ENABLED=true` - Enable the feature
-   `VTCODE_SMART_SUMMARIZATION_INTERVAL=30` - Min seconds between summarizations
-   `VTCODE_SMART_SUMMARIZATION_MAX_CONCURRENT=4` - Max concurrent tasks
-   `VTCODE_SMART_SUMMARIZATION_MAX_TURN_LENGTH=2000` - Max chars per turn
-   `VTCODE_SMART_SUMMARIZATION_AGGRESSIVE_THRESHOLD=15000` - Compression threshold

**Features:**

-   Rule-based compression with importance scoring
-   Semantic similarity detection (Jaccard)
-   Extractive summarization for long messages
-   Advanced error pattern analysis with temporal clustering
-   Comprehensive summary generation with metrics

**Warning:** This feature is experimental and may affect conversation quality. Enable only for testing long-running sessions.

## Developer Tips

-   Prefer updating the shared defaults module when adding new configuration knobs so CLI bootstrapping and serde defaults stay aligned.
-   Add focused validation routines next to the structs that own the data to keep error messages specific and maintainable.
-   Update unit tests in `vtcode-core/src/config/loader/mod.rs` when adjusting precedence rules or default values to avoid regressions.
