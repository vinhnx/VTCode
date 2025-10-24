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
- [x] Implement the strategy's optional executors (`pure-rust`, `dry-run`) and telemetry bridge so downstream adopters can toggle features before publishing the crate.【F:vtcode-bash-runner/Cargo.toml†L1-L40】【F:vtcode-bash-runner/src/executor.rs†L1-L470】【F:docs/vtcode_bash_runner.md†L1-L120】

## `vtcode-exec-events`
- [x] Outline extraction strategy covering event schema versioning, telemetry integration points, and feature gating (see `docs/vtcode_exec_events_extraction.md`).
- [x] Scaffold the standalone crate with the existing event enums and serialization helpers.
- [x] Document event semantics, versioning policy, and consumer integration patterns.
- [x] Publish examples or tests that demonstrate emitting and capturing execution events outside VTCode's runtime.
- [x] Add schema metadata, emitter traits/adapters, JSON helpers, and schema export support to satisfy the extraction strategy.

## Release readiness
- [x] Draft a consolidated release plan for the extracted crates covering version alignment, changelog updates, and publication checklists (see `docs/component_release_plan.md`).

## Release execution
- [x] Align extracted crate versions to `0.1.0`, enable publishing metadata, and record the changes in the changelog.
  - Normalized the `Cargo.toml` manifests for `vtcode-commons`, `vtcode-markdown-store`, `vtcode-indexer`, `vtcode-bash-runner`, and `vtcode-exec-events` to `0.1.0` so the release tags match the plan.
- [x] Run `cargo publish --dry-run -p <crate>` for each extracted crate to validate manifests before release (revisit `vtcode-bash-runner` once `vtcode-commons` is published to crates.io).
- [x] Schedule the sequential publishes, tag pushes, and dependency updates outlined in the release plan.
- [ ] Execute the sequential publishes, push tags, rerun the `vtcode-bash-runner` dry run after releasing `vtcode-commons`, and merge the dependency bump PRs to finish the extraction effort.
    - [x] Ensure the release automation enforces the fmt/clippy/nextest validation suite before publishing (falls back to `cargo test` when `cargo-nextest` is unavailable).
    - New helper script `scripts/publish_extracted_crates.sh` automates the release order with optional dry-run coverage; use it when the release window opens.
    - [x] Harden the release script dry-run path so rehearsals avoid creating tags or mutating the lockfile, and document the behavior in the release plan.
    - [x] Regenerate crate API docs as part of the release automation by default while allowing skips for rehearsals.



## Cross-Cutting
- [x] Capture external integration prerequisites (environment variables, binary dependencies) and align them with optional feature groups.
- [x] Define shared base traits (`vtcode-commons`) for filesystem paths, telemetry, and error handling used by multiple crates.
- [x] Adopt the new `vtcode-commons` traits inside `vtcode-llm`'s configuration adapters so consumers can provide custom path and telemetry hooks.
- [x] Document reference implementations of the shared traits for downstream adopters (see `docs/vtcode_commons_reference.md`).
- [x] Adopt the shared hooks throughout `vtcode-tools` so policy and registry wiring use the common contracts.
- [x] Establish a migration checklist covering documentation, CI, and release steps for each extracted crate.
- [x] Audit dependency licenses (tree-sitter grammars, provider SDKs) to confirm compatibility before publishing.
- [x] Update project README once the first crate is ready for community feedback (linked roadmap + feedback invitation).
