# vtcode-tools Policy Customization Guide

This guide explains how to adopt the `vtcode-tools` crate while keeping tool
policy configuration in your own application's storage hierarchy. The default
VT Code implementation persists policy state inside a `.vtcode` directory, but
external consumers often prefer to colocate policy files with their existing
configuration tree.

## 1. Enable the `policies` feature

The policy management APIs are exported behind the optional `policies` feature
flag. Enable it in your `Cargo.toml` to access `ToolPolicyManager` and related
types:

```toml
[dependencies]
vtcode-tools = { version = "0.0.1", features = ["policies"] }
```

## 2. Pick a custom storage location

Decide where policy state should live inside your application. For example, you
might align it with an existing configuration directory:

```rust
use std::path::PathBuf;

fn policy_path(root: &PathBuf) -> PathBuf {
    root.join("config").join("tool-policy.json")
}
```

## 3. Construct a `ToolPolicyManager` with your path

The `ToolPolicyManager::new_with_config_path` helper from `vtcode-core`
initializes the policy store without touching VT Code's default directories:

```rust
use vtcode_tools::policies::ToolPolicyManager;

let custom_manager = ToolPolicyManager::new_with_config_path(policy_path(&app_root))?;
```

The constructor ensures parent directories exist and loads (or creates) the JSON
configuration file at the provided path.

## 4. Inject the manager into the registry

`ToolRegistry` exposes dedicated constructors for supplying a pre-built policy
manager so the default VT Code wiring never executes:

```rust
use vtcode_tools::ToolRegistry;

let mut registry = ToolRegistry::new_with_custom_policy(workspace_root, custom_manager);
```

If you need to configure PTY behaviour or toggle planner support, the
`new_with_custom_policy_and_config` variant accepts the same knobs as the
existing constructors.

## 5. Apply your application's defaults

Once the registry is created you can call the usual policy helpers to enforce
your own defaults:

```rust
registry.allow_all_tools()?; // or selectively set policies per tool
```

Because the policy storage path is now under your control, the resulting JSON
file can be versioned, synchronized, or otherwise managed according to your
project's requirements.

## 6. Try the headless example

The workspace now ships with a runnable integration example that keeps policy
state out of `~/.vtcode` and registers a lightweight tool for headless usage:

```sh
cargo run -p vtcode-tools --example headless_registry --no-default-features --features "policies"
```

The example stores its policy file under a temporary configuration directory and
demonstrates how to register a custom tool while keeping feature flags slimmed
down to only `policies`. Treat it as a reference when wiring the registry into
your own application.

## Next steps

See `docs/component_extraction_plan.md` for the broader roadmap and remaining
extraction milestones.
