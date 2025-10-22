# VTCode Component Extraction Plan

## Purpose
This document captures the results of a quick architectural survey of VTCode with the goal of identifying subsystems that could be extracted into standalone crates for open-source reuse. The focus is on components that already expose clear boundaries, have minimal coupling to the TUI, and would benefit the wider Rust agent ecosystem.

## Evaluation Approach
- Reviewed `vtcode-core` modules to map existing responsibilities and public APIs.
- Prioritized subsystems that already encapsulate well-defined responsibilities and expose reusable traits or data structures.
- Noted required decoupling work (configuration, logging, storage paths) needed before a clean extraction.

## Candidate Crates Overview
| Candidate crate | Source modules | Core capability | Key dependencies | Reuse potential |
| --- | --- | --- | --- | --- |
| `vtcode-llm` | `vtcode-core/src/llm` | Unified async client abstraction over Gemini, OpenAI, Anthropic, xAI, DeepSeek, and Z.AI providers (streaming, function calling, retries). | `anyhow`, `futures`, provider SDKs, config loader. | High – provides a ready-made multi-provider facade with streaming and function-call support.【F:vtcode-core/src/llm/mod.rs†L1-L160】|
| `vtcode-tools` | `vtcode-core/src/tools` | Registry-driven tool execution framework with safety policies, PTY integration, AST/grep search utilities. | `async_trait`, `serde_json`, `tokio`, tree-sitter crates. | High – modular tool runtime could power other agents or CLI automation surfaces.【F:vtcode-core/src/tools/mod.rs†L1-L160】|
| `vtcode-commons` | New shared crate | Foundational traits for workspace paths, telemetry sinks, and error reporting shared across extracted crates. | `anyhow`. | High – keeps downstream integrations consistent without depending on VTCode's binary or storage defaults. |
| `vtcode-config` | `vtcode-core/src/config/loader/mod.rs` plus `config` submodules | Typed loader for TOML configuration with defaults covering agent, tools, security, UI, MCP/ACP, telemetry, syntax highlighting. | `serde`, `toml`, `anyhow`. | Medium – valuable for other terminal agents; requires separating VTCode-specific defaults and paths.【F:vtcode-core/src/config/loader/mod.rs†L1-L200】|
| `vtcode-markdown-store` | `vtcode-core/src/markdown_storage.rs`, `project.rs` | Markdown-backed storage, project management, simple cache/kv utilities. | `serde_json`, `serde_yaml`, `indexmap`. | Medium – lightweight alternative to database-backed state useful for offline tooling.【F:vtcode-core/src/markdown_storage.rs†L1-L200】【F:vtcode-core/src/project.rs†L1-L200】|
| `vtcode-indexer` | `vtcode-core/src/simple_indexer.rs` | Regex-powered file indexer with on-disk markdown snapshots and search helpers. | `regex`, filesystem APIs. | Medium – simple workspace index ideal for scripting or other agents.【F:vtcode-core/src/simple_indexer.rs†L1-L200】|
| `vtcode-bash-runner` | `vtcode-core/src/bash_runner.rs` | Safe wrapper around common shell commands (cd/ls/mkdir/rm/cp/mv/grep) with contextual error handling. | `std::process`, `anyhow`. | Low/Medium – helpful for sandboxed automation or testing harnesses.【F:vtcode-core/src/bash_runner.rs†L1-L200】|
| `vtcode-exec-events` | `vtcode-core/src/exec/events.rs` | Structured event schema for autonomous execution telemetry (thread lifecycle, command/file updates, error reporting). | `serde`. | Medium – reusable telemetry schema for orchestrating multi-step agent runs.【F:vtcode-core/src/exec/events.rs†L1-L200】|

## Detailed Extraction Notes

### `vtcode-llm`
**What it offers:**
- Common `AnyClient` interface with helper constructors for every supported provider, including streaming and function calling flows.【F:vtcode-core/src/llm/mod.rs†L1-L160】
- Consistent error taxonomy for auth, rate limits, provider, and network failures.

