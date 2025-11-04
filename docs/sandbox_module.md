# Sandbox Module Overview

The sandbox module bundles the reusable components that configure and persist
sandbox permissions for the command execution environment.  Projects can embed
this module directly from the `vtcode-core` crate by importing
`vtcode_core::sandbox`.

## Components

- `SandboxEnvironment` – High level controller that tracks the allow-listed
  filesystem locations, network domains, deny rules, and runtime metadata.
- `SandboxProfile` – Immutable snapshot that can be passed to runtime launchers
  (e.g. PTY or process executors) to run commands with sandboxing enabled.
- `SandboxSettings` – Serializable representation of the sandbox configuration
  that is written to disk for the runtime to consume.

## Quick Start

```rust,no_run
use anyhow::Result;
use vtcode_core::sandbox::{SandboxEnvironment, SandboxRuntimeKind};

fn main() -> Result<()> {
    let mut environment = SandboxEnvironment::builder("./workspace")
        .sandbox_root("./.vtcode/sandbox")
        .runtime_kind(SandboxRuntimeKind::AnthropicSrt)
        .build();

    environment.allow_domain("example.com")?;
    environment.allow_path("logs")?;
    environment.write_settings()?;

    let profile = environment.create_profile("/usr/local/bin/srt");
    println!("Sandbox settings stored at {}", environment.settings_path().display());
    println!("Runtime: {}", profile.runtime_kind());

    Ok(())
}
```

Refer to the inline documentation within the module for additional helpers and
extension points.
