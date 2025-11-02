# VTCode

[![VSCode Marketplace](https://img.shields.io/visual-studio-marketplace/v/nguyenxuanvinh.vtcode-companion?style=flat-square&label=VSCode%20Marketplace&logo=visual-studio-code)](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion)
[![Installs](https://img.shields.io/visual-studio-marketplace/i/nguyenxuanvinh.vtcode-companion?style=flat-square&logo=visual-studio-code)](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion)
[![Downloads](https://img.shields.io/visual-studio-marketplace/d/nguyenxuanvinh.vtcode-companion?style=flat-square&logo=visual-studio-code)](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion)
[![Rating](https://img.shields.io/visual-studio-marketplace/r/nguyenxuanvinh.vtcode-companion?style=flat-square&logo=visual-studio-code)](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion)

**VTCode** is an AI coding assistant available for your favorite IDEs. This repository contains the Visual Studio Code extension, providing deep integration with [VT Code](https://github.com/vinhnx/vtcode), a Rust-based terminal coding agent with semantic code intelligence.

## Download VT Code Extension for your IDE

[**Visual Studio Code**](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion) | [**Winsurf** (Coming Soon)](./docs/ide-downloads.md) | [**Cursor** (Coming Soon)](./docs/ide-downloads.md) | [**All Downloads**](./docs/ide-downloads.md)

For installation instructions and download links for other IDEs, visit our [IDE Downloads](./docs/ide-downloads.md) page.

## Features

-   **AI Coding Assistant**: Access the VTCode agent directly from VS Code
-   **Quick Actions**: Easily send questions and get responses without leaving your editor
-   **Code Analysis**: Analyze your workspace with semantic code intelligence
-   **Configuration Management**: Edit your `vtcode.toml` configuration files with syntax highlighting
-   **Context Awareness**: Leverages Tree-sitter and ast-grep for deep code understanding
-   **Multi-Provider AI**: Supports OpenAI, Anthropic, Google, xAI, DeepSeek, and more
-   **Security First**: Built-in safeguards with human-in-the-loop controls

## Prerequisites

Before using this extension, you need to have the VTCode CLI installed:

```bash
# Install with Cargo (recommended)
cargo install vtcode

# Or with Homebrew
brew install vtcode

# Or with NPM
npm install -g vtcode-ai
```

## Quick Start

1. Install the VTCode CLI using one of the methods above
2. Install this extension from the VS Code Marketplace
3. Open a workspace containing a `vtcode.toml` file
4. Access VTCode features through:
    - The Command Palette (`Cmd+Shift+P` or `Ctrl+Shift+P`)
    - The VTCode Quick Actions view in the Explorer
    - Right-click context menu on selected code
    - Status bar icon

## Commands

The extension contributes the following commands:

-   `VTCode: Open Quick Actions` - Access the quick actions panel
-   `VTCode: Ask the Agent` - Send a question to the VTCode agent
-   `VTCode: Ask About Selection` - Ask about highlighted code
-   `VTCode: Launch Agent Terminal` - Open an integrated terminal session running `vtcode chat`
-   `VTCode: Analyze Workspace` - Run `vtcode analyze` on your workspace
-   `VTCode: Open Configuration` - Edit your `vtcode.toml` configuration file
-   `VTCode: Open Documentation` - Access VTCode documentation
-   `VTCode: Toggle Human-in-the-Loop` - Control human approval for sensitive operations
-   And more...

## Configuration

The extension contributes the following settings:

-   `vtcode.commandPath`: Path to the VTCode executable (default: `vtcode`)

## Requirements

-   VS Code version 1.87.0 or higher
-   VTCode CLI installed and accessible in your PATH

## Contributing

Contributions are welcome! Please see the [main VTCode repository](https://github.com/vinhnx/vtcode) for contribution guidelines.

### Development

For development instructions, see [DEVELOPMENT.md](DEVELOPMENT.md).

### Releasing

To release a new version of the extension:

```bash
# Patch release (0.1.1 -> 0.1.2)
./release.sh patch

# Minor release (0.1.1 -> 0.2.0)
./release.sh minor

# Major release (0.1.1 -> 1.0.0)
./release.sh major
```

The release script automates version bumping, building, packaging, and publishing to both VSCode Marketplace and Open VSX Registry. See [RELEASE.md](RELEASE.md) for details.

## Support

If you find VTCode useful, please consider supporting the project by visiting [BuyMeACoffee](https://www.buymeacoffee.com/vinhnx).

## License

This extension is licensed under the [MIT License](LICENSE).