**Decoupling tasks:**
- Extract provider-specific clients behind feature flags to keep the crate lightweight.
- Replace direct references to `vtcode_core::utils::dot_config::ProviderConfigs` with a provider-agnostic config trait.
- Document environment variable expectations and provide mock implementations for testing.

### `vtcode-tools`
**What it offers:**
- Registry pattern (`ToolRegistry`, `ToolRegistration`) with async execution and serde-based parameter schemas.【F:vtcode-core/src/tools/mod.rs†L18-L159】
- Rich catalogue of built-in tools (bash, AST-grep, srgn, curl, planners) bundled behind module boundaries.
- Safety policies: workspace path validation, command allow/deny lists, execution logging.【F:vtcode-core/src/tools/mod.rs†L42-L85】

**Decoupling tasks:**
- Isolate tree-sitter parsers and heavy dependencies into optional features.
- Move VTCode-specific policy wiring (config structs, telemetry hooks) into adapters so the crate exports clean traits.
- Provide integration examples demonstrating registry setup and tool execution from a headless context.

### `vtcode-commons`
**What it offers:**
- Shared contracts for resolving workspace paths, emitting telemetry, and reporting recoverable errors without depending on the CLI layer.
- Default no-op implementations to unblock prototypes that do not yet integrate with observability stacks.

**Decoupling tasks:**
- Wire the new traits into `vtcode-llm` and `vtcode-tools` so consumers can optionally provide their own implementations.
- Audit `vtcode-core` to identify additional utilities (path normalization, logging adapters) that belong in the shared crate.
- Document recommended trait implementations and patterns for external adopters.

### `vtcode-config`
**What it offers:**
- A single `VTCodeConfig` struct with defaults covering agent tuning, tool permissions, PTY behavior, telemetry, syntax highlighting, automation, MCP/ACP, and caching.【F:vtcode-core/src/config/loader/mod.rs†L88-L167】
- Bootstrap helpers that generate config files and `.gitignore` entries for new workspaces.【F:vtcode-core/src/config/loader/mod.rs†L169-L200】

**Decoupling tasks:**
- Split the monolithic config into layered modules (`agent`, `tools`, `telemetry`, etc.) so downstream projects can pick subsets.
- Allow callers to inject their own default paths and theme lists instead of writing into `.vtcode` directories.
- Publish serde schemas and conversion utilities for forward compatibility.

### `vtcode-markdown-store`
**What it offers:**
- `MarkdownStorage` abstraction that serializes structs into Markdown with JSON/YAML blocks and reloads them via serde.【F:vtcode-core/src/markdown_storage.rs†L13-L155】
- Higher-level utilities: `SimpleKVStorage`, `ProjectStorage`, and `ProjectData` for human-readable project metadata.【F:vtcode-core/src/markdown_storage.rs†L158-L200】
- `SimpleProjectManager` wrapping storage with helper methods to create/list/update projects and locate per-project directories.【F:vtcode-core/src/project.rs†L10-L140】
- `SimpleCache` for filesystem-backed caching with contextual errors.【F:vtcode-core/src/project.rs†L142-L200】

**Decoupling tasks:**
- Extract `.vtcode` directory assumptions; allow callers to pass storage paths or use temp dirs.
- Add file locking or atomic writes if concurrent agents will reuse the crate.
- Expose serde feature gates for consumers who only need KV or project metadata pieces.

### `vtcode-indexer`
**What it offers:**
- Pure-Rust file indexer with metadata hashing, language detection via extension, and regex-powered search/find helpers.【F:vtcode-core/src/simple_indexer.rs†L16-L200】
- Stores snapshots as Markdown alongside an in-memory cache, making results auditable and git-friendly.

**Decoupling tasks:**
- Replace Markdown persistence with pluggable storage (trait) so consumers can target SQLite, S3, etc.
- Add filtering hooks (ignore globs, binary detection) before walking directories.
- Publish CLI examples showing indexing and search flows to demonstrate reuse outside VTCode.

