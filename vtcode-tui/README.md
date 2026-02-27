# vtcode-tui

Reusable terminal UI primitives and session APIs for Rust CLI/TUI applications.

## Status

`vtcode-tui` is self-contained and can be used without `vtcode-core` or `vtcode-config`.
The crate focuses on terminal UI primitives and session rendering only (no auth/provider logic).
The full implementation lives in `src/core_tui/`.
For integrations, use the standalone options API:

- `SessionOptions`
- `SessionSurface`
- `KeyboardProtocolSettings`
- `spawn_session_with_options`
- `spawn_session_with_host`

Host-injected customization in `SessionOptions`:
- `slash_commands`: command palette metadata
- `appearance`: optional UI appearance override (`SessionAppearanceConfig`)
- `app_name`: terminal title/app branding text
- `non_interactive_hint`: custom message when no interactive TTY is available

## Quick Start

```rust
use vtcode_tui::{
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

- Session lifecycle: `spawn_session_with_options`, `spawn_session_with_host`, `InlineSession`
- Interaction: `InlineHandle`, `InlineCommand`, `InlineEvent`
- UI models: plans, diff previews, modal/list/wizard selection types
- Theme/style helpers: `theme_from_styles`, `convert_style`, `ratatui_style_from_ansi`

## Examples

- `examples/minimal_session.rs`
- `examples/custom_theme_and_widgets.rs`
- `examples/host_adapter_integration.rs`
