# vtcode-config

Config loader components shared across VT Code and downstream adopters.

Exposes `VTCodeConfig` and `ConfigManager` for reading and validating
`vtcode.toml` files. Applications can customize the filesystem layout via
`ConfigDefaultsProvider` and opt into the `bootstrap` feature (enabled by
default) to scaffold configuration directories with project-specific defaults.

## Modules

| Module | Purpose |
|---|---|
| `loader` | Config parsing, merging, watching, and layer stack |
| `core` | Primary config structs (`AgentConfig`, `ModelConfig`, `SandboxConfig`, …) |
| `defaults` | `ConfigDefaultsProvider` and helpers for search paths |
| `acp` | Agent Client Protocol configuration |
| `api_keys` | API key source resolution |
| `auth` | OAuth / ChatGPT / Copilot auth flows |
| `codex` | TUI and history persistence settings |
| `constants` | Compile-time constants |
| `context` | Dynamic context and ledger settings |
| `debug` | Debug and trace configuration |
| `hooks` | Lifecycle hook configuration |
| `ide_context` | IDE context provider configuration |
| `mcp` | MCP` | MCP server and client configuration |
| `models` | Model identifiers and metadata |
| `optimization` | Performance tuning knobs (caching, pooling, profiling) |
| `subagents` | Sub-agent discovery and specs |

## Public entrypoints

| Export | Description |
|---|---|
| `VTCodeConfig` | Deserialized configuration root |
| `ConfigManager` | Load, merge, and watch configuration files |
| `ConfigDefaultsProvider` | Trait for customizing default paths and values |
| `install_config_defaults_provider` | Register a custom defaults provider globally |
| `ConfigLayerStack` | Ordered stack of config layers with merge semantics |
| `ConfigWatcher` / `SimpleConfigWatcher` | File-system watchers for live reload |

## Usage

```rust
use vtcode_config::ConfigManager;

fn main() -> anyhow::Result<()> {
    let manager = ConfigManager::load_from_workspace(".")?;
    println!("Active provider: {}", manager.config().agent.provider);
    Ok(())
}
```

## Features

| Feature | Default | Description |
|---|---|---|
| `bootstrap` | ✓ | Scaffold config directories on first load |
| `schema` | — | JSON Schema generation via `schemars` |

<!-- cargo-rdme start -->

Shared configuration loader utilities for VT Code and downstream integrations.

This crate exposes `VTCodeConfig` and `ConfigManager` for reading and
validating `vtcode.toml` files while allowing applications to customize the
filesystem layout via `ConfigDefaultsProvider`. Consumers can opt into the
`bootstrap` feature (enabled by default) to scaffold
configuration directories with project-specific defaults.
Disable default features when you only need parsing/validation to omit the
filesystem bootstrap helpers and reduce dependencies.

### Examples
```rust
use vtcode_config::ConfigManager;

let manager = ConfigManager::load_from_workspace(".")?;
println!("Active provider: {}", manager.config().agent.provider);
```

Install a custom `ConfigDefaultsProvider` with
`install_config_defaults_provider` when you need to override search paths
or syntax highlighting defaults exposed by the loader.

<!-- cargo-rdme end -->

## Adding a Model