### `vtcode-bash-runner`
**What it offers:**
- Friendly wrapper for common shell actions (cd/ls/pwd/mkdir/rm/cp/mv/grep) with canonicalized paths and rich error messages.【F:vtcode-core/src/bash_runner.rs†L11-L200】

**Decoupling tasks:**
- Harden for cross-platform support (Windows fallback to `cmd` / PowerShell or rely on busybox).
- Inject command execution strategy (pure Rust alternatives) for sandboxed environments.
- Provide dry-run mode to log commands without executing them.

### `vtcode-exec-events`
**What it offers:**
- Serde-tagged enums describing thread lifecycle, command execution, file change summaries, and token usage metrics.【F:vtcode-core/src/exec/events.rs†L3-L123】

**Decoupling tasks:**
- Document event semantics and versioning policy so downstream consumers can rely on backward compatibility.
- Offer feature flags for optional payloads (e.g., exclude file diffs to reduce payload size).

## Cross-Cutting Recommendations
- Establish a shared `vtcode-commons` crate for base error types, path utilities, and configuration traits referenced by multiple extracted crates.
- Adopt semantic versioning per crate and generate docs via `cargo doc` before publishing to crates.io.
- Ensure all extracted crates include focused integration tests and, where applicable, minimal examples under `examples/` demonstrating standalone use.

## Progress Update
- Scaffolded prototype crates `vtcode-llm` and `vtcode-tools` that re-export the existing LLM layer and tool registry for experimentation while decoupling work proceeds.
- Added `docs/component_extraction_todo.md` to track follow-up tasks for configuration traits, feature flags, documentation, and cross-cutting concerns.
- Mapped external integration requirements for prospective consumers and drafted a feature flag matrix covering providers, heavyweight tools, and optional telemetry so the prototype crates can slim dependencies while staying configurable.
- Introduced initial feature gates in both prototype crates so downstream consumers can opt into provider-specific exports, tool categories, telemetry helpers, and function-calling utilities without pulling the entire workspace surface by default.
- Published dedicated environment configuration guidance for `vtcode-llm`, documenting provider API keys, configuration helper patterns, and the new mock client utilities for deterministic tests.
- Documented how `vtcode-tools` adopters can supply their own policy storage by exposing constructors that accept custom `ToolPolicyManager` instances and publishing the `docs/vtcode_tools_policy.md` guide.
- Published a headless integration example (`vtcode-tools/examples/headless_registry.rs`) that wires a custom policy manager into the registry while keeping crate features slimmed down to the `policies` toggle.
- Introduced the `vtcode-commons` crate to host shared path, telemetry, and error-reporting traits so future extractions avoid reimplementing the same contracts.
- Adopted the shared `WorkspacePaths`, telemetry sink, and error-reporting hooks inside `vtcode-llm`'s provider configuration adapters so consumers can resolve prompt caches and surface failures without relying on VTCode defaults.
- Published reference adapters for `vtcode-commons` in `docs/vtcode_commons_reference.md`, giving downstream adopters turnkey implementations of the shared traits.
- Outlined the extraction strategy for `vtcode-config`, defining how to decompose the loader into domain modules, parameterize defaults, and expose bootstrap helpers that respect caller-provided workspace paths (see `docs/vtcode_config_extraction.md`).
- Prototyped a workspace-aware `ConfigDefaultsProvider` that maps `WorkspacePaths` into loader defaults, allowing custom config directories and syntax presets without relying on `.vtcode`.
- Refactored the bootstrap helpers to honor the installed `ConfigDefaultsProvider`, letting workspaces and home directories be scaffolded without hardcoding `.vtcode` paths.
- Authored a migration guide that walks downstream consumers through adopting the standalone `vtcode-config` crate, defaults provider, and bootstrap helpers.
- Scaffolded the standalone `vtcode-config` crate, moving the defaults provider and bootstrap path helpers into it while re-exporting them through `vtcode-core` for compatibility.
- Migrated the `VTCodeConfig` loader and `ConfigManager` into the new crate, rewiring `vtcode-core` to act as a thin re-export layer and relocating the OpenRouter metadata build script to `vtcode-config`.
- Authored end-user documentation for `vtcode-markdown-store` covering feature flags and usage examples (`docs/vtcode_markdown_store.md`) so downstream adopters understand how to integrate the crate.
- Hardened `vtcode-markdown-store` storage primitives with cross-platform file locks and synced writes so concurrent agents can safely share the markdown-backed state.
- Extracted the `vtcode-indexer` crate, migrating `SimpleIndexer` with configurable index roots and hidden-directory controls to decouple it from VTCode's `.vtcode` layout assumptions.
- Documented the new `IndexStorage` and `TraversalFilter` contracts in `docs/vtcode_indexer.md` and shipped a runnable example showcasing custom persistence and filters.
- Outlined the extraction strategy for `vtcode-bash-runner`, capturing cross-platform command abstractions, feature flag groupings, and a testing approach ahead of migrating the module (see `docs/vtcode_bash_runner_extraction.md`).
- Scaffolded the standalone `vtcode-bash-runner` crate with a trait-driven executor, shell-family shims for Unix and Windows targets, and workspace-aware policy hooks reusable across applications.
- Documented the new `vtcode-bash-runner` crate, covering shell selection, policy hooks, and a dry-run example for CI environments.
- Outlined the extraction strategy for `vtcode-exec-events`, covering schema versioning, telemetry adapters, and feature gating ahead of crate scaffolding (see `docs/vtcode_exec_events_extraction.md`).
- Scaffolded the standalone `vtcode-exec-events` crate, moving the telemetry event schema behind a reusable dependency and re-exporting it through `vtcode-core` for compatibility.

