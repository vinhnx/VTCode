# Self-Update Module

This module provides a robust self-update mechanism for vtcode, allowing users to easily keep their installation up to date with the latest features and bug fixes.

## Architecture

The update system is composed of several key components:

### Core Components

1. **UpdateManager** (`mod.rs`)
   - Main coordinator for all update operations
   - Orchestrates checking, downloading, installing, and rolling back updates
   - Provides a high-level API for update operations

2. **UpdateChecker** (`checker.rs`)
   - Checks for available updates from GitHub releases
   - Compares versions and determines if an update is available
   - Handles update frequency and caching
   - Finds appropriate binary for the current platform

3. **UpdateDownloader** (`downloader.rs`)
   - Downloads update files from GitHub releases
   - Supports streaming downloads with progress tracking
   - Downloads checksums and signatures for verification

4. **UpdateVerifier** (`verifier.rs`)
   - Verifies downloaded binaries using SHA256 checksums
   - Validates signatures (when available)
   - Ensures executable permissions on Unix systems

5. **UpdateInstaller** (`installer.rs`)
   - Installs downloaded updates
   - Handles archive extraction (ZIP, TAR, etc.)
   - Replaces the current executable
   - Platform-specific installation logic

6. **RollbackManager** (`rollback.rs`)
   - Creates backups before updates
   - Manages backup retention
   - Provides rollback functionality
   - Cleans up old backups

7. **UpdateConfig** (`config.rs`)
   - Configuration for the update system
   - Update channels (stable, beta, nightly)
   - Update frequency settings
   - Directory management

## Features

### Version Checking

- Automatic version checking from GitHub releases
- Configurable check frequency (always, daily, weekly, never)
- Caching of check results to avoid rate limiting
- Support for pre-releases and beta channels

### Secure Downloads

- HTTPS downloads from GitHub releases
- SHA256 checksum verification
- Signature verification (when available)
- Streaming downloads with progress tracking
- Automatic retry on failure

### Safe Installation

- Automatic backup before installation
- Atomic replacement of executable
- Platform-specific installation logic
- Rollback on installation failure
- Verification of installed binary

### Backup Management

- Automatic backup creation
- Configurable backup retention
- Backup metadata tracking
- Easy rollback to previous versions

### Cross-Platform Support

- Linux (x86_64, aarch64)
- macOS (x86_64, aarch64)
- Windows (x86_64, aarch64)
- Automatic platform detection
- Platform-specific binary selection

## Usage

### Basic Usage

```rust
use vtcode_core::update::{UpdateConfig, UpdateManager};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create update manager with default configuration
    let config = UpdateConfig::from_env()?;
    let mut manager = UpdateManager::new(config)?;

    // Check for updates
    let status = manager.check_for_updates().await?;

    if status.update_available {
        println!("Update available: {}", status.latest_version.unwrap());

        // Perform update
        let result = manager.perform_update().await?;

        if result.success {
            println!("Update successful!");
        }
    }

    Ok(())
}
```

### Configuration

```rust
use vtcode_core::update::{UpdateChannel, UpdateConfig, UpdateFrequency};

let mut config = UpdateConfig::default();
config.channel = UpdateChannel::Beta;
config.frequency = UpdateFrequency::Weekly;
config.auto_download = true;
config.max_backups = 5;
```

### Environment Variables

```bash
export VTCODE_UPDATE_ENABLED=true
export VTCODE_UPDATE_CHANNEL=stable
export VTCODE_UPDATE_FREQUENCY=daily
export VTCODE_UPDATE_AUTO_DOWNLOAD=false
export VTCODE_UPDATE_MAX_BACKUPS=3
```

## Security

### Checksum Verification

All downloads are verified using SHA256 checksums:

1. Download the binary
2. Download the `.sha256` file
3. Calculate the SHA256 hash of the binary
4. Compare with the expected hash
5. Fail if hashes don't match

### Signature Verification

Binary signatures are verified when available:

1. Download the binary
2. Download the `.sig` file
3. Verify the signature using a public key
4. Fail if signature is invalid

### HTTPS Downloads

All downloads use HTTPS to prevent man-in-the-middle attacks.

### Automatic Backups

A backup is automatically created before each update, allowing rollback if needed.

## Error Handling

The update system implements comprehensive error handling:

- Network errors: Retry with exponential backoff
- Verification errors: Fail and cleanup
- Installation errors: Automatic rollback
- Rollback errors: Manual recovery instructions

## Testing

The module includes comprehensive tests:

- Unit tests for each component
- Integration tests for the full update workflow
- Platform-specific tests
- Error handling tests

Run tests with:

```bash
cargo test --package vtcode-core --lib update
```

## CLI Integration

The update system is integrated into the vtcode CLI:

```bash
# Check for updates
vtcode update check

# Install updates
vtcode update install

# Configure updates
vtcode update config --channel beta

# List backups
vtcode update backups

# Rollback
vtcode update rollback

# Cleanup old backups
vtcode update cleanup
```

## Future Enhancements

Potential future improvements:

1. Delta updates for faster downloads
2. Peer-to-peer distribution
3. Automatic update scheduling
4. Update notifications
5. Rollback to specific versions
6. Update history tracking
7. Bandwidth throttling
8. Resume interrupted downloads
9. Multi-source downloads
10. Update verification using multiple checksums

## Contributing

When contributing to the update module:

1. Follow the existing code style
2. Add tests for new functionality
3. Update documentation
4. Test on all supported platforms
5. Consider security implications
6. Handle errors gracefully

## See Also

- [Self-Update Guide](../../../docs/guides/self-update.md)
- [Security Guide](../../../docs/guides/security.md)
- [CLI Documentation](../cli/README.md)
