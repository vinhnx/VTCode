# Self-Update Migration Guide

This guide helps you migrate to the new self-update system and understand the changes.

## What's New

The self-update system introduces:

- ✅ Automatic version checking from GitHub releases
- ✅ Secure downloads with checksum verification
- ✅ Automatic backups before updates
- ✅ Rollback support for failed updates
- ✅ Cross-platform support (Linux, macOS, Windows)
- ✅ Configurable update channels and frequency
- ✅ CLI commands for update management

## For Existing Users

### No Action Required

The self-update system is **opt-in by default**. Your existing installation will continue to work without any changes.

### Enabling Self-Update

To start using the self-update feature:

```bash
# Check for updates
vtcode update check

# Install updates when available
vtcode update install
```

### Configuration

Configure update behavior:

```bash
# Enable automatic update checks
vtcode update config --enabled true

# Set check frequency
vtcode update config --frequency daily

# Choose update channel
vtcode update config --channel stable
```

## For New Users

### First-Time Setup

1. Install vtcode using your preferred method
2. The self-update system is ready to use immediately
3. Check for updates: `vtcode update check`

### Recommended Configuration

```bash
# Enable daily update checks
vtcode update config --enabled true --frequency daily

# Use stable channel
vtcode update config --channel stable

# Keep 3 backups (default)
export VTCODE_UPDATE_MAX_BACKUPS=3
```

## Migration from Manual Updates

### Before (Manual Updates)

```bash
# Download new version manually
curl -L https://github.com/vinhnx/vtcode/releases/latest/download/vtcode-linux -o vtcode

# Replace binary manually
sudo mv vtcode /usr/local/bin/vtcode
sudo chmod +x /usr/local/bin/vtcode

# No backup, no rollback
```

### After (Self-Update)

```bash
# Check and install in one command
vtcode update check
vtcode update install

# Automatic backup created
# Automatic rollback on failure
# Checksum verification included
```

## Migration from Package Managers

### Homebrew Users

If you installed via Homebrew:

```bash
# Option 1: Continue using Homebrew
brew upgrade vtcode

# Option 2: Switch to self-update
# Uninstall Homebrew version
brew uninstall vtcode

# Install standalone version
# Then use self-update
vtcode update install
```

### APT/DNF Users

If you installed via package manager:

```bash
# Option 1: Continue using package manager
sudo apt update && sudo apt upgrade vtcode

# Option 2: Switch to self-update
# Uninstall package manager version
sudo apt remove vtcode

# Install standalone version
# Then use self-update
vtcode update install
```

## Breaking Changes

### None

The self-update system is **fully backward compatible**. No breaking changes to existing functionality.

### New Features Only

All changes are additive:
- New CLI commands under `vtcode update`
- New configuration options
- New environment variables
- No changes to existing commands

## Configuration Migration

### Environment Variables

New environment variables (all optional):

```bash
# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)
export VTCODE_UPDATE_ENABLED=true
export VTCODE_UPDATE_CHANNEL=stable
export VTCODE_UPDATE_FREQUENCY=daily
export VTCODE_UPDATE_MAX_BACKUPS=3
```

### Configuration File

No changes to `vtcode.toml` required. Update settings are managed separately via:
- Environment variables
- CLI commands (`vtcode update config`)

## Directory Structure

### New Directories

The self-update system creates:

```
~/.vtcode/
├── updates/              # Downloaded updates
│   ├── last_check.json  # Update check cache
│   └── vtcode-*         # Downloaded binaries
└── backups/             # Backup versions
    └── vtcode_backup_*  # Timestamped backups
```

### Cleanup

Old update files are automatically cleaned up. To manually clean:

```bash
# Remove old backups
vtcode update cleanup

# Remove update cache
rm -rf ~/.vtcode/updates/
```

## Security Considerations

### Enhanced Security

The self-update system provides:

1. **Checksum Verification**: SHA256 checksums for all downloads
2. **Signature Verification**: Binary signatures (when available)
3. **HTTPS Only**: All downloads use HTTPS
4. **Automatic Backups**: Rollback support on failure