- **Next milestone:** document event semantics, versioning guarantees, and integration patterns for downstream consumers ahead of example coverage.

## Feature Flag Strategy

### `vtcode-llm`
- **Core API surface**: Keep `AnyClient` and shared request/response types in the default build.
- **Provider features**:
  - `openai`, `anthropic`, `google`, `xai`, `deepseek`, `zai` – each toggles the concrete client module and corresponding SDK dependency.
  - `mock` – enables the deterministic fake provider used in integration tests and documentation snippets.
- **Streaming/function-calling**: Expose a `functions` feature that wires in shared schema helpers and executor glue for function calls across providers.
- **Telemetry hooks**: Gate optional tracing/metrics emitters behind a `telemetry` feature so downstream projects can integrate observability without extra deps by default.
- **Configuration**: Introduce a `ProviderConfig` trait that the workspace implements; consumers can supply their own config sources while retaining typed secrets and retry policies.

### `vtcode-tools`
- **Registry core**: Keep registry types and lightweight tools (`fs_inspect`, `echo`, `metadata`) enabled in the default feature set.
- **Heavyweight tools**:
  - `bash` – shell execution and sandboxing utilities.
  - `search` – AST-grep, ripgrep, and srgn integrations that require tree-sitter crates.
  - `net` – curl/httpie style tooling that pulls in `reqwest` and TLS stacks.
  - `planner` – planning/analysis helpers that depend on LLM streaming callbacks.
- **Telemetry and policies**: Provide a `policies` feature to re-export VTCode’s policy wiring (path guards, command allowlists) while letting consumers define their own implementations when the feature is disabled.
- **Examples**: An `examples` feature builds the headless demonstration binaries that exercise registry registration and execution.

### External API Requirements
- Documented environment variables per provider (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc.) and mapped their usage to the `ProviderConfig` trait so consumers can plug in vault-backed secrets.
- Identified tool prerequisites (tree-sitter grammars, `rg`, `srgn`, `curl`) and grouped them under optional features to avoid surprising runtime dependencies.
- Captured telemetry expectations (structured events, token accounting) to ensure downstream adopters can opt in via feature flags without carrying unused schema code.

## Migration Checklists
- Each extracted crate follows a consistent migration checklist so that publishing to crates.io and integrating back into VTCode remains predictable.

