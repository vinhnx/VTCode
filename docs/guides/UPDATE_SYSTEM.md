# VT Code Update System Guide

This guide covers VT Code's update system, including release channels, version pinning, and configuration options.

## Overview

VT Code includes a built-in update system that can check for and install updates from GitHub Releases. The system supports:

- **Multiple release channels** (stable, beta, nightly)
- **Version pinning** for staying on specific versions
- **Configurable update behavior** via `~/.vtcode/update.toml`
- **Download mirrors** for better availability

## Quick Start

### Check for Updates

```bash
# Check and install updates
vtcode update

# Check only (don't install)
vtcode update --check
```

### List Available Versions

```bash
# List recent versions
vtcode update --list

# List more versions
vtcode update --list --limit 20
```

### Pin to a Specific Version

```bash
# Pin to a specific version
vtcode update --pin 0.85.3

# Unpin and resume following the channel
vtcode update --unpin
```

### Release Channels

```bash
# Switch to beta channel
vtcode update --channel beta

# Switch to nightly channel
vtcode update --channel nightly

# Back to stable
vtcode update --channel stable
```

### View Configuration

```bash
# Show current update configuration
vtcode update --show-config
```

## Release Channels

VT Code follows three release channels:

### Stable (Default)

- **Purpose**: Production-ready releases
- **Update frequency**: Every 2-4 weeks
- **Recommended for**: Most users, production environments
- **Quality**: Fully tested and validated

```toml
# ~/.vtcode/update.toml
channel = "stable"
```

### Beta

- **Purpose**: Pre-release testing
- **Update frequency**: Weekly
- **Recommended for**: Early adopters, testing new features
- **Quality**: Feature-complete, minor bugs possible

```toml
# ~/.vtcode/update.toml
channel = "beta"
```

### Nightly

- **Purpose**: Bleeding edge builds
- **Update frequency**: Daily (automated builds)
- **Recommended for**: Contributors, extreme early adopters
- **Quality**: May contain bugs, rapid iteration

```toml
# ~/.vtcode/update.toml
channel = "nightly"
```

## Version Pinning

Version pinning allows you to stay on a specific version, disabling automatic updates until you unpin.

### When to Pin

- Waiting for a bug fix in the next release
- Stability requirements for production
- Testing compatibility with specific version
- Avoiding a problematic release

### Pin to a Version

```bash
vtcode update --pin 0.85.3
```

This creates/updates `~/.vtcode/update.toml`:

```toml
[pin]
version = "0.85.3"
```

### Unpin

```bash
vtcode update --unpin
```

## Configuration File

Location: `~/.vtcode/update.toml`

### Example Configuration

```toml
# VT Code Update Configuration

# Release channel to follow
# Options: stable (default), beta, nightly
channel = "stable"

# Version pinning (optional)
# Uncomment to pin to a specific version
# [pin]
# version = "0.85.3"
# reason = "Waiting for bug fix in next release"
# auto_unpin = false

# Download mirrors (optional)
# [mirrors]
# primary = "https://github.com/vinhnx/vtcode/releases"
# fallbacks = [
#     "https://mirror.example.com/vtcode",
# ]
# geo_select = true

# Auto-update check interval in hours (0 = disable)
check_interval_hours = 24

# Download timeout in seconds
download_timeout_secs = 300

# Keep backup of previous version after update
keep_backup = true

# Auto-rollback on startup if new version fails
auto_rollback = false
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `channel` | String | `"stable"` | Release channel: `stable`, `beta`, or `nightly` |
| `pin.version` | String | `null` | Pinned version (null = follow channel) |
| `pin.reason` | String | `null` | User note for pinning |
| `pin.auto_unpin` | Boolean | `false` | Auto-remove pin after successful update |
| `mirrors.primary` | String | GitHub | Primary download mirror URL |
| `mirrors.fallbacks` | Array | `[]` | Fallback mirror URLs |
| `mirrors.geo_select` | Boolean | `true` | Enable geographic mirror selection |
| `check_interval_hours` | Integer | `24` | Hours between update checks (0 = disable) |
| `download_timeout_secs` | Integer | `300` | Download timeout in seconds |
| `keep_backup` | Boolean | `true` | Keep backup of previous version |
| `auto_rollback` | Boolean | `false` | Auto-rollback on startup failure |

## CLI Reference

### `vtcode update`

Check for and install updates.

**Options:**
- `--check` - Check only, don't install
- `--force` - Force reinstall even if up-to-date
- `--list` - List available versions
- `--limit <N>` - Number of versions to list (default: 10)
- `--pin <VERSION>` - Pin to specific version
- `--unpin` - Remove version pin
- `--channel <CHANNEL>` - Set release channel
- `--show-config` - Show current configuration

**Examples:**

```bash
# Check and install
vtcode update

# Check only
vtcode update --check

# List versions
vtcode update --list

# Pin version
vtcode update --pin 0.85.3

# Switch to beta
vtcode update --channel beta
```

## Update Behavior

### Managed Installs

If VT Code was installed via a package manager, the update system will detect this and provide the appropriate update command:

- **Homebrew**: `brew upgrade vinhnx/tap/vtcode`
- **Cargo**: `cargo install vtcode --force`
- **npm**: `npm install -g vtcode@latest`
- **Standalone**: Direct update via `vtcode update`

### Backup and Rollback

When `keep_backup = true` (default), the previous version is kept after update. If `auto_rollback = true`, VT Code will automatically revert to the backup if the new version fails to start.

### Update Checks

By default, VT Code checks for updates every 24 hours. The check timestamp is cached in `~/.cache/vtcode/last_update_check`.

To disable automatic checks:

```toml
check_interval_hours = 0
```

## Troubleshooting

### Update Fails to Download

1. Check internet connectivity
2. Try a different mirror:
   ```toml
   [mirrors]
   primary = "https://mirror.example.com/vtcode"
   ```
3. Increase timeout:
   ```toml
   download_timeout_secs = 600
   ```

### Binary Permissions Issues

On Unix systems, ensure the binary has execute permissions:

```bash
chmod +x ~/.local/bin/vtcode
```

### Rollback to Previous Version

If an update causes issues:

1. **Manual rollback**: Download previous version from GitHub Releases
2. **Auto-rollback**: If enabled, happens automatically on startup failure
   ```toml
   auto_rollback = true
   ```

### Check Current Version

```bash
vtcode --version
```

### Force Update

If you suspect corruption:

```bash
vtcode update --force
```

## Integration with CI/CD

For automated environments, you can:

1. **Pin versions** to ensure consistency:
   ```bash
   vtcode update --pin 0.85.3
   ```

2. **Disable auto-checks**:
   ```toml
   check_interval_hours = 0
   ```

3. **Use specific channels** for testing:
   ```toml
   channel = "beta"
   ```

## Security Considerations

- Updates are downloaded from GitHub Releases over HTTPS
- Binary signatures are verified automatically
- Backup versions are kept for rollback safety
- Configuration file is user-controlled (`~/.vtcode/update.toml`)

## Related Documentation

- [Installation Guide](../installation/README.md)
- [Configuration Precedence](../config/CONFIGURATION_PRECEDENCE.md)
- [Release Notes](https://github.com/vinhnx/vtcode/releases)
