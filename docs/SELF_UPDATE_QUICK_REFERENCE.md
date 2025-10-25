# Self-Update Quick Reference

Quick reference for vtcode self-update commands and configuration.

## Commands

### Check for Updates

```bash
vtcode update check              # Basic check
vtcode update check --verbose    # Detailed information
```

### Install Updates

```bash
vtcode update install            # Install with confirmation
vtcode update install -y         # Install without confirmation
vtcode update install --force    # Force reinstall
```

### Configure Updates

```bash
vtcode update config                           # View current config
vtcode update config --enabled true            # Enable updates
vtcode update config --channel beta            # Set channel
vtcode update config --frequency weekly        # Set frequency
vtcode update config --auto-download true      # Enable auto-download
```

### Manage Backups

```bash
vtcode update backups            # List all backups
vtcode update rollback           # Rollback to latest backup
vtcode update rollback <path>    # Rollback to specific backup
vtcode update cleanup            # Remove old backups
```

## Environment Variables

```bash
# Enable/disable startup update checks
export VT_UPDATE_CHECK=true

# Enable/disable updates
export VTCODE_UPDATE_ENABLED=true

# Update channel (stable, beta, nightly)
export VTCODE_UPDATE_CHANNEL=stable

# Check frequency (always, daily, weekly, never)
export VTCODE_UPDATE_FREQUENCY=daily

# Auto-download updates
export VTCODE_UPDATE_AUTO_DOWNLOAD=false

# Auto-install updates
export VTCODE_UPDATE_AUTO_INSTALL=false

# Custom directories
export VTCODE_UPDATE_DIR=~/.vtcode/updates
export VTCODE_UPDATE_BACKUP_DIR=~/.vtcode/backups

# Maximum backups to keep
export VTCODE_UPDATE_MAX_BACKUPS=3

# GitHub API token (optional, for rate limiting)
export GITHUB_TOKEN=your_token_here
```

## Update Channels

| Channel | Description | Use Case |
|---------|-------------|----------|
| `stable` | Thoroughly tested releases | Production use (default) |
| `beta` | Early access to new features | Testing new features |
| `nightly` | Latest development builds | Cutting-edge features |

## Update Frequency

| Frequency | Description |
|-----------|-------------|
| `always` | Check on every launch |
| `daily` | Check once per day (default) |
| `weekly` | Check once per week |
| `never` | Disable automatic checks |

## Common Workflows

### Standard Update

```bash
# 1. Check for updates
vtcode update check

# 2. Install if available
vtcode update install

# 3. Restart vtcode
vtcode --version
```

### Beta Testing

```bash
# Switch to beta channel
vtcode update config --channel beta

# Check for beta updates
vtcode update check

# Install beta version
vtcode update install -y
```

### Rollback After Issues

```bash
# List available backups
vtcode update backups

# Rollback to previous version
vtcode update rollback

# Verify version
vtcode --version
```

### Automated Updates (CI/CD)

```bash
#!/bin/bash
# Check and install updates automatically

if vtcode update check | grep -q "update is available"; then
    vtcode update install -y
    echo "Updated to $(vtcode --version)"
fi
```

## Troubleshooting

### Rate Limiting

```bash
# Use GitHub token
export GITHUB_TOKEN=your_token
vtcode update check
```

### Permission Errors

```bash
# Unix: Set executable permissions
chmod +x $(which vtcode)

# Windows: Run as administrator
```

### Network Issues

```bash
# Check connectivity
curl -I https://api.github.com

# Use verbose logging
export RUST_LOG=debug
vtcode update check
```

### Verification Failures

```bash
# Re-download update
vtcode update install --force
```

### Installation Failures

```bash
# Automatic rollback occurs
# Check backup directory
ls -la ~/.vtcode/backups/

# Manual rollback if needed
vtcode update rollback
```

## File Locations

| Item | Location |
|------|----------|
| Updates | `~/.vtcode/updates/` |
| Backups | `~/.vtcode/backups/` |
| Cache | `~/.vtcode/updates/last_check.json` |
| Checksums | `~/.vtcode/updates/*.sha256` |
| Signatures | `~/.vtcode/updates/*.sig` |

## Security Checklist

- ✅ HTTPS downloads only
- ✅ SHA256 checksum verification
- ✅ Signature verification (when available)
- ✅ Automatic backups before updates
- ✅ Rollback on installation failure
- ✅ Isolated update directories

## Best Practices

1. **Use stable channel** for production
2. **Keep backups** (don't set max_backups to 0)
3. **Test after updates** in development first
4. **Review release notes** before updating
5. **Use GitHub token** to avoid rate limiting
6. **Monitor update logs** for issues
7. **Keep vtcode updated** for security fixes

## Quick Tips

- Use `-y` flag to skip confirmations in scripts
- Use `--verbose` to see release notes
- Check `~/.vtcode/backups/` for manual recovery
- Set `RUST_LOG=debug` for detailed logging
- Use `--force` to reinstall current version
- Configure frequency to `never` to disable auto-checks

## See Also

- [Full Self-Update Guide](./guides/self-update.md)
- [Security Guide](./guides/security.md)
- [Configuration Guide](./user-guide/configuration.md)
- [Troubleshooting Guide](./user-guide/troubleshooting.md)
