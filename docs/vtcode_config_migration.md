# Migrating to the `vtcode-config` Crate

This guide walks through updating existing VTCode integrations to rely on the standalone `vtcode-config` crate. It focuses on consumers that previously accessed configuration helpers directly from `vtcode-core` and now need to opt into the configurable defaults provider and crate feature flags introduced during the extraction effort.

## Audience & Prerequisites
- You currently depend on `vtcode-core` for loading or bootstrapping VTCode configuration files.
- Your project can add the `vtcode-config` crate as a dependency (workspace members can use a path dependency during the transition).
- You are ready to adopt the `ConfigDefaultsProvider` trait so configuration defaults resolve from your workspace or service environment instead of `.vtcode` paths.

## Migration Checklist

### 1. Add the Crate Dependency
- **Workspace consumers:** add `vtcode-config = { path = "../vtcode-config", features = ["bootstrap", "vtcode-commons"] }` to the member using the loader.
- **External adopters:** depend on the published crate (version TBD) and enable the feature flags you need:
  - `bootstrap` (default) for filesystem scaffolding helpers.
  - `schema` when exporting JSON Schema definitions of the config surface. Use the `vtcode_config::schema` helpers to access the raw `RootSchema`, a `serde_json::Value`, or a pretty-printed JSON string for documentation tooling.
  - `vtcode-commons` to reuse the shared `WorkspacePaths` adapters for defaults resolution.
- Consumers that only need parsing/validation can disable default features and opt out of the `bootstrap` helpers to avoid pulling in filesystem scaffolding code.


### 2. Implement or Select a Defaults Provider
- The loader expects a `ConfigDefaultsProvider` implementation. Most integrations can reuse the bundled `WorkspacePathsDefaults` adapter:
  ```rust
  use vtcode_commons::paths::WorkspacePaths;
  use vtcode_config::defaults::{ConfigDefaultsProvider, WorkspacePathsDefaults};

  let paths = WorkspacePaths::discover()?;
  let defaults = WorkspacePathsDefaults::new(paths);
  ```
- Services that manage their own directories can implement the trait directly to expose bespoke config, cache, and prompt directories.

### 3. Update Loader Construction
- Replace calls to `VTCodeConfig::load_from_workspace` or `ConfigManager::load_from_workspace` with the new crate surface:
  ```rust
  use vtcode_config::loader::ConfigLoader;

  let loader = ConfigLoader::builder()
      .with_defaults(defaults)
      .build();

  let config = loader.load_from_workspace(&workspace_root)?;
  ```
- When you need raw struct access without IO, re-exported domain modules remain available under `vtcode_config::domains`.

### 4. Migrate Bootstrap Helpers
- Swap `VTCodeConfig::bootstrap_project` calls for `vtcode_config::bootstrap::bootstrap_project_with_provider`, passing the same defaults instance:
  ```rust
  use vtcode_config::bootstrap;

  let report = bootstrap::bootstrap_project_with_provider(
      &workspace_root,
      defaults,
      bootstrap::BootstrapOptions::default(),
  )?;
  ```
- The helper now ensures parent directories exist, reports created artifacts, and respects provider-driven path overrides.

### 5. Refresh Tests & Automation
- Update integration tests to install a provider before exercising bootstrap or loader flows. The `TestWorkspaceDefaults` helper in `vtcode-core/tests/config_loader_test.rs` illustrates how to inject ephemeral directories.
- Regenerate fixtures or golden files if they previously assumed `.vtcode`-relative paths.
- Run `cargo fmt`, `cargo clippy --all-targets`, and `cargo nextest run` (or `cargo test`) within your workspace to verify compatibility.

### 6. Clean Up Deprecated Imports
- Remove references to `vtcode_core::config::loader` and the old `VTCodeConfig` bootstrap APIs once your integration compiles against `vtcode-config`.
- Confirm that downstream crates only enable the feature flags they require to minimize dependency footprint.

## Rolling Adoption Strategy
- **Pilot phase:** migrate internal services first to validate defaults provider behavior and identify additional adapters needed.
- **General availability:** once documentation and examples stabilize, publish `vtcode-config` to crates.io and update top-level README references.
- **Deprecation window:** keep the legacy `vtcode-core` re-exports available for at least one minor release, emitting deprecation warnings that point to this guide.

## Additional Resources
- [Extraction strategy](./vtcode_config_extraction.md) for architectural context and crate layout proposals.
- `vtcode-core/src/config/defaults/provider.rs` for the live provider trait and reference adapter implementations.
- `vtcode-core/tests/config_loader_test.rs` showcasing provider-driven bootstrap tests.
- `cargo run --example minimal -p vtcode-config` for a runnable walkthrough that injects a custom defaults provider before loading configuration.
