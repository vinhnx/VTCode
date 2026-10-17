<!-- Compact maintainer rules retain the repository instruction line budget. -->
<!-- markdownlint-disable MD013 -->
# xtask

[Root AGENTS.md](../../../AGENTS.md) | Release packaging automation for `cargo-binstall` extra-files layout.

## Role

Generates release archives with man pages and shell completions in the directory
layout that `cargo-binstall` auto-detects.

## Commands

```text
cargo xtask package-release --target <triple> --version <ver> --binary <path>
```

## Rules

- Runs on the host (not cross-compiled) -- only generates text files and copies binaries.
- Archive layout: `vtcode-{target}-v{version}/` with `man/`, `completions/`.
- The archive stage allowlist is intentional: package only `vtcode`, `man/`, and `completions/`; project instruction files never ship.
- Uses `vtcode_core::Cli::command()` for man page and completion generation.
