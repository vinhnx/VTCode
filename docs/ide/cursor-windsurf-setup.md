# Cursor and Windsurf Setup Guide

This guide provides detailed instructions for installing and using VTCode with Cursor and Windsurf editors.

## Overview

VTCode is available for both Cursor and Windsurf through the Open VSX registry. Since both editors support VS Code extensions, the VTCode extension provides the same functionality as in VS Code.

## Prerequisites

Before installing the VTCode extension in Cursor or Windsurf, you need to install the VTCode CLI:

```bash
# Install with Cargo (recommended)
cargo install vtcode

# Or with Homebrew
brew install vtcode

# Or with NPM

```

Verify the installation:
```bash
vtcode --version
```

## Installation Methods

### Method 1: In-Editor Marketplace (Recommended)

#### For Cursor:
1. Open Cursor
2. Go to Extensions panel (Ctrl+Shift+X or Cmd+Shift+X)
3. Search for "vtcode-companion"
4. Click Install

#### For Windsurf:
1. Open Windsurf
2. Go to Extensions panel
3. Search for "vtcode-companion"
4. Click Install

### Method 2: VSIX File Installation

If the extension isn't available through the marketplace:

1. Download the `.vsix` file from [Open VSX](https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion)
2. In your editor's Extensions panel, click the "..." menu
3. Select "Install from VSIX..."
4. Choose the downloaded VSIX file

### Method 3: CLI Installation (Cursor Only)

For Cursor, you can also install the extension via CLI:
```bash
# Replace with actual version
cursor --install-extension vtcode-companion-<version>.vsix
```

## Configuration

After installation, the VTCode extension should work automatically if:

1. The VTCode CLI is installed and in your system PATH
2. Your workspace contains a `vtcode.toml` configuration file

To create a configuration file:
1. Open Command Palette (Ctrl+Shift+P or Cmd+Shift+P)
2. Run "VTCode: Open Configuration"
3. Configure your AI providers and settings

## Features Available

The VTCode extension provides the same features in Cursor and Windsurf as in VS Code:

- **Ask the Agent**: Send questions to the VTCode AI assistant
- **Ask About Selection**: Get explanations for selected code
- **Quick Actions Panel**: Access common VTCode commands
- **Terminal Integration**: Launch VTCode chat directly in the editor
- **Configuration Management**: Manage vtcode.toml settings
- **Semantic Code Intelligence**: AST-based code understanding

## Troubleshooting

### Extension Not Working

1. Verify VTCode CLI is installed:
   ```bash
   vtcode --version
   ```

2. Ensure VTCode is in your PATH:
   ```bash
   which vtcode
   ```

3. Restart your editor after installing the extension

4. Check that your workspace contains a `vtcode.toml` configuration file

### CLI Path Issues

If the extension can't find the VTCode CLI:

- **In Cursor/Windsurf settings**, look for `vtcode.commandPath` setting
- **Set the full path** to your VTCode executable if it's installed in a non-standard location

### Configuration Issues

1. Make sure your `vtcode.toml` file is in the root of your workspace
2. Verify your API keys are properly configured in the configuration file
3. Restart the editor after making configuration changes

## Support

If you encounter issues:

1. Check the [main troubleshooting guide](./troubleshooting.md)
2. Join our [community Discord](https://discord.gg/vtcode)
3. Open an issue on our [GitHub repository](https://github.com/vinhnx/vtcode/issues)
4. Verify that your Cursor or Windsurf version supports the VS Code extension format

## Updating the Extension

The extension will typically update automatically when new versions are published to Open VSX. You can also manually check for updates in your editor's Extensions panel.

---

*Note: VTCode is designed to work with VS Code-compatible editors through the Open VSX registry. For the best experience, ensure you have the latest version of both the extension and the VTCode CLI.*