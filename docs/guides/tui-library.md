# TUI Library Guide

This guide describes the extracted `vtcode-tui` crate and how to use it from
other crates.

## What It Provides

- Stable import surface for VT Code inline terminal UI primitives
- Standalone session options API (`SessionOptions`, `SessionSurface`, `KeyboardProtocolSettings`)
- Session lifecycle APIs (`spawn_session_with_options`, `spawn_session_with_host`)
- Typed command/event protocol (`InlineHandle`, `InlineCommand`, `InlineEvent`)
- Modal, plan-confirmation, and diff-preview data models

## Current Architecture

`vtcode-tui` now contains the migrated TUI implementation source in `src/core_tui/`.
For compatibility, `vtcode-core::ui::tui` remains the canonical runtime type
surface (compiled through a shim), and `vtcode-tui` re-exports that stable API.
Standalone session options still avoid direct `vtcode_core::config` imports in
downstream projects.

Implementation source location:

- `vtcode-tui/src/core_tui/` (full migrated TUI modules)
- `vtcode-core/src/ui/tui.rs` (compatibility shim)

## Usage

```rust
use vtcode_tui::{InlineTheme, SessionOptions, spawn_session_with_options};

# fn run() -> anyhow::Result<()> {
let options = SessionOptions {
    placeholder: Some("Prompt".to_string()),
    ..SessionOptions::default()
};
let _session = spawn_session_with_options(InlineTheme::default(), options)?;
# Ok(()) }
```

## Host Traits

`vtcode-tui::host` defines lightweight traits for future host decoupling:

- `WorkspaceInfoProvider`
- `NotificationProvider`
- `ThemeProvider`
- `HostAdapter`

`SessionOptions::from_host` and `spawn_session_with_host` consume these traits
for reusable, host-driven defaults.
