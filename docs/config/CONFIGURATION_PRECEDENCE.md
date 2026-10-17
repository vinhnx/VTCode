# Configuration Precedence in VT Code

This document summarizes how VT Code discovers configuration at startup and how default values and runtime validation
interact with user-provided settings.

## Resolution Order

When the CLI starts it builds a **layer stack** and merges all present layers from lowest to highest precedence.

1. **Built-in defaults** – used only when no config layer exists
2. **System** – `/etc/vtcode/vtcode.toml` plus `XDG_CONFIG_DIRS` candidates (Unix)
3. **Legacy user** – `VTCODE_HOME/vtcode.toml` or historical `~/.vtcode/vtcode.toml`
4. **Canonical user** – the platform config directory's `vtcode.toml`
5. **Project profile** – `<workspace>/.vtcode/projects/<project>/config/vtcode.toml`
6. **Workspace fallback** – `<workspace>/.vtcode/vtcode.toml` (when distinct from root file)
7. **Workspace root** – `<workspace>/vtcode.toml`
8. **Explicit config file** – `VTCODE_CONFIG_PATH` or `--config path/to/file.toml` replaces the normal workspace-file
   layer while retaining the global layers; `--config key=value` remains the inline override form
9. **Runtime overrides** – CLI `-c/--config key=value` and explicit runtime flags (highest precedence)

Tables are deep-merged recursively. Scalars and arrays are replaced by the higher-precedence layer.

### Legacy top-level provider aliases

Bare top-level `default_provider = "…"` / `default_model = "…"` keys (in the global `config.toml` or a `vtcode.toml`
root) are honored: the loader promotes them into `[agent] provider` / `[agent] default_model` **per layer** before
merging. Normal layer precedence still applies — an explicit `[agent]` value in the same file wins over that file's own
top-level alias, and higher-precedence layers win over lower ones. Without the promotion these keys would be silently
dropped and the runtime would fall back to the compiled-in default provider.

### Explicit config file (session override)

When an explicit config file is present (`--config path` or `VTCODE_CONFIG_PATH`), the resolved path is captured once at
startup as the **session config override**. Every later configuration reload during that session — slash-command
persistence, the settings palette, live-reload watchers, and `ConfigService` reads/writes — reads from and writes to the
same explicit file instead of drifting back to the default workspace `vtcode.toml`. Relative paths and `~` are resolved
identically for both the CLI flag and the environment variable. `--config key=value` overrides and
`--model`/`--provider` remain runtime layers above all files.

Scope note: global-only operations (for example `vtcode mcp login` / `mcp logout`) always operate on the canonical user
configuration and intentionally ignore the explicit session override.

### Inline CLI overrides

