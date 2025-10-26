# VTCode Troubleshooting Guide

This guide covers common issues and solutions when using VTCode with your IDE.

## Prerequisites Not Found

**Issue**: VTCode extension can't find the VTCode CLI.

**Solution**:
1. Ensure VTCode CLI is installed:
   ```bash
   # Install with Cargo (recommended)
   cargo install vtcode
   
   # Or with Homebrew
   brew install vinhnx/tap/vtcode
   
   # Or with NPM
   npm install -g vtcode-ai
   ```
2. Check that VTCode is in your PATH:
   ```bash
   vtcode --version
   ```
3. If VTCode is installed in a custom location, update your IDE settings to point to the correct path:
   - VS Code: Set `vtcode.commandPath` in settings to the full path of the VTCode executable

## Extension Not Working

**Issue**: VTCode commands are not responding or showing errors.

**Solution**:
1. Restart your IDE after installing the extension
2. Check that your workspace contains a `vtcode.toml` configuration file
3. Verify the extension is enabled in your IDE
4. Check the IDE's output panel for error messages

## AI Provider Not Working

**Issue**: VTCode can't connect to AI providers (OpenAI, Anthropic, etc.).

**Solution**:
1. Ensure you have valid API keys in your `vtcode.toml` configuration file
2. Check that your API key has sufficient permissions
3. Verify your internet connection
4. Check if the AI provider has any service interruptions

## Slow Performance

**Issue**: VTCode is taking a long time to respond or analyze code.

**Solution**:
1. For large codebases, consider excluding large directories in your `vtcode.toml`
2. Check that your system has sufficient memory and CPU resources
3. Ensure your internet connection is stable if using cloud-based AI providers
4. Consider switching to a faster AI model in your configuration

## Configuration Issues

**Issue**: VTCode isn't using expected configuration settings.

**Solution**:
1. Verify your `vtcode.toml` file is in the root of your workspace
2. Check the syntax of your configuration file
3. Restart your IDE after making configuration changes
4. Use the "VTCode: Open Configuration" command to edit your config directly

## Need More Help?

If you're still experiencing issues:

1. Check the [main documentation](../README.md)
2. Join our [community Discord](https://discord.gg/vtcode)
3. Open an issue on our [GitHub repository](https://github.com/vinhnx/vtcode/issues)
4. Provide detailed information about your setup, the issue you're experiencing, and any error messages