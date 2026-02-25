# VT Code Downloads

Welcome to the VT Code downloads center! VT Code is available across multiple platforms and IDEs to enhance your coding experience with AI-powered assistance.

## Available for Your IDE

Choose your favorite code editor to download VT Code:

### [Visual Studio Code](./ide-downloads.md#visual-studio-code)

[![VSCode Marketplace](https://img.shields.io/visual-studio-marketplace/v/nguyenxuanvinh.vtcode-companion?style=for-the-badge&logo=visual-studio-code&logoColor=white&label=VSCode%20Marketplace)](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion)

The original VT Code extension for Visual Studio Code with LLM-native code understanding and AI assistance.

### [Winsurf](./ide-downloads.md#winsurf)

[![Open VSX Registry](https://img.shields.io/badge/Available-Open%20VSX-4CAF50?style=for-the-badge&logo=opensearch&logoColor=white)](https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion)

VT Code is available for Windsurf through the Open VSX Registry. Install directly from the extensions marketplace or via VSIX file download.

### [Cursor](./ide-downloads.md#cursor)

[![Open VSX Registry](https://img.shields.io/badge/Available-Open%20VSX-2196F3?style=for-the-badge&logo=opensearch&logoColor=white)](https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion)

VT Code is available for Cursor through the Open VSX Registry. Install directly from the extensions marketplace, via VSIX file, or using the CLI.

## What is VT Code?

VT Code is a Rust-based AI coding assistant that provides:

-   **Semantic Code Understanding**: LLM-native code understanding and ripgrep integration
-   **Multi-Provider AI**: Support for OpenAI, Anthropic, Google, xAI, DeepSeek, and more
-   **Security First**: Built-in safeguards with human-in-the-loop controls
-   **Offline Analysis**: Analyze your codebase without sending code to external services
-   **Configurable**: Customizable through `vtcode.toml` configuration files

## Prerequisites

Before installing VT Code for your IDE, you need to install the VT Code CLI:

```bash
# Install with Cargo (recommended)
cargo install vtcode

# Or with Homebrew
brew install vtcode

# Or with NPM

```

## Installation Instructions

### Visual Studio Code

Install directly from the [VSCode Marketplace](https://marketplace.visualstudio.com/items?itemName=nguyenxuanvinh.vtcode-companion) or search for "vtcode-companion" in the Extensions panel.

### Windsurf

VT Code is available in the Windsurf extensions marketplace powered by Open VSX:

1. Open the Extensions panel in Windsurf
2. Search for "vtcode-companion"
3. Click Install

Alternatively, you can install from a VSIX file:

1. Download the `.vsix` file from [Open VSX](https://open-vsx.org/extension/nguyenxuanvinh/vtcode-companion)
2. In the Extensions panel, click the "..." menu and select "Install from VSIX..."
3. Select the downloaded file

For detailed setup instructions, see our [Cursor and Windsurf Setup Guide](./cursor-windsurf-setup.md).

### Cursor

VT Code is available in Cursor's extensions marketplace powered by Open VSX:

1. Open the Extensions panel in Cursor
2. Search for "vtcode-companion"
3. Click Install

Alternative installation methods for Cursor:

-   **VSIX file**: Command Palette → **Extensions: Install from VSIX…**
-   **CLI**: `cursor --install-extension vtcode-companion-<version>.vsix`
-   **URL**: Direct installation may be available depending on your Cursor version

For detailed setup instructions, see our [Cursor and Windsurf Setup Guide](./cursor-windsurf-setup.md).

## Support and Documentation

-   [Documentation](../README.md)
-   [Troubleshooting](./troubleshooting.md)
-   [Community Discord](https://discord.gg/vtcode)
-   [GitHub Issues](https://github.com/vinhnx/vtcode/issues)

---

_VT Code is designed to work with your favorite IDE to provide LLM-native code understanding and AI assistance. All VS Code compatible editors can use VT Code through the Open VSX registry._
