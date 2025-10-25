# Self-Update Implementation Summary

This document provides a comprehensive overview of the self-update mechanism implemented for vtcode.

## Overview

A robust self-update system has been implemented that allows vtcode to automatically check for, download, verify, and install updates from GitHub releases. The system includes backup management, rollback support, and cross-platform compatibility.

## Implementation Details

### Module Structure

The self-update functionality is implemented in `vtcode-core/src/update/` with the following components:

```
vtcode-core/src/update/
├── mod.rs              # Main module and UpdateManager
├── config.rs           # Configuration types and management
├── checker.rs          # Version checking and GitHub API integration
├── downloader.rs       # Download management with progress tracking
├── verifier.rs         # Binary verification (checksums, signatures)
├── installer.rs        # Installation and archive extraction
├── rollback.rs         # Backup and rollback management
└── README.md           # Module documentation
```

### Key Features

#### 1. Version Checking (`checker.rs`)

- Fetches latest release information from GitHub API
- Compares semantic versions
- Supports update channels (stable, beta, nightly)
- Configurable check frequency (always, daily, weekly, never)
- Caches check results to avoid rate limiting
- Platform-specific binary detection

#### 2. Secure Downloads (`downloader.rs`)

- Streaming downloads with progress tracking
- HTTPS-only connections
- Automatic download of checksums and signatures
- Configurable timeout settings
- Error handling and retry logic

#### 3. Binary Verification (`verifier.rs`)

- SHA256 checksum verification
- Signature verification (when available)
- Executable permission validation (Unix)
- Comprehensive error reporting

#### 4. Safe Installation (`installer.rs`)

- Archive extraction (ZIP, TAR, GZ, BZ2, XZ)
- Atomic executable replacement
- Platform-specific installation logic
- Automatic rollback on failure
- Executable permission management

#### 5. Backup Management (`rollback.rs`)

- Automatic backup creation before updates
- Timestamped backup files
- Configurable backup retention (default: 3)
- Easy rollback to previous versions
- Backup cleanup functionality

#### 6. Configuration (`config.rs`)

- Environment variable support
- Update channels (stable, beta, nightly)
- Update frequency settings
- Directory management
- GitHub API configuration

### CLI Integration

New CLI commands added to `vtcode-core/src/cli/update_commands.rs`:

```bash
vtcode update check              # Check for updates
vtcode update install            # Install updates
vtcode update config             # Configure settings
vtcode update backups            # List backups
vtcode update rollback           # Rollback to previous version
vtcode update cleanup            # Clean up old backups
```

### Configuration Options

#### Environment Variables

```bash
VTCODE_UPDATE_ENABLED=true              # Enable/disable updates
VTCODE_UPDATE_CHANNEL=stable            # Update channel
VTCODE_UPDATE_FREQUENCY=daily           # Check frequency
VTCODE_UPDATE_AUTO_DOWNLOAD=false       # Auto-download updates
VTCODE_UPDATE_AUTO_INSTALL=false        # Auto-install updates
VTCODE_UPDATE_DIR=~/.vtcode/updates     # Update directory
VTCODE_UPDATE_BACKUP_DIR=~/.vtcode/backups  # Backup directory
VTCODE_UPDATE_MAX_BACKUPS=3             # Maximum backups to keep
GITHUB_TOKEN=your_token                 # GitHub API token (optional)
```

#### Update Channels

- **Stable**: Thoroughly tested releases (default)
- **Beta**: Early access to new features
- **Nightly**: Latest development builds

#### Update Frequency

- **Always**: Check on every launch
- **Daily**: Check once per day (default)
- **Weekly**: Check once per week
- **Never**: Disable automatic checks

### Security Features

1. **Checksum Verification**
   - SHA256 checksums for all downloads
   - Automatic verification before installation
   - Fail-safe on mismatch

2. **Signature Verification**
   - Binary signature validation (when available)
   - Public key verification
   - Cryptographic authenticity checks

3. **HTTPS Downloads**
   - All downloads use HTTPS
   - Prevents man-in-the-middle attacks
   - Certificate validation

4. **Automatic Backups**
   - Backup before every update
   - Rollback support on failure
   - Safe update process

5. **Workspace Isolation**
   - Updates stored in isolated directories
   - No interference with running processes
   - Clean separation of concerns

### Cross-Platform Support

#### Supported Platforms

- **Linux**: x86_64, aarch64
- **macOS**: x86_64 (Intel), aarch64 (Apple Silicon)
- **Windows**: x86_64, aarch64

#### Platform-Specific Features

- **Unix**: Executable permission management
- **Windows**: Special handling for running executables
- **macOS**: Code signing support (future)

### Error Handling

Comprehensive error handling throughout:

1. **Network Errors**
   - Connection failures
   - Timeout handling
   - Rate limiting

2. **Verification Errors**
   - Checksum mismatches
   - Signature validation failures
   - Corrupted downloads

3. **Installation Errors**
   - Permission issues
   - Disk space problems
   - Automatic rollback

4. **Rollback Errors**
   - Manual recovery instructions
   - Backup validation
   - Safe failure modes

