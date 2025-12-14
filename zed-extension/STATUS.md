# VT Code Extension Status

## Agent Server Implementation

  **Complete**: Full Agent Server extension configured for VT Code

### Features
-   Agent Server registration in extension.toml
-   Cross-platform support (macOS, Linux, Windows)
-   ACP (Agent Client Protocol) integration
-   Environment variable configuration
-   SVG icon included
-   Proper command structure (`vtcode acp`)

### Platform Targets
-   darwin-aarch64 (macOS ARM64)
-   darwin-x86_64 (macOS Intel)
-   linux-x86_64 (Linux)
-   windows-x86_64 (Windows)

### Testing Status
-  Development testing requires actual binaries at specified URLs
-   Ready for Zed marketplace submission when binaries are published

### Notes
The VT Code Agent Server Extension is a configuration package that tells Zed how to download and run VT Code in ACP mode. VT Code itself is a Rust CLI application with built-in ACP support, not a Rust extension for Zed.

The extension is configured to download VT Code binaries from GitHub releases, but these URLs contain placeholder references to version 0.3.0 which may not exist. For successful local development testing:

1. Build VT Code for your platform: `cargo build --release`
2. Create test archives with the correct file structure
3. Host the binaries at the URLs specified in extension.toml (or update the URLs to point to your test binaries)
4. Calculate the actual SHA-256 hashes of your binaries and update extension.toml
5. Install the extension in Zed as a development extension

A setup script is provided at `scripts/setup-agent-extension.sh` to help automate this process.