### `vtcode-llm`
1. **Testing**:
   - Run `cargo fmt`, `cargo clippy --all-targets --all-features`, and `cargo nextest run --all-features` within the crate.
   - Execute provider-specific smoke tests with mock configs when optional features are enabled.
2. **Documentation**:
   - Update crate-level README with supported providers, feature flag matrix, and configuration examples.
   - Regenerate API docs via `cargo doc --no-deps --all-features` and publish to docs.rs after release.
3. **CI & Release**:
   - Ensure workspace CI runs the crate matrix (core + each provider feature).
   - Tag releases with `vtcode-llm-vX.Y.Z`, update changelog entries, and publish using `cargo release`.

### `vtcode-tools`
1. **Testing**:
   - Run `cargo fmt`, `cargo clippy --all-targets --all-features`, and `cargo nextest run --all-features`.
   - Validate integration examples for headless execution paths under the `examples` feature.
2. **Documentation**:
   - Refresh README sections covering available tool categories, feature flags, and safety policies.
   - Document external binary requirements per feature (tree-sitter grammars, `rg`, `srgn`, `curl`).
3. **CI & Release**:
   - Extend CI to cover feature permutations (default, `bash`, `search`, `net`, `planner`, `policies`).
   - Tag releases with `vtcode-tools-vX.Y.Z`, update the changelog, and coordinate workspace dependency bumps.

## License Compatibility Review
- Conducted a targeted license audit for the dependencies bundled (or planned) with the prototype crates to ensure compatibility with VTCode's MIT licensing strategy.
- `vtcode-tools` heavy dependencies (tree-sitter grammars, AST/grep helpers) all publish under the MIT license, which is permissive and compatible for redistribution.
- Core runtime crates that power both `vtcode-llm` and `vtcode-tools` (`tokio`, `reqwest`, `serde`, `serde_json`, `anyhow`) are dual-licensed MIT/Apache-2.0, aligning with VTCode's licensing.
- No provider-specific Rust SDKs are currently vendored—requests are issued via `reqwest`—so there are no additional license obligations beyond HTTP API terms of service. Document future additions in the TODO tracker to keep this audit current.

## Next Steps
1. ✅ Implement the `ProviderConfig` trait within `vtcode-llm` and refactor the existing providers to depend on it instead of workspace-specific structures.
   - Added a provider-agnostic trait and conversion helpers in the `vtcode-llm` crate so external consumers can supply configuration without relying on `vtcode_core::utils::dot_config`.
   - Implemented adapters for the existing dot-config structs and the internal factory configuration, plus an owned builder to support tests and integration examples.
2. ✅ Wire up feature gates in both prototype crates, introducing cfg guards and updating Cargo manifests to reflect the new optional dependencies.
   - Added provider, function-calling, and telemetry toggles to `vtcode-llm`, exposing provider modules only when the matching feature is enabled.
   - Gated heavy tool categories (`bash`, `search`, `net`, `planner`) and policy re-exports in `vtcode-tools`, letting consumers keep lightweight registry/trait types by default.
3. ✅ Define a migration checklist (tests, documentation, CI) for each crate to keep the releases consistent.
   - Documented migration checklists for `vtcode-llm` and `vtcode-tools`, covering testing expectations, documentation deliverables, and release automation hooks so crate publishing follows a predictable path.
4. ✅ Evaluate licensing compatibility of bundled dependencies (tree-sitter grammars, provider SDKs) before publishing.
   - Audited licenses for the tree-sitter grammar crates and core runtime dependencies; all are MIT or MIT/Apache-2.0, compatible with VTCode's licensing.
   - Confirmed we currently depend on HTTP client crates (not provider SDKs), so compliance hinges on external API terms until SDKs are added.
5. ✅ Communicate roadmap in the main README and invite community feedback once the first crate lands on crates.io.
   - Added a "Component Extraction Roadmap" section to the root README describing the goals for `vtcode-llm` and `vtcode-tools`, linking back to this plan and the shared TODO tracker.
   - Documented how community members can provide feedback ahead of the first crates.io publish so expectations stay aligned while remaining decoupling tasks land.