### Testing

Comprehensive test coverage:

1. **Unit Tests** (`vtcode-core/tests/update_tests.rs`)
   - Configuration parsing
   - Version comparison
   - Platform detection
   - Serialization/deserialization

2. **Integration Tests**
   - Full update workflow
   - Rollback scenarios
   - Error handling

3. **Example Code** (`examples/self_update_example.rs`)
   - Usage demonstrations
   - Configuration examples
   - Best practices

### Documentation

Complete documentation provided:

1. **User Guide** (`docs/guides/self-update.md`)
   - Quick start guide
   - Configuration instructions
   - Command reference
   - Troubleshooting

2. **Module Documentation** (`vtcode-core/src/update/README.md`)
   - Architecture overview
   - Component descriptions
   - Usage examples
   - Security details

3. **Inline Documentation**
   - Comprehensive rustdoc comments
   - Code examples
   - API documentation

### Dependencies Added

New dependencies in `vtcode-core/Cargo.toml`:

```toml
zip = "2.2"      # ZIP archive extraction
tar = "0.4"      # TAR archive extraction
bzip2 = "0.5"    # BZIP2 compression
xz2 = "0.1"      # XZ compression
```

Existing dependencies used:
- `reqwest`: HTTP client for downloads
- `sha2`: SHA256 checksum calculation
- `tokio`: Async runtime
- `serde`: Serialization
- `anyhow`: Error handling

## Usage Examples

### Basic Update Check

```rust
use vtcode_core::update::{UpdateConfig, UpdateManager};

let config = UpdateConfig::from_env()?;
let manager = UpdateManager::new(config)?;
let status = manager.check_for_updates().await?;

if status.update_available {
    println!("Update available: {}", status.latest_version.unwrap());
}
```

### Perform Update

```rust
let mut manager = UpdateManager::new(config)?;
let result = manager.perform_update().await?;

if result.success {
    println!("Updated from {} to {}", result.old_version, result.new_version);
}
```

### Configure Updates

```rust
let mut config = UpdateConfig::default();
config.channel = UpdateChannel::Beta;
config.frequency = UpdateFrequency::Weekly;
config.auto_download = true;
```

### Rollback

```rust
let manager = UpdateManager::new(config)?;
manager.rollback_to_backup(&backup_path)?;
```

## CLI Usage

```bash
# Check for updates
vtcode update check --verbose

# Install updates
vtcode update install -y

# Configure updates
vtcode update config --channel beta --frequency weekly

# List backups
vtcode update backups

# Rollback to previous version
vtcode update rollback

# Clean up old backups
vtcode update cleanup
```

## Future Enhancements

Potential improvements for future versions:

1. **Delta Updates**: Download only changed parts
2. **P2P Distribution**: Peer-to-peer update distribution
3. **Automatic Scheduling**: Background update checks
4. **Update Notifications**: Desktop notifications
5. **Version History**: Track update history
6. **Bandwidth Throttling**: Limit download speed
7. **Resume Downloads**: Resume interrupted downloads
8. **Multi-Source**: Download from multiple sources
9. **Code Signing**: macOS code signing support
10. **Update Analytics**: Track update success rates

## Integration Points

### Main Binary (`src/main.rs`)

Added update command handler:

```rust
Some(Commands::Update { command }) => {
    vtcode_core::cli::handle_update_command(command.clone()).await?;
}
```

### CLI Args (`vtcode-core/src/cli/args.rs`)

Added Update command to Commands enum:

```rust
Update {
    #[command(subcommand)]
    command: crate::cli::update_commands::UpdateCommands,
}
```

### Library Exports (`vtcode-core/src/lib.rs`)

Exported update types:

```rust
pub use update::{
    UpdateChannel, UpdateConfig, UpdateFrequency, 
    UpdateManager, UpdateResult, UpdateStatus,
};
```

## Best Practices

1. **Regular Updates**: Check for updates regularly
2. **Stable Channel**: Use stable channel for production
3. **Backup Management**: Keep multiple backups
4. **Test After Updates**: Verify functionality after updates
5. **Monitor Release Notes**: Review changes before updating
6. **Use GitHub Token**: Avoid rate limiting with authentication
7. **Secure Configuration**: Protect API tokens and credentials

## Troubleshooting

Common issues and solutions:

1. **Rate Limiting**: Use GitHub token
2. **Permission Errors**: Check file permissions
3. **Network Issues**: Check internet connection
4. **Verification Failures**: Re-download update
5. **Installation Failures**: Automatic rollback
6. **Rollback Issues**: Manual restoration from backup

## Conclusion

The self-update implementation provides a robust, secure, and user-friendly way to keep vtcode up to date. It includes comprehensive error handling, automatic backups, rollback support, and cross-platform compatibility. The system is fully integrated into the CLI and provides both programmatic and command-line interfaces for update management.

## See Also

- [Self-Update Guide](./guides/self-update.md)
- [Security Guide](./guides/security.md)
- [Installation Guide](./user-guide/installation.md)
- [Configuration Guide](./user-guide/configuration.md)
