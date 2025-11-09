# VTCode Zed Extension - Setup & Installation Guide

Complete guide for setting up the VTCode Zed extension in your environment.

## What's Included

✅ **Complete Zed Extension** for VTCode AI coding assistant
- Language support for `vtcode.toml` configuration files
- Integration with VTCode CLI
- WebAssembly binary (compiled and ready)
- Full documentation and roadmap

## Directory Structure

```
zed-extension/
├── extension.toml                 # Extension metadata
├── Cargo.toml                    # Rust project configuration
├── LICENSE                       # MIT License
├── src/lib.rs                   # Extension source code
├── languages/vtcode/config.toml # Language definition
├── target/release/libvtcode.dylib # Compiled binary (419KB)
│
├── README.md                    # User documentation
├── QUICK_START.md               # 5-minute setup guide
├── DEVELOPMENT.md               # Development setup
├── IMPLEMENTATION_ROADMAP.md    # Future features
├── extension-features.md        # Feature documentation
└── .gitignore                   # Git ignore rules
```

## Prerequisites

### Required
- **Zed Editor**: Latest stable version
- **Rust**: Installed via [rustup](https://www.rust-lang.org/tools/install)
- **VTCode CLI**: Install with `cargo install vtcode`

### Optional
- **Git**: For cloning and version control
- **Node.js**: If building from npm sources

## Installation Steps

### Step 1: Verify Prerequisites

```bash
# Check Rust installation
rustup --version

# Check Zed installation
zed --version

# Check VTCode CLI
vtcode --version
```

All three should output version information.

### Step 2: Get the Extension

**Option A: Development Installation** (recommended for testing)

```bash
# Clone VTCode repository
git clone https://github.com/vinhnx/vtcode.git
cd vtcode/zed-extension

# Open Zed
# Go to Extensions → "Install Dev Extension"
# Select the zed-extension directory
```

**Option B: Build from Source**

```bash
# Clone and build
git clone https://github.com/vinhnx/vtcode.git
cd vtcode/zed-extension

# Build the extension
cargo build --release

# The binary will be created at:
# target/release/libvtcode.dylib (macOS)
# target/release/libvtcode.so (Linux)
# target/release/libvtcode.dll (Windows)
```

**Option C: Install from Registry** (once published)

1. Open Zed
2. Go to Extensions (Cmd/Ctrl + Shift + X)
3. Search for "vtcode"
4. Click "Install"

### Step 3: Configure VTCode

Create `vtcode.toml` in your workspace root:

```toml
# Minimal Configuration
[ai]
provider = "anthropic"
model = "claude-3-5-sonnet-20241022"

# Advanced Configuration
[workspace]
analyze_on_startup = false
max_context_tokens = 8000
ignore_patterns = ["node_modules", ".git"]

[security]
human_in_the_loop = true
```

### Step 4: Set API Credentials

Choose your AI provider and set the API key:

```bash
# Anthropic (recommended)
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI
export OPENAI_API_KEY="sk-..."

# Google
export GOOGLE_API_KEY="AIza..."
```

### Step 5: Verify Installation

1. Open Zed
2. Open a workspace with `vtcode.toml`
3. Open command palette (Cmd/Ctrl + Shift + P)
4. Type "vtcode" - you should see available commands
5. Try running a command

## Troubleshooting Installation

### Extension Not Found in Zed

**Problem**: Extension doesn't appear in Extensions list

**Solution**:
```bash
# Ensure Rust is installed via rustup
rustup --version

# Reinstall the extension
# 1. Open Extensions panel
# 2. Click "Install Dev Extension"
# 3. Navigate to zed-extension directory
```

### Build Errors

**Problem**: `cargo build` fails

**Solution**:
```bash
# Clean build
cargo clean

# Update dependencies
cargo update

# Try building again
cargo build --release

# Check Rust version
rustc --version
# Should be 1.70.0 or newer
```

### VTCode CLI Not Found

**Problem**: Extension can't find vtcode command

**Solution**:
```bash
# Verify vtcode is installed
which vtcode

# If not installed
cargo install vtcode

# Verify installation
vtcode --version

# Check it's in PATH
echo $PATH
```

### API Key Errors

**Problem**: "Invalid API key" when running commands

**Solution**:
```bash
# Verify API key is set
echo $ANTHROPIC_API_KEY

# Get new API key from provider:
# - Anthropic: https://console.anthropic.com
# - OpenAI: https://platform.openai.com
# - Google: https://aistudio.google.com

# Set API key (add to ~/.zshrc or ~/.bashrc for persistence)
export ANTHROPIC_API_KEY="your-key-here"

# Reload shell
source ~/.zshrc  # or ~/.bashrc
```

## Verification Checklist

- [ ] Rust installed via rustup: `rustup --version`
- [ ] Zed installed: `zed --version`
- [ ] VTCode CLI installed: `vtcode --version`
- [ ] Extension installed in Zed Extensions
- [ ] `vtcode.toml` exists in workspace root
- [ ] API key set in environment: `echo $ANTHROPIC_API_KEY`
- [ ] Commands visible in command palette

## Next Steps

1. **Quick Start**: Read [QUICK_START.md](QUICK_START.md) (5 minutes)
2. **Full Documentation**: Read [README.md](README.md)
3. **Development**: If modifying code, see [DEVELOPMENT.md](DEVELOPMENT.md)
4. **Features**: Learn about capabilities in [extension-features.md](extension-features.md)

## File Locations

### macOS
- Zed Config: `~/.config/zed`
- Extension Cache: `~/.cache/zed`
- Logs: `~/.local/share/zed/logs`

### Linux
- Zed Config: `~/.config/zed`
- Extension Cache: `~/.cache/zed`
- Logs: `~/.local/share/zed/logs`

### Windows
- Zed Config: `%APPDATA%\Zed`
- Extension Cache: `%LOCALAPPDATA%\Zed`
- Logs: `%APPDATA%\Zed\logs`

## Environment Variables

Set these to customize the extension behavior:

```bash
# API Credentials
ANTHROPIC_API_KEY=sk-ant-...
OPENAI_API_KEY=sk-...

# Extension Configuration
VTCODE_LOG_LEVEL=debug
VTCODE_CONFIG_PATH=/path/to/vtcode.toml

# Rust/Build
RUST_BACKTRACE=1  # For debugging build issues
```

## Performance Tuning

### For Better Performance

1. **Reduce Context**: Lower `max_context_tokens` in `vtcode.toml`
2. **Exclude Directories**: Add to `ignore_patterns`
3. **Use Smaller Models**: Faster responses for simple queries
4. **Disable Auto-Analysis**: Set `analyze_on_startup = false`

### For Better Quality

1. **Increase Context**: Raise `max_context_tokens`
2. **Use Larger Models**: `claude-3-5-sonnet` vs `claude-3-haiku`
3. **Enable Analysis**: Set `analyze_on_startup = true`
4. **Provide Context**: Highlight relevant code before asking

## Security Considerations

- **API Keys**: Never commit to version control
- **Trust Model**: Respects Zed's workspace trust settings
- **Sandboxing**: Extension runs in secure WASM environment
- **File Access**: Limited to workspace directory

## Getting Help

### Documentation
- [VTCode Main Repo](https://github.com/vinhnx/vtcode)
- [Zed Editor Docs](https://zed.dev/docs)
- [Extension Source](./src/lib.rs)

### Support
- **Issues**: [GitHub Issues](https://github.com/vinhnx/vtcode/issues)
- **Discussions**: [GitHub Discussions](https://github.com/vinhnx/vtcode/discussions)
- **Feedback**: Star ⭐ the repo if you find it useful

## Uninstallation

To remove the extension:

1. Open Zed
2. Go to Extensions panel
3. Find "vtcode"
4. Click the ⋯ menu and select "Uninstall"
5. Restart Zed

Or manually:
```bash
# Remove from extensions directory
rm -rf ~/.cache/zed/extensions/installed/vtcode
```

## Version Information

- **Extension Version**: 0.1.0
- **Zed Compatibility**: 0.150.0+
- **VTCode CLI**: 0.1.0+
- **Rust Edition**: 2021
- **Last Updated**: November 2024

## License

This extension is licensed under the MIT License. See [LICENSE](LICENSE) file for details.

---

**Ready to get started?** Follow the installation steps above, then check out [QUICK_START.md](QUICK_START.md) for your first 5 minutes with VTCode!
