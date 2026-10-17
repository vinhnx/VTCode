# Tool Policy

> **Deprecated:** The `vtcode-tools` crate has been deleted (2026-06-14). All functionality has been merged into
> `vtcode-core`. The cache, middleware, executor, patterns, optimizer, and adapter modules now live in
> `vtcode_core::tools::*`. The examples below reference the old crate paths and are kept for historical reference only.

## vtcode-tools Policy Customization Guide (Historical)

This guide originally described how to adopt the `vtcode-tools` crate for tool policy configuration. Since the crate has
been deleted, use `vtcode-core` directly instead.

### Custom storage location

To persist policy state outside VT Code's default `.vtcode` directory:

```rust
use std::path::PathBuf;

fn policy_path(root: &PathBuf) -> PathBuf {
    root.join("config").join("tool-policy.json")
}
```

### Construct a `ToolPolicyManager` with your path

The `ToolPolicyManager::new_with_config_path` helper from `vtcode-core` initializes the policy store without touching VT
Code's default directories:

```rust
use vtcode_core::tools::policy::ToolPolicyManager;

let custom_manager = ToolPolicyManager::new_with_config_path(policy_path(&app_root))?;
```

### Next steps

See `docs/project/crate-consolidation-plan.md` for the broader roadmap and remaining consolidation milestones.
