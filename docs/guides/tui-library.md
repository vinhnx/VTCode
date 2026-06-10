# TUI Library Guide

This guide describes the `vtcode-ui` crate and how to use its TUI functionality
from other crates.

## What It Provides

- Stable import surface for VT Code inline terminal UI primitives
- Explicit module split: `vtcode_ui::tui::core` for reusable TUI foundation, `vtcode_ui::tui::app` for VT Code-specific overlays and behaviors
- Standalone session options API (`vtcode_ui::tui::app::SessionOptions`, `SessionSurface`, `KeyboardProtocolSettings`)
- Session lifecycle APIs (`vtcode_ui::tui::app::spawn_session_with_options`, `spawn_session_with_host`)
- Typed command/event protocol (`vtcode_ui::tui::app::InlineHandle`, `InlineCommand`, `InlineEvent`)
- Modal, plan-confirmation, and diff-preview data models

## Current Architecture

`vtcode-ui` contains the TUI implementation source in `src/tui/core_tui/`.
For compatibility, `vtcode-core::ui::tui` remains the canonical runtime type
surface (compiled through a shim) and re-exports the app-layer API.
Standalone session options still avoid direct `vtcode_core::config` imports in
downstream projects.

Implementation source location:

- `vtcode-ui/src/tui/core_tui/` (full TUI modules)
- `vtcode-core/src/ui/tui.rs` (compatibility shim)

## Usage

```rust
use vtcode_ui::tui::app::{InlineTheme, SessionOptions, spawn_session_with_options};

# fn run() -> anyhow::Result<()> {
let options = SessionOptions {
    placeholder: Some("Prompt".to_string()),
    ..SessionOptions::default()
};
let _session = spawn_session_with_options(InlineTheme::default(), options)?;
# Ok(()) }
```

## Host Traits

`vtcode_ui::tui::host` defines lightweight traits for future host decoupling:

- `WorkspaceInfoProvider`
- `NotificationProvider`
- `ThemeProvider`
- `HostAdapter`

`SessionOptions::from_host` and `spawn_session_with_host` consume these traits
for reusable, host-driven defaults.
