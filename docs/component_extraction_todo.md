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
- [ ] Extract policy wiring so configuration structs live outside the crate boundary.
- [x] Make tree-sitter and heavy tooling optional via feature flags.
- [ ] Publish integration examples exercising registry setup and execution in a headless context.

## Cross-Cutting
- [x] Capture external integration prerequisites (environment variables, binary dependencies) and align them with optional feature groups.
- [ ] Define shared base traits (`vtcode-commons`) for filesystem paths, telemetry, and error handling used by multiple crates.
- [x] Establish a migration checklist covering documentation, CI, and release steps for each extracted crate.
- [x] Audit dependency licenses (tree-sitter grammars, provider SDKs) to confirm compatibility before publishing.
- [x] Update project README once the first crate is ready for community feedback (linked roadmap + feedback invitation).
