## Description

Upgrading ratatui from `0.30.0` to `0.30.1` causes compile-time `Send` trait bound violations in types related to `CellEffect` from the `tachyonfx` crate (used via `ratatui-cheese`). This forces downstream projects to pin `ratatui = "=0.30.0"` as a workaround.

## Reproduction

1. Have a project that depends on:
   - `ratatui` 0.30.x (with `crossterm` feature)
   - `ratatui-cheese` 0.7 (which transitively pulls in `tachyonfx`)
2. Build with ratatui 0.30.0 — compiles successfully
3. Change to ratatui 0.30.1 (or use `"0.30"` semver range) — compile fails with `Send` bound errors on `CellEffect`-related types

## Expected behavior

A patch version bump (0.30.0 to 0.30.1) should not break `Send` trait compatibility for downstream types.

## Suspected cause

`ratatui-widgets` 0.3.1 (pulled in by ratatui 0.30.1) introduced changes to cell-diff internals (`CellDiffOption` and related types). These changes likely altered the `Send` implementation status of some type in the dependency chain, breaking `CellEffect` types from `tachyonfx` that depend on `Send` bounds.

The `tachyonfx` crate has a `sendable` feature that enables `Send` for effects/shaders, and when active, requires all `Shader` implementations to satisfy `Send`. The ratatui-widgets 0.3.1 changes appear to have introduced a non-`Send` type into a path that `CellEffect` transitively depends on.

## Environment

- Rust: 1.88
- Edition: 2024
- OS: macOS (Darwin 25.5.0), also reproducible on Linux
- ratatui: 0.30.0 (working) -> 0.30.1 (broken)
- ratatui-cheese: 0.7.0
- ratatui-widgets: 0.3.0 (working) -> 0.3.1 (broken)

## Workaround

Pin ratatui to the exact working version in workspace `Cargo.toml`:

```toml
ratatui = { version = "=0.30.0", default-features = false, features = ["crossterm"] }
```

## Additional context

- Pinning commit: `8539a774a` in [vinhnx/vtcode](https://github.com/vinhnx/vtcode)
- The `CellEffect` type itself comes from `tachyonfx`, not ratatui directly, but the breakage is triggered by the ratatui 0.30.1 upgrade