6. ✅ Publish environment variable documentation and mock client guidance for `vtcode-llm` so adopters understand configuration requirements before the crate is released.
   - Authored `docs/vtcode_llm_environment.md` covering provider API keys, configuration trait usage patterns, and examples for combining environment variables with the owned adapter helpers.
   - Added a `mock` feature module exposing `StaticResponseClient` so downstream tests can queue deterministic responses without reaching real providers.
7. ✅ Document how `vtcode-tools` consumers can replace the default policy wiring so configuration structs live outside the crate boundary.
   - Added `ToolPolicyManager::new_with_config_path` and custom `ToolRegistry` constructors so downstream projects can inject pre-configured policy managers without touching VTCode's `.vtcode` directory.
   - Authored `docs/vtcode_tools_policy.md` with step-by-step guidance on enabling the `policies` feature, selecting a storage path, and wiring the custom manager into the registry.
8. ✅ Publish headless integration examples demonstrating `vtcode-tools` usage with custom policy storage and feature flags for lightweight adoption.
   - Added the `headless_registry` example to the `vtcode-tools` crate showcasing how to register custom tools, persist policies outside `~/.vtcode`, and run with only the `policies` feature enabled.
   - Extended the policy customization guide with commands for running the example so downstream users can replicate the workflow.
9. ✅ Define shared base traits (`vtcode-commons`) for filesystem paths, telemetry, and error handling used by multiple crates.
   - Published the `vtcode-commons` crate with shared contracts for workspace path resolution, telemetry sinks, and error reporting, and re-exported them from the prototype crates.
   - Added default no-op implementations so adopters can opt-in gradually without wiring observability or storage upfront.
10. ✅ Document reference implementations of the shared traits for downstream adopters.
   - Added memory-backed telemetry and error reporters plus a static path resolver to `vtcode-commons`, providing ready-to-use scaffolding for tests and prototypes.
   - Documented how to use the new helpers in `docs/vtcode_commons_reference.md`, guiding external consumers through drop-in integration steps.
11. ✅ Adopt the new `vtcode-commons` traits across the remaining `vtcode-tools` entry points so registry construction and policy wiring stay decoupled from VTCode defaults.
   - Introduced a `RegistryBuilder` helper that consumes `WorkspacePaths`, telemetry, and error-reporting hooks from `vtcode-commons`, ensuring policy files resolve to caller-controlled directories.
   - Updated the headless integration example to exercise the new builder so downstream adopters can follow a concrete workspace-aware setup when wiring the registry.
12. ✅ Scaffolded the `vtcode-markdown-store` crate and migrated markdown, project, and cache helpers so storage utilities can evolve independently of `vtcode-core`.
   - Ported the markdown storage and simple project manager modules into the new crate with feature flags for the KV, project, and cache layers.
   - Added customizable project roots so `.vtcode` directory assumptions can be overridden when embedding the crate in other tools, and wired `vtcode-core` to re-export the new crate for compatibility.
13. ✅ Introduced pluggable storage and traversal hooks for `vtcode-indexer` so adopters can integrate custom persistence and workspace policies.
   - Added the `IndexStorage` trait with a default Markdown implementation and rewired the indexer to depend on the trait for all persistence.
   - Exposed a `TraversalFilter` hook that builds on the existing configuration but allows external callers to opt into bespoke directory and file selection logic.
   - Extended the unit test suite with in-memory storage and custom filter fixtures to illustrate usage of the new extension points.
14. ✅ Documented the `vtcode-exec-events` schema and integration touchpoints for downstream telemetry pipelines.
   - Authored `docs/vtcode_exec_events.md` covering event categories, versioning guarantees, and how to attach sinks via the agent runner.
   - Called out additive evolution guidelines so existing consumers can upgrade without deserialization breaks.
15. ☐ Publish runnable examples that showcase emitting and capturing execution events outside the VTCode runtime.
   - Create a headless recorder binary that forwards events to stdout or a file for quick validation.
   - Provide a streaming example (e.g., WebSocket or message bus forwarder) so adopters can wire telemetry into observability stacks.
