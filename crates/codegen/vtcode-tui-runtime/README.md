# vtcode-tui-runtime

TUI and ANSI rendering runtime for [VT Code](https://github.com/vinhnx/vtcode).

This crate owns the terminal-rendering code that used to sit behind
`vtcode-core`'s `tui` feature: theme resolution, markdown-to-ANSI rendering,
transcript presentation, the inline-session protocol, and the terminal setup
wizard. `vtcode-core` has no `tui` feature and no dependency on `vtcode-ui`,
so headless consumers (for example `vtcode-acp`) compile a genuinely UI-free
dependency graph.

## Usage

```rust,ignore
use vtcode_tui_runtime::utils::ansi::AnsiRenderer;
use vtcode_tui_runtime::ui::theme;

let renderer = AnsiRenderer::stdout();
```

## License

MIT OR Apache-2.0
