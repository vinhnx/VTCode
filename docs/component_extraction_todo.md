# Component Extraction TODO

This list tracks actionable tasks spawned from the component extraction plan as we prototype new crates.

## `vtcode-llm`
- [x] Scaffold prototype crate that re-exports the existing LLM abstraction layer.
- [x] Document external API surface area and propose feature flags for providers, function calling, telemetry, and mocks.
- [x] Introduce a provider configuration trait to decouple from `vtcode_core::utils::dot_config`.
- [x] Gate provider implementations behind feature flags to shrink dependency footprint.
- [x] Add documentation on expected environment variables and provide mock clients for testing.

## `vtcode-tools`
- [x] Scaffold prototype crate that re-exports the tool registry and built-in tools.
- [x] Outline feature flag groups for heavyweight tools, policy wiring, telemetry, and examples.
- [x] Extract policy wiring so configuration structs live outside the crate boundary.
- [x] Make tree-sitter and heavy tooling optional via feature flags.
- [x] Publish integration examples exercising registry setup and execution in a headless context.

## `vtcode-config`
- [x] Outline extraction strategy covering module boundaries and customizable default paths.
- [x] Prototype a `ConfigDefaultsProvider` trait that maps `WorkspacePaths` into path and theme defaults used by the loader.
- [x] Rework bootstrap helpers to accept injected defaults/providers instead of constructing `.vtcode` directories directly.
- [x] Document migration steps for downstream consumers adopting the new crate structure.
- [x] Scaffold the standalone crate layout and migrate defaults/bootstrap helpers while leaving compatibility re-exports in `vtcode-core`.
- [x] Move the `VTCodeConfig` loader and `ConfigManager` into the new crate while maintaining temporary re-exports in `vtcode-core`.
- [x] Backfill `vtcode-config` with loader-focused tests and crate-level documentation updates ahead of publishing.
- [x] Gate bootstrap helpers behind an optional crate feature so parsing-only consumers can disable filesystem scaffolding.
- [x] Expose JSON Schema export helpers under an optional feature flag for downstream documentation tooling.
- [x] Publish a runnable example that demonstrates installing custom defaults providers when loading configuration outside `.vtcode` directories.


## `vtcode-markdown-store`
- [x] Scaffold the standalone crate exporting markdown storage, key-value, project, and cache helpers under optional feature flags.
- [x] Add constructors that accept custom project roots so `.vtcode` directory assumptions are no longer hardcoded.
- [x] Document crate usage patterns and feature gates for downstream adopters before publishing an initial release. See `docs/vtcode_markdown_store.md` for examples and feature guidance.
- [x] Harden storage operations with cross-platform file locks and atomic rewrites so concurrent agents can safely share the same root.


## `vtcode-indexer`
- [x] Scaffold the standalone crate and migrate `SimpleIndexer` with configurable index directories and hidden-directory controls.
- [x] Introduce a pluggable storage trait so index summaries can target Markdown, JSON, or external services.
- [x] Add traversal filtering hooks (ignore globs, binary detection) ahead of directory walks for better downstream tuning.
- [x] Publish an example demonstrating indexing/search flows outside VTCode's runtime.


## `vtcode-bash-runner`
- [x] Outline extraction strategy covering cross-platform execution, command abstraction, and feature flag groupings (see `docs/vtcode_bash_runner_extraction.md`).
- [x] Introduce a trait-driven command executor so backends can swap between system processes, pure-Rust shims, or dry-run logging.
- [x] Add platform-aware shims and shell selection to support Windows (PowerShell) and constrained environments via feature flags.
- [x] Parameterize safety policies (allowed commands, workspace guards) to integrate with `vtcode-commons` without hardcoded paths.
- [x] Publish documentation and examples demonstrating cross-platform usage and dry-run testing hooks ahead of crate publication.

## `vtcode-exec-events`
- [x] Outline extraction strategy covering event schema versioning, telemetry integration points, and feature gating (see `docs/vtcode_exec_events_extraction.md`).
- [x] Scaffold the standalone crate with the existing event enums and serialization helpers.
- [ ] Document event semantics, versioning policy, and consumer integration patterns.
- [ ] Publish examples or tests that demonstrate emitting and capturing execution events outside VTCode's runtime.



## Cross-Cutting
- [x] Capture external integration prerequisites (environment variables, binary dependencies) and align them with optional feature groups.
- [x] Define shared base traits (`vtcode-commons`) for filesystem paths, telemetry, and error handling used by multiple crates.
- [x] Adopt the new `vtcode-commons` traits inside `vtcode-llm`'s configuration adapters so consumers can provide custom path and telemetry hooks.
- [x] Document reference implementations of the shared traits for downstream adopters (see `docs/vtcode_commons_reference.md`).
- [x] Adopt the shared hooks throughout `vtcode-tools` so policy and registry wiring use the common contracts.
- [x] Establish a migration checklist covering documentation, CI, and release steps for each extracted crate.
- [x] Audit dependency licenses (tree-sitter grammars, provider SDKs) to confirm compatibility before publishing.
- [x] Update project README once the first crate is ready for community feedback (linked roadmap + feedback invitation).
