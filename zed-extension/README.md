# VT Code Agent Server Extension

This extension provides the VT Code AI coding assistant as an Agent Server in Zed through the Agent Client Protocol (ACP).

## Features

-   Full integration with Zed's Agent panel
-   Semantic code intelligence via Tree-sitter
-   Support for multiple LLM providers (OpenAI, Anthropic, Google Gemini, etc.)
-   Secure execution with workspace boundaries and tool policies
-   Real-time streaming responses and reasoning

## Configuration

The extension automatically configures the necessary environment variables for ACP:

-   `VT_ACP_ENABLED=1` - Enables the Agent Client Protocol bridge
-   `VT_ACP_ZED_ENABLED=1` - Enables the Zed transport

## Platform Support

The extension supports the following platforms:

-   macOS (darwin-aarch64, darwin-x86_64)
-   Linux (linux-x86_64)
-   Windows (windows-x86_64)

## Building from Source

To modify and build the extension:

1. Build VT Code with `cargo build --release`
2. Create release archives with your built binary
3. Update the archive URLs in `extension.toml` to point to your built binaries
4. Generate SHA-256 checksums for security
5. Install the extension in Zed as a development extension

## Development and Testing

For local development and testing:

1. Build VT Code: `cd /path/to/vtcode && cargo build --release`
2. Create a test release archive:
    - `mkdir temp_package && cp target/release/vtcode temp_package/`
    - `cd temp_package && tar -czf vtcode-PLATFORM.tar.gz vtcode`
3. Calculate the SHA-256 checksum of your archive: `shasum -a 256 vtcode-PLATFORM.tar.gz`
4. Update `extension.toml` archive URLs to point to your local test archive file (host via HTTP server or use GitHub release)
5. Update SHA-256 hashes in `extension.toml` with actual values from your test file
6. Install as a development extension in Zed: Command Palette → `zed: install dev extension` → select this directory
7. Check the ACP logs in Zed to verify the agent server is communicating properly
8. Test the agent functionality in the Agent panel

## Installation Troubleshooting

If you encounter "failed to install zed extensions" errors:

1. **Check Release Availability**: The extension tries to download VT Code binaries from GitHub releases. Ensure that the version specified in `extension.toml` exists as a release on GitHub.

2. **Verify Checksums**: If using custom binaries, make sure the SHA-256 checksums in `extension.toml` match your actual binary files.

3. **Use Development Installation**: For local development, use Zed's "Install Dev Extension" feature to sidestep release availability issues.

4. **Network Access**: Verify that Zed can access the URLs specified in the extension.toml file.

5. **Build from Source**: If binaries aren't available, build VT Code from source using `cargo build --release` and create your own release archive.

**Important**: The default extension.toml references GitHub releases that may not exist for all versions. When in doubt, use the development installation method.

## Requirements

-   Zed v0.201 or later with Agent Client Protocol support
-   VT Code configuration with your AI provider settings
