# VTCode Zed Extension Development Guide

This guide covers developing and testing the VTCode extension for Zed.

## Prerequisites

- **Rust**: Install via [rustup](https://www.rust-lang.org/tools/install)
- **Zed**: Latest version of the Zed editor
- **VTCode CLI**: Install using `cargo install vtcode` or your package manager

## Building the Extension

### Development Build

```bash
# Check the code compiles
cargo check

# Build debug version
cargo build

# Build optimized release version
cargo build --release
```

### WebAssembly Compilation

The extension is compiled to WebAssembly (WASM). After building, the binary will be in `target/wasm32-unknown-unknown/release/vtcode.wasm` (for release builds).

Zed automatically handles the WASM compilation and linking when you install the dev extension.

## Installing as a Dev Extension

1. Open Zed
2. Open the Extensions panel (Cmd/Ctrl + Shift + X or from Extensions menu)
3. Click "Install Dev Extension" button
4. Select this directory (`zed-extension`)
5. The extension should appear in your extensions list as "overridden by dev extension"

## Testing the Extension

### Manual Testing

1. Create a test workspace with a `vtcode.toml` file:
   ```toml
   [ai]
   provider = "anthropic"
   model = "claude-3-5-sonnet-20241022"
   ```

2. Open the workspace in Zed
3. Test extension features:
   - Syntax highlighting for `vtcode.toml`
   - Configuration file validation
   - VTCode commands in the command palette

### Debugging

To see debug output:

```bash
# Start Zed from the terminal with --foreground for verbose logging
zed --foreground
```

Check the Zed log file:
```bash
# On macOS
tail -f ~/.local/share/zed/logs/zed.log

# On Linux
tail -f ~/.local/share/zed/logs/zed.log

# On Windows
Get-Content "$(env:APPDATA)\Zed\logs\zed.log" -Tail 50 -Wait
```

## Project Structure

```
zed-extension/
├── extension.toml           # Extension metadata
├── Cargo.toml              # Rust package configuration
├── src/
│   └── lib.rs              # Extension Rust code
├── languages/
│   └── vtcode/             # Language support for vtcode.toml
│       └── config.toml     # Language metadata
├── README.md               # User documentation
├── DEVELOPMENT.md          # This file
├── LICENSE                 # MIT License
└── .gitignore             # Git ignore rules
```

## Extension Capabilities

The VTCode extension provides:

1. **Language Support**: Syntax highlighting and validation for `vtcode.toml`
2. **Configuration**: Reading workspace `vtcode.toml` configuration
3. **Integration**: Bridges Zed editor with VTCode CLI agent

## Building and Publishing

### Local Testing

```bash
# Build the extension
cargo build --release

# Install as dev extension in Zed
# Use "Install Dev Extension" in Zed's Extensions panel
```

### Publishing to Zed Extension Registry

Follow the [Zed Extension Publishing Guide](https://zed.dev/docs/extensions/developing-extensions#publishing-your-extension):

1. Fork the [zed-industries/extensions](https://github.com/zed-industries/extensions) repository
2. Create a subdirectory for the extension
3. Add the extension as a Git submodule
4. Update `extensions.toml`
5. Open a pull request

### Version Bumping

Update version in both files:
- `extension.toml`: `version = "x.y.z"`
- `Cargo.toml`: `version = "x.y.z"`

## Troubleshooting

### Extension Not Appearing

- Ensure `extension.toml` is valid TOML
- Check Zed logs for compilation errors
- Verify Rust is installed via rustup (required for dev extensions)

### Build Failures

```bash
# Clean build
cargo clean
cargo build --release

# Update dependencies
cargo update
cargo check
```

### Language Support Not Working

- Ensure `languages/vtcode/config.toml` exists
- Check language configuration syntax
- Verify the language name matches in configuration files

## API Documentation

- [Zed Extension API Reference](https://zed.dev/docs/extensions)
- [Zed Extension Capabilities](https://zed.dev/docs/extensions/capabilities)
- [Language Extensions](https://zed.dev/docs/extensions/languages)

## Contributing

Contributions are welcome! Please:

1. Follow Rust naming conventions (snake_case for functions, PascalCase for types)
2. Test your changes locally before submitting
3. Update documentation as needed
4. Keep commits focused and descriptive

## Additional Resources

- [Zed Extension Developing Guide](https://zed.dev/docs/extensions/developing-extensions)
- [VTCode Main Repository](https://github.com/vinhnx/vtcode)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