Inspired by [OpenAI Codex CLI](https://github.com/openai/codex), VT Code now accepts `-c/--config key=value` overrides
directly on the command line. These overrides apply as a dedicated runtime layer above all file-based layers. Use
multiple flags to set several keys during a single run. For example:

```text
vtcode --workspace ~/repo \
  --config agent.provider="openai" \
  --config context.curation.enabled=false
```

Relative config paths passed via `--config path/to/vtcode.toml` remain supported and are resolved against the workspace
before falling back to the current working directory.

## Resetting a configuration layer

Reset clears one file layer by writing an empty TOML document to the resolved target. It does not delete the file's
parent directory, remove credentials, or change lower-precedence layers:

| Command                         | Layer cleared                                                     |
| ------------------------------- | ----------------------------------------------------------------- |
| `vtcode config reset`           | Active workspace file, or the explicit `--config path`            |
| `vtcode config reset --global`  | Canonical user configuration                                      |
| `vtcode config reset --project` | Current project's `.vtcode/projects/<project>/config/vtcode.toml` |
| `/config reset`                 | The target file shown by the settings palette, after confirmation |

`--global` and `--project` cannot be combined. The reset service validates targets against known configuration paths and
rejects symlinked or non-regular files before writing. The effective stack is reloaded and its cache is invalidated
after a successful reset. If the layer is already absent, reset is a no-op followed by the same effective-config reload.

## Live reload and invalid edits

Interactive sessions poll all active configuration sources, including user, project, workspace fallback/root, explicit
session, and theme paths. A valid change is debounced and applied to safe runtime settings. Provider/model identity
selected for the current session stays stable until a later turn or session when switching the active client would be
unsafe.

Malformed, inaccessible, or otherwise invalid edits fail closed: the last valid runtime snapshot remains active and VT
Code displays a warning. Once the file is repaired, the next reload applies it. The open settings palette refreshes its
values from the effective stack while retaining its section and selected entry when that entry still exists.

## Default Values

Layered defaults are defined in the Rust sources so the application can generate a baseline configuration and reason
about missing fields:

- **Global configuration defaults** live in `crates/codegen/vtcode-core/src/config/defaults/`
- **Syntax highlighting defaults** are centralized in `syntax_highlighting.rs` and reused by the loader and serde
- **Context and tooling defaults** remain close to their owning modules but consume the shared constants exported by the
  defaults module

The CLI uses these defaults when generating sample configs (`vtcode init`) and when no user configuration is present.

## Validation

Every effective configuration goes through `VTCodeConfig::validate`. Loader errors are now layer-attributed: malformed
layers are reported with their exact source path.

The validator performs:

- Syntax highlighting checks (minimum file size, timeout, language entries)
- Context subsystem checks (ledger limits, token budget thresholds, curation limits)
- Router checks (heuristic thresholds and required model identifiers)
- Lifecycle hooks validation (matcher patterns, command syntax, timeout values)

Validation is applied both to user-provided files and the built-in defaults. Any validation error is surfaced with
contextual messaging that includes the offending file path.

### Provider configuration trust boundary

Workspace-root `vtcode.toml`, workspace `.vtcode/vtcode.toml`, and project profiles are repository-controlled layers.
After merging the layers, the loader fails closed if a non-empty `custom_providers` value comes from one of those
layers. This also prevents repository configuration from introducing a custom provider's executable `auth.command`.

The loader likewise rejects repository-controlled values for `provider_overrides.<name>.base_url` and
`provider_overrides.<name>.api_key_env`. These fields can redirect model traffic or select a credential environment
variable, so they must come from system configuration, user configuration, an explicitly selected config file, or an
explicit runtime override. The check uses the winning field origin after the merge; `workspace.use_root_config` does not
bypass it.

Custom providers and endpoint overrides remain available in user-level configuration. Environment filtering on provider
subprocesses is an additional defense and does not make a repository-supplied authentication command safe.

Normal startup and live reload detect this specific legacy-file violation, remove only the protected repository fields
atomically, and retry the strict load. Malformed or symlinked files still fail closed; strict loader APIs keep rejecting
repository-controlled provider fields, and explicitly selected config files are never repaired.

## Environment Variables

Environment variables such as `GEMINI_API_KEY` still participate in runtime behavior (API key selection), but they do
not bypass validation. `VTCODE_CONFIG` and `VTCODE_DATA` select the canonical user config/data roots;
`VTCODE_CONFIG_PATH` selects the explicit file layer described above. Once the configuration is constructed, the same
validation rules are applied.

The canonical user directory policy, including XDG defaults, native macOS and Windows locations,
data/state/cache/runtime/executable roots, migration, and rollback behavior, is documented in the
[user data directories guide](../guides/user-data-directories.md) and the
[XDG directory specification](../protocols/XDG_DIRECTORY_SPECIFICATION.md).

## Lifecycle Hooks Configuration

Lifecycle hooks are configured under the `[hooks.lifecycle]` section in `vtcode.toml` and allow you to execute shell
commands in response to agent events. For detailed information about hook types, configuration options, and practical
examples, see the [Lifecycle Hooks Guide](../../docs/guides/lifecycle-hooks.md).

## Experimental Features

### Smart Conversation Summarization

**Status:** EXPERIMENTAL - Disabled by default

Smart summarization automatically compresses conversation history when context grows too large. This feature uses
advanced algorithms for intelligent compression while preserving critical information.

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

- `VTCODE_SMART_SUMMARIZATION_ENABLED=true` - Enable the feature
- `VTCODE_SMART_SUMMARIZATION_INTERVAL=30` - Min seconds between summarizations
- `VTCODE_SMART_SUMMARIZATION_MAX_CONCURRENT=4` - Max concurrent tasks
- `VTCODE_SMART_SUMMARIZATION_MAX_TURN_LENGTH=2000` - Max chars per turn
- `VTCODE_SMART_SUMMARIZATION_AGGRESSIVE_THRESHOLD=15000` - Compression threshold

**Features:**

- Rule-based compression with importance scoring
- Semantic similarity detection (Jaccard)
- Extractive summarization for long messages
- Advanced error pattern analysis with temporal clustering
- Comprehensive summary generation with metrics

**Warning:** This feature is experimental and may affect conversation quality. Enable only for testing long-running
sessions.

## Developer Tips

- Prefer updating the shared defaults module when adding new configuration knobs so CLI bootstrapping and serde defaults
  stay aligned.
- Add focused validation routines next to the structs that own the data to keep error messages specific and
  maintainable.
- Update unit tests in `crates/codegen/vtcode-core/src/config/loader/mod.rs` when adjusting precedence rules or default
  values to avoid regressions.
