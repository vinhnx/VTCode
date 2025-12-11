VTCode migration toolkit (next-gen harness)

-   Detects missing plugin manifests; default search path `~/.vtcode/plugins`.
-   Aligns new RL config (`[optimization]`) and zero-trust security toggles.
-   Use `apply_migration_defaults` (crate `vtcode-core::utils::migration`) before constructing the agent.
-   Generate a summary for dashboards: `migration_summary(&config)`.
-   Back up existing `vtcode.toml` before writing changes.
