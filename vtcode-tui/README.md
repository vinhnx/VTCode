# vtcode-tui

Reusable inline terminal UI API for VT Code-style interfaces.

## Status

`vtcode-tui` now contains the full migrated TUI implementation source under `src/core_tui/`.

`vtcode-core::ui::tui` remains the canonical runtime type surface via a compatibility shim.
`vtcode-tui` re-exports that stable API while housing the migrated source tree.

For standalone integrations, prefer the crate-local options API:

- `SessionOptions`
- `SessionSurface`
- `KeyboardProtocolSettings`
- `spawn_session_with_options`
- `spawn_session_with_host`

## Quick Start

```rust
use vtcode_tui::{InlineHeaderContext, InlineTheme, SessionOptions, spawn_session_with_options};

# fn run() -> anyhow::Result<()> {
let _context = InlineHeaderContext::default();
let _theme = InlineTheme::default();

let options = SessionOptions {
    placeholder: Some("Ask me anything...".to_string()),
    ..SessionOptions::default()
};

let _session = spawn_session_with_options(InlineTheme::default(), options)?;
# Ok(()) }
```

## Public API Highlights

- Session lifecycle: `spawn_session_with_options`, `spawn_session_with_host`, `InlineSession`
- Interaction: `InlineHandle`, `InlineCommand`, `InlineEvent`
- UI models: plans, diff previews, modal/list/wizard selection types
- Theme/style helpers: `theme_from_styles`, `convert_style`

## Examples

- `examples/minimal_session.rs`
- `examples/custom_theme_and_widgets.rs`
- `examples/host_adapter_integration.rs`
