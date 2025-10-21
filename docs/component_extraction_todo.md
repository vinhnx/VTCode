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
- [ ] Backfill `vtcode-config` with loader-focused tests and crate-level documentation updates ahead of publishing.

## Cross-Cutting
- [x] Capture external integration prerequisites (environment variables, binary dependencies) and align them with optional feature groups.
- [x] Define shared base traits (`vtcode-commons`) for filesystem paths, telemetry, and error handling used by multiple crates.
- [x] Adopt the new `vtcode-commons` traits inside `vtcode-llm`'s configuration adapters so consumers can provide custom path and telemetry hooks.
- [x] Document reference implementations of the shared traits for downstream adopters (see `docs/vtcode_commons_reference.md`).
- [x] Adopt the shared hooks throughout `vtcode-tools` so policy and registry wiring use the common contracts.
- [x] Establish a migration checklist covering documentation, CI, and release steps for each extracted crate.
- [x] Audit dependency licenses (tree-sitter grammars, provider SDKs) to confirm compatibility before publishing.
- [x] Update project README once the first crate is ready for community feedback (linked roadmap + feedback invitation).
