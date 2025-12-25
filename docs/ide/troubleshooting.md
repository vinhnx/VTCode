# VT Code Troubleshooting Guide

This guide covers common issues and solutions when using VT Code with your IDE.

## Prerequisites Not Found

**Issue**: VT Code extension can't find the VT Code CLI.

**Solution**:

1. Ensure VT Code CLI is installed:

    ```bash
    # Install with Cargo (recommended)
    cargo install vtcode

    # Or with Homebrew
    brew install vtcode

    # Or with NPM

    ```

2. Check that VT Code is in your PATH:
    ```bash
    vtcode --version
    ```
3. If VT Code is installed in a custom location, update your IDE settings to point to the correct path:
    - VS Code: Set `vtcode.commandPath` in settings to the full path of the VT Code executable
    - Cursor/Windsurf: Look for similar extension settings to specify the VT Code executable path

## Extension Not Working

**Issue**: VT Code commands are not responding or showing errors.

**Solution**:

1. Restart your IDE after installing the extension
2. Check that your workspace contains a `vtcode.toml` configuration file
3. Verify the extension is enabled in your IDE
4. Check the IDE's output panel for error messages

## AI Provider Not Working

**Issue**: VT Code can't connect to AI providers (OpenAI, Anthropic, etc.).

**Solution**:

1. Ensure you have valid API keys in your `vtcode.toml` configuration file
2. Check that your API key has sufficient permissions
3. Verify your internet connection
4. Check if the AI provider has any service interruptions

## Slow Performance

**Issue**: VT Code is taking a long time to respond or analyze code.

**Solution**:

1. For large codebases, consider excluding large directories in your `vtcode.toml`
2. Check that your system has sufficient memory and CPU resources
3. Ensure your internet connection is stable if using cloud-based AI providers
4. Consider switching to a faster AI model in your configuration

## Configuration Issues

**Issue**: VT Code isn't using expected configuration settings.

**Solution**:

1. Verify your `vtcode.toml` file is in the root of your workspace
2. Check the syntax of your configuration file
3. Restart your IDE after making configuration changes
4. Use the "VTCode: Open Configuration" command to edit your config directly

## VS Code-Compatible Editors

**Issue**: Using VT Code with Cursor, Windsurf, or other VS Code-compatible editors.

**Solution**:
VT Code works with any VS Code-compatible editor through the Open VSX registry:

1. Ensure the VT Code CLI is installed separately on your system
2. Install the extension from the Open VSX registry or via VSIX file
3. The extension behavior should be identical to VS Code
4. Configuration settings may be located in different places depending on the editor

## Need More Help?

If you're still experiencing issues:

1. Check the [main documentation](../README.md)
2. Review the [Cursor and Windsurf Setup Guide](./cursor-windsurf-setup.md) for editor-specific instructions
3. Join our [community Discord](https://discord.gg/vtcode)
4. Open an issue on our [GitHub repository](https://github.com/vinhnx/vtcode/issues)
5. Provide detailed information about your setup, the issue you're experiencing, and any error messages
