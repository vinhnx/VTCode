# vtcode-terminal-detection

Shared terminal detection primitives for VT Code.

It provides:

- `TerminalType` – enum of supported terminal emulators (Ghostty, Kitty, Alacritty, WezTerm, iTerm2, VS Code, Warp, Zed, and more)
- `TerminalFeature` – capabilities such as Multiline, CopyPaste, ShellIntegration, ThemeSync, Notifications
- `TerminalSetupAvailability` – whether a terminal has NativeSupport, is Offered setup, or GuidanceOnly
- `is_ghostty_terminal()` – quick helper for Ghostty detection

## Usage

```rust
use vtcode_terminal_detection::{TerminalType, TerminalFeature};

fn main() -> anyhow::Result<()> {
    let terminal = TerminalType::detect()?;
    println!("Running in: {}", terminal.name());

    if terminal.supports_feature(TerminalFeature::Multiline) {
        println!("Multiline input supported");
    }

    if terminal.should_offer_terminal_setup() {
        println!("Terminal setup available");
    }

    let config = terminal.config_path()?;
    println!("Config at: {}", config.display());

    Ok(())
}
```

## API Reference

### `TerminalType`

| Method | Description |
|---|---|
| `detect() -> Result<Self>` | Detect terminal from environment variables (`TERM_PROGRAM`, `KITTY_WINDOW_ID`, `ALACRITTY_SOCKET`, `ZED_TERMINAL`, etc.) |
| `name() -> &'static str` | Human-readable terminal name |
| `supports_feature(TerminalFeature) -> bool` | Check if a specific feature is supported |
| `has_native_multiline_support() -> bool` | Whether multiline works without config changes |
| `terminal_setup_availability() -> TerminalSetupAvailability` | How `/terminal-setup` should be presented |
| `should_offer_terminal_setup() -> bool` | Whether `/terminal-setup` appears in discovery |
| `requires_manual_setup() -> bool` | Whether the terminal needs manual configuration |
| `config_path() -> Result<PathBuf>` | Platform-aware path to the terminal's config file |

### `TerminalFeature`

`Multiline`, `CopyPaste`, `ShellIntegration`, `ThemeSync`, `Notifications`

### `TerminalSetupAvailability`

`NativeSupport`, `Offered`, `GuidanceOnly`
