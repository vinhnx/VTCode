# `vtcode-config` Extraction Strategy

## Goals
- Publish a standalone crate that loads and validates VTCode-compatible configuration without depending on the TUI workspace layout.
- Split the monolithic `VTCodeConfig` loader into domain-focused modules so downstream projects can include only the sections they need.
- Allow callers to override filesystem defaults (config directory, cache roots, theme bundles) while reusing the existing serde structures.

## Current State Snapshot
- `VTCodeConfig` lives in `vtcode-core/src/config/loader/mod.rs` with a single struct covering agent, tools, security, UI, PTY, telemetry, syntax highlighting, automation, router, MCP, and ACP settings.
- Default values now live in `vtcode-config/src/defaults` (re-exported through `vtcode-core`), continuing to include baked-in paths like `.vtcode/context` and theme caches until further decoupled.
- Bootstrap helpers (`VTCodeConfig::bootstrap_project` and `bootstrap_project_with_options`) delegate directory selection to the installed defaults provider while continuing to default to `.vtcode` paths via the bundled adapter.
- Bootstrap path selection utilities (`determine_bootstrap_targets`, parent-dir creation, and gitignore helpers) were moved into `vtcode-config/src/loader/bootstrap.rs` so downstream adopters can use them without depending on `vtcode-core` internals.

## Proposed Crate Layout
```
vtcode-config/
├── src/
│   ├── lib.rs                  // re-exports the high-level `ConfigLoader`
│   ├── loader.rs               // parses files, merges overrides, drives validation
│   ├── defaults/
│   │   ├── mod.rs              // shared defaults API
│   │   └── paths.rs            // adapter for workspace-aware paths/themes
│   ├── domains/
│   │   ├── agent.rs
│   │   ├── tools.rs
│   │   ├── security.rs
│   │   ├── ui.rs
│   │   ├── automation.rs
│   │   ├── telemetry.rs
│   │   ├── syntax.rs
│   │   ├── router.rs
│   │   ├── mcp.rs
│   │   └── acp.rs
│   ├── bootstrap.rs            // workspace scaffolding + gitignore management
│   └── schema.rs               // optional serde schema export helpers
└── examples/
    └── minimal.rs              // loads config with injected defaults
```
- The `domains/` directory re-exports the existing structs from `vtcode-core/src/config` to avoid rewriting serde definitions. Each module stays feature-gated so adopters can choose subsets.
- `bootstrap.rs` isolates project scaffolding logic so the crate can expose CLI helpers without forcing downstream consumers to depend on VTCode’s project manager.

## Customizable Defaults Strategy
1. Introduce a `ConfigDefaultsProvider` trait that supplies filesystem roots and theme bundles:
   ```rust
   pub trait ConfigDefaultsProvider {
       fn config_dir(&self) -> &Path;
       fn cache_dir(&self) -> &Path;
       fn syntax_theme(&self) -> &str;
       fn syntax_languages(&self) -> &[String];
       fn prompt_cache_dir(&self) -> &Path;
   }
   ```
2. Provide an implementation that wraps `vtcode-commons::paths::WorkspacePaths` so VTCode reuses its current behavior without code duplication.
3. Update the defaults module to replace static constants with functions that read from the provider. The provider instance travels through the loader and bootstrap APIs.
4. Expose a builder on the `ConfigLoader` that accepts the provider and optional overrides (e.g., telemetry sinks, theme registries).
5. For callers who do not care about filesystem defaults, ship an in-memory `NoopDefaults` implementation that mimics today’s values.

**Status:** `vtcode-config/src/defaults/provider.rs` defines the `ConfigDefaultsProvider` trait alongside a `WorkspacePathsDefaults` adapter. `ConfigManager::load_from_workspace` (still hosted in `vtcode-core`) consumes the provider to resolve workspace and home search paths, and syntax highlighting defaults now flow through the provider API via the new crate re-export.

## Bootstrap Flow Updates
- **Completed:** Refactor `VTCodeConfig::bootstrap_project` so it accepts a `ConfigDefaultsProvider`, enabling callers to inject workspace-aware defaults.
- **Completed:** Move path construction responsibilities into `vtcode-config::loader::bootstrap`, exposing helper functions that `vtcode-core` now consumes via re-exports.
- **Upcoming:** Extract the remaining loader logic (`ConfigManager`, serde helpers) into the crate with compatibility re-exports for VTCode.
- **Upcoming:** Allow downstream consumers to disable bootstrap entirely via a feature flag when they only need parsing/validation.

## Migration Plan
1. **Internal refactor:** introduce the trait and builder within the monorepo, updating existing call sites to pass `WorkspacePaths`.
2. **Crate split:** move loader/default/bootstrap modules into `vtcode-config`, leaving type definitions in place under `vtcode-core` until dependents migrate.
3. **Documentation:** add migration notes covering trait implementations, new feature flags, and examples for headless services.
4. **Release prep:** publish serde schema helpers (optional) and ensure `cargo doc` highlights the new extension points.

**Status:** Steps 1–3 are complete, the new [`vtcode_config_migration.md`](./vtcode_config_migration.md) guide captures the documentation milestone, and the crate split is underway with defaults/bootstrap helpers now hosted in `vtcode-config`. The next phase focuses on migrating the loader and manager types.

## Dependencies & Feature Flags
- Hard dependencies: `serde`, `serde_json`, `toml`, `anyhow`.
- Optional features:
  - `bootstrap` (default on) for filesystem scaffolding utilities.
  - `schema` to export JSON Schema definitions via `schemars`.
  - `vtcode-commons` adapter to enable `WorkspacePaths` integration without requiring the main application.

## Open Questions
- How should we expose theme bundles so terminal and web consumers can register custom syntax highlighting packages?
- Do we need an async interface for reading remote configuration sources (e.g., HTTP or secrets managers)?
- Should prompt cache defaults live alongside other storage defaults or move into a dedicated crate?
