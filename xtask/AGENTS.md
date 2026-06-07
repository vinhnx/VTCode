# xtask

[Root AGENTS.md](../AGENTS.md) | Release packaging automation for `cargo-binstall` extra-files layout.

## Role

Generates release archives with man pages and shell completions in the directory
layout that `cargo-binstall` auto-detects.

## Commands

```
cargo xtask package-release --target <triple> --version <ver> --binary <path>
```

## Rules

- Runs on the host (not cross-compiled) -- only generates text files and copies binaries.
- Archive layout: `vtcode-{target}-v{version}/` with `man/`, `completions/`.
- Uses `vtcode_core::Cli::command()` for man page and completion generation.
