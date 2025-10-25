# Self-Update Guide

VTCode includes a robust self-update mechanism that allows you to easily keep your installation up to date with the latest features and bug fixes.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Commands](#commands)
- [Update Channels](#update-channels)
- [Security](#security)
- [Backup and Rollback](#backup-and-rollback)
- [Troubleshooting](#troubleshooting)

## Overview

The self-update system provides:

- **Automatic startup checks**: Checks for updates when you launch vtcode
- **Interactive prompts**: Prompts you to install updates when available
- **Automatic version checking** from GitHub releases
- **Secure downloads** with checksum and signature verification
- **Automatic backups** before updates
- **Rollback support** for failed updates
- **Cross-platform support** (Linux, macOS, Windows)
- **Configurable update channels** (stable, beta, nightly)
- **Flexible update frequency** settings

## Quick Start

### Automatic Startup Checks

When you launch vtcode, it automatically checks for updates (respecting your configured frequency). If an update is available, you'll see a prominent notification:

```
════════════════════════════════════════════════════════════════════════════════
  UPDATE AVAILABLE
════════════════════════════════════════════════════════════════════════════════

  Current version: 0.33.1
  Latest version:  0.34.0

  Release highlights:
    • New self-update system
    • Improved security features
    • Bug fixes and performance improvements

  → Run 'vtcode update install' to update
════════════════════════════════════════════════════════════════════════════════

Would you like to install this update now? [y/N]
```

You can disable startup checks by setting:
```bash
export VT_UPDATE_CHECK=false
```

### Manual Check for Updates

```bash
vtcode update check
```

This will check if a new version is available and display information about it.

### Install Updates

```bash
vtcode update install
```

This will download and install the latest version. A backup of your current version will be created automatically.

### Install Without Confirmation

```bash
vtcode update install -y
```

Skip the confirmation prompt and install immediately.

## Configuration

### Environment Variables

You can configure the update system using environment variables:

```bash
# Enable or disable startup update checks
export VT_UPDATE_CHECK=true

# Enable or disable automatic updates
export VTCODE_UPDATE_ENABLED=true

# Set update channel (stable, beta, nightly)
export VTCODE_UPDATE_CHANNEL=stable

# Set update frequency (always, daily, weekly, never)
export VTCODE_UPDATE_FREQUENCY=daily

# Enable automatic downloads
export VTCODE_UPDATE_AUTO_DOWNLOAD=false

# Enable automatic installation
export VTCODE_UPDATE_AUTO_INSTALL=false

# Custom update directory
export VTCODE_UPDATE_DIR=~/.vtcode/updates

# Custom backup directory
export VTCODE_UPDATE_BACKUP_DIR=~/.vtcode/backups

# Maximum number of backups to keep
export VTCODE_UPDATE_MAX_BACKUPS=3

# GitHub API token for authenticated requests (optional)
export GITHUB_TOKEN=your_token_here
```

### Using the Config Command

```bash
# View current configuration
vtcode update config

# Enable automatic updates
vtcode update config --enabled true

# Set update channel to beta
vtcode update config --channel beta

# Set update frequency to weekly
vtcode update config --frequency weekly

# Enable automatic downloads
vtcode update config --auto-download true
```

## Commands

### Check for Updates

```bash
vtcode update check
```

Check if a new version is available.

Options:
- `-v, --verbose`: Show detailed information including release notes

Example:
```bash
vtcode update check --verbose
```

### Install Updates

```bash
vtcode update install
```

Download and install the latest version.

Options:
- `-y, --yes`: Skip confirmation prompt
- `-f, --force`: Force reinstall even if no update is available

Examples:
```bash
# Install with confirmation
vtcode update install

# Install without confirmation
vtcode update install -y

# Force reinstall current version
vtcode update install --force
```

### Configure Updates

```bash
vtcode update config [OPTIONS]
```

Configure update settings.

Options:
- `--enabled <BOOL>`: Enable or disable automatic updates
- `--channel <CHANNEL>`: Set update channel (stable, beta, nightly)
- `--frequency <FREQ>`: Set update frequency (always, daily, weekly, never)
- `--auto-download <BOOL>`: Enable or disable automatic downloads
- `--auto-install <BOOL>`: Enable or disable automatic installation

Examples:
```bash
# View current configuration
vtcode update config

# Enable automatic updates
vtcode update config --enabled true

# Set to beta channel
vtcode update config --channel beta

# Check for updates daily
vtcode update config --frequency daily
```

### List Backups

```bash
vtcode update backups
```

List all available backups with their sizes and modification times.

### Rollback to Previous Version

```bash
vtcode update rollback [BACKUP]
```

Rollback to a previous version.

Options:
- `BACKUP`: Optional path to specific backup file. If not provided, rolls back to the most recent backup.

Examples:
```bash
# Rollback to most recent backup
vtcode update rollback

# Rollback to specific backup
vtcode update rollback ~/.vtcode/backups/vtcode_backup_20250125_143022
```

### Clean Up Old Backups

```bash
vtcode update cleanup
```

Remove old backups beyond the configured maximum (default: 3).

## Update Channels

VTCode supports three update channels:

### Stable (Default)

The stable channel provides thoroughly tested releases with the highest reliability.

```bash
vtcode update config --channel stable
```

### Beta

The beta channel provides early access to new features before they reach stable.

```bash
vtcode update config --channel beta
```

### Nightly

The nightly channel provides the latest development builds with cutting-edge features.

```bash
vtcode update config --channel nightly
```

## Security

The self-update system implements multiple security measures:

### Checksum Verification

All downloads are verified using SHA256 checksums to ensure integrity.

```bash
# Checksums are verified automatically
vtcode update install
```

### Signature Verification

Binary signatures are verified to ensure authenticity (when available).

### HTTPS Downloads

All downloads use HTTPS to prevent man-in-the-middle attacks.

### Automatic Backups

A backup is automatically created before each update, allowing you to rollback if needed.

### Workspace Isolation

Update files are stored in isolated directories to prevent conflicts.

## Backup and Rollback

### Automatic Backups

Before each update, VTCode automatically creates a backup of your current version:

```
~/.vtcode/backups/vtcode_backup_YYYYMMDD_HHMMSS
```

### Manual Rollback

If an update causes issues, you can easily rollback:

```bash
# Rollback to most recent backup
vtcode update rollback

# List available backups
vtcode update backups

# Rollback to specific backup
vtcode update rollback ~/.vtcode/backups/vtcode_backup_20250125_143022
```

### Backup Management

VTCode automatically manages backups:

- Keeps the 3 most recent backups by default
- Automatically removes older backups
- Configurable maximum backup count

```bash
# Set maximum backups to keep
export VTCODE_UPDATE_MAX_BACKUPS=5

# Clean up old backups manually
vtcode update cleanup
```

## Troubleshooting

### Update Check Fails

If update checking fails:

1. Check your internet connection
2. Verify GitHub is accessible
3. Check if you're behind a proxy or firewall
4. Try using a GitHub token for authenticated requests:

```bash
export GITHUB_TOKEN=your_token_here
vtcode update check
```

### Download Fails

If download fails:

1. Check available disk space
2. Verify write permissions to update directory
3. Try again with verbose logging:

```bash
export RUST_LOG=debug
vtcode update install
```

### Installation Fails

If installation fails:

1. The system will automatically rollback to your previous version
2. Check the error message for details
3. Verify you have write permissions to the installation directory
4. Try running with elevated privileges if needed

### Rollback Fails

If rollback fails:

1. Manually restore from backup:

```bash
# Find your backup
ls -la ~/.vtcode/backups/

# Copy backup to installation location
cp ~/.vtcode/backups/vtcode_backup_YYYYMMDD_HHMMSS $(which vtcode)

# Set executable permissions (Unix)
chmod +x $(which vtcode)
```

### Permission Errors

On Unix systems, you may need to set executable permissions:

```bash
chmod +x ~/.vtcode/updates/vtcode
```

On Windows, you may need to run as administrator.

### GitHub Rate Limiting

If you encounter rate limiting:

1. Use a GitHub personal access token:

```bash
export GITHUB_TOKEN=your_token_here
```

2. Reduce update frequency:

```bash
vtcode update config --frequency weekly
```

## Advanced Usage

### Custom GitHub API Base URL

For GitHub Enterprise installations:

```bash
export GITHUB_API_BASE=https://github.company.com/api/v3
vtcode update check
```

### Custom Update Directory

```bash
export VTCODE_UPDATE_DIR=/custom/path/updates
vtcode update install
```

### Disable Automatic Updates

```bash
vtcode update config --enabled false --frequency never
```

### Automated Updates in CI/CD

```bash
#!/bin/bash
# Update vtcode in CI/CD pipeline

# Check for updates
if vtcode update check | grep -q "update is available"; then
    # Install update without confirmation
    vtcode update install -y
    
    # Verify installation
    vtcode --version
fi
```

## Best Practices

1. **Regular Updates**: Keep vtcode up to date for the latest features and security fixes
2. **Backup Before Major Updates**: Consider manual backups before major version updates
3. **Test After Updates**: Verify functionality after updates, especially in production
4. **Use Stable Channel**: Use the stable channel for production environments
5. **Monitor Release Notes**: Review release notes before updating
6. **Keep Backups**: Don't disable automatic backups
7. **Use GitHub Token**: Use a GitHub token to avoid rate limiting

## See Also

- [Installation Guide](../user-guide/installation.md)
- [Configuration Guide](../user-guide/configuration.md)
- [Security Guide](./security.md)
- [Troubleshooting Guide](../user-guide/troubleshooting.md)
