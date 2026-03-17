# vtcode-tui

Reusable terminal UI primitives and session APIs for Rust CLI/TUI applications.

## Status

`vtcode-tui` is self-contained and can be used without `vtcode-core` or `vtcode-config`.
The crate focuses on terminal UI primitives and session rendering only (no auth/provider logic).
The full implementation lives in `src/core_tui/`.

The public API is split into:

- `vtcode_tui::core` — reusable TUI foundation (core session + widgets)
- `vtcode_tui::app` — VT Code–specific overlays and behaviors

For integrations, use the app-layer options API:

- `vtcode_tui::app::SessionOptions`
- `vtcode_tui::app::SessionSurface`
- `vtcode_tui::app::KeyboardProtocolSettings`
- `vtcode_tui::app::spawn_session_with_options`
- `vtcode_tui::app::spawn_session_with_host`

Host-injected customization in `SessionOptions`:
- `slash_commands`: command palette metadata
- `appearance`: optional UI appearance override (`SessionAppearanceConfig`)
- `app_name`: terminal title/app branding text
- `non_interactive_hint`: custom message when no interactive TTY is available

## Quick Start

```rust
use vtcode_tui::app::{
    InlineHeaderContext, InlineTheme, SessionAppearanceConfig, SessionOptions,
    SlashCommandItem, spawn_session_with_options,
};

# fn run() -> anyhow::Result<()> {
let _context = InlineHeaderContext::default();
let _theme = InlineTheme::default();

let options = SessionOptions {
    placeholder: Some("Ask me anything...".to_string()),
    app_name: "My Agent".to_string(),
    slash_commands: vec![SlashCommandItem::new("help", "Show help")],
    appearance: Some(SessionAppearanceConfig::default()),
    ..SessionOptions::default()
};

let _session = spawn_session_with_options(InlineTheme::default(), options)?;
# Ok(()) }
```

## Public API Highlights

- Core layer: `vtcode_tui::core::{CoreSession, CoreHandle, CoreCommand, CoreEvent}`
- App layer: `vtcode_tui::app::{InlineSession, InlineHandle, InlineCommand, InlineEvent}`
- Session lifecycle: `vtcode_tui::app::{spawn_session_with_options, spawn_session_with_host}`
- UI models: plans, diff previews, modal/list/wizard selection types
- Theme/style helpers: `vtcode_tui::core::{theme_from_styles, convert_style}`

## Examples

- `examples/minimal_session.rs`
- `examples/custom_theme_and_widgets.rs`
- `examples/host_adapter_integration.rs`