### GitHub Token (Optional)

For private repositories or to avoid rate limiting:

```bash
# Create a GitHub personal access token
# https://github.com/settings/tokens

# Add to environment
export GITHUB_TOKEN=your_token_here
```

## Rollback Procedure

### Automatic Rollback

Installation failures trigger automatic rollback:

```bash
vtcode update install
# If installation fails, automatically rolls back
```

### Manual Rollback

If needed, manually rollback:

```bash
# List available backups
vtcode update backups

# Rollback to latest backup
vtcode update rollback

# Or rollback to specific backup
vtcode update rollback ~/.vtcode/backups/vtcode_backup_20250125_143022
```

## Testing the Migration

### Verify Installation

```bash
# Check current version
vtcode --version

# Check for updates
vtcode update check

# List available commands
vtcode update --help
```

### Test Update Process

```bash
# Dry run (check only)
vtcode update check --verbose

# Install with confirmation
vtcode update install

# Verify new version
vtcode --version
```

### Test Rollback

```bash
# List backups
vtcode update backups

# Test rollback (if you have backups)
vtcode update rollback

# Verify version
vtcode --version
```

## Troubleshooting Migration

### Update Commands Not Found

```bash
# Ensure you're running the latest version
vtcode --version

# Update to latest version manually first
# Then self-update will be available
```

### Permission Issues

```bash
# Unix: Ensure executable permissions
chmod +x $(which vtcode)

# Check file ownership
ls -la $(which vtcode)
```

### Network Issues

```bash
# Test GitHub connectivity
curl -I https://api.github.com

# Use verbose logging
export RUST_LOG=debug
vtcode update check
```

### Rate Limiting

```bash
# Use GitHub token
export GITHUB_TOKEN=your_token
vtcode update check
```

## Best Practices After Migration

1. **Enable Update Checks**
   ```bash
   vtcode update config --enabled true --frequency daily
   ```

2. **Keep Backups**
   ```bash
   export VTCODE_UPDATE_MAX_BACKUPS=3
   ```

3. **Use Stable Channel**
   ```bash
   vtcode update config --channel stable
   ```

4. **Test Updates First**
   ```bash
   # In development environment
   vtcode update install
   # Test functionality
   # Then update production
   ```

5. **Monitor Release Notes**
   ```bash
   vtcode update check --verbose
   ```

## FAQ

### Q: Will this break my existing installation?

**A:** No. The self-update system is fully backward compatible and opt-in.

### Q: Can I continue using my package manager?

**A:** Yes. You can continue using Homebrew, APT, etc. The self-update system is optional.

### Q: What happens to my configuration?

**A:** Your existing `vtcode.toml` and settings are unchanged. Update settings are separate.

### Q: How do I disable self-update?

**A:** Set `VTCODE_UPDATE_ENABLED=false` or `vtcode update config --enabled false --frequency never`

### Q: Can I rollback to any previous version?

**A:** You can rollback to any backup in `~/.vtcode/backups/`. By default, 3 backups are kept.

### Q: Is my data safe during updates?

**A:** Yes. Updates only replace the binary. Your data, configuration, and projects are untouched.

### Q: What if an update fails?

**A:** The system automatically rolls back to your previous version. No manual intervention needed.

### Q: Can I use this in CI/CD?

**A:** Yes. Use `vtcode update install -y` for non-interactive updates.

## Support

If you encounter issues during migration:

1. Check the [Troubleshooting Guide](./user-guide/troubleshooting.md)
2. Review [Self-Update Guide](./guides/self-update.md)
3. Open an issue on [GitHub](https://github.com/vinhnx/vtcode/issues)
4. Join the community discussion

## Feedback

We'd love to hear your feedback on the self-update system:

- What works well?
- What could be improved?
- Any issues encountered?

Please share your experience on GitHub or in community discussions.

## See Also

- [Self-Update Guide](./guides/self-update.md)
- [Quick Reference](./SELF_UPDATE_QUICK_REFERENCE.md)
- [Implementation Details](./SELF_UPDATE_IMPLEMENTATION.md)
- [Security Guide](./guides/security.md)
