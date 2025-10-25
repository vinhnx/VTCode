# Startup Update Check

VTCode automatically checks for updates when you launch the application, providing a seamless way to stay up to date.

## How It Works

### Automatic Check on Launch

When you start vtcode (without a specific command), it:

1. Checks if update checks are enabled
2. Verifies it's running in an interactive terminal
3. Checks for available updates (with a 5-second timeout)
4. Displays a prominent notification if an update is available
5. Prompts you to install the update

### Example Flow

```bash
$ vtcode

════════════════════════════════════════════════════════════════════════════════
  UPDATE AVAILABLE
════════════════════════════════════════════════════════════════════════════════

  Current version: 0.33.1
  Latest version:  0.34.0

  Release highlights:
    • New self-update system with automatic checks
    • Enhanced security features
    • Performance improvements
    • Bug fixes

  → Run 'vtcode update install' to update
════════════════════════════════════════════════════════════════════════════════

Would you like to install this update now? [y/N] y

  → Downloading update...
  ✓ Update installed successfully!
  → Updated from 0.33.1 to 0.34.0
  ℹ Backup created at: ~/.vtcode/backups/vtcode_backup_20250125_143022

  ⚠ Please restart vtcode to use the new version.
  → Run 'vtcode --version' to verify the update.
```

## Configuration

### Disable Startup Checks

To disable automatic update checks on startup:

```bash
# Temporarily disable
export VT_UPDATE_CHECK=false
vtcode

# Permanently disable (add to ~/.bashrc or ~/.zshrc)
echo 'export VT_UPDATE_CHECK=false' >> ~/.bashrc
```

### Configure Update Frequency

Control how often updates are checked:

```bash
# Check on every launch
vtcode update config --frequency always

# Check once per day (default)
vtcode update config --frequency daily

# Check once per week
vtcode update config --frequency weekly

# Never check automatically
vtcode update config --frequency never
```

### Enable Auto-Install

To automatically install updates without prompting:

```bash
vtcode update config --auto-install true
```

When auto-install is enabled, updates are installed automatically when detected:

```bash
$ vtcode

════════════════════════════════════════════════════════════════════════════════
  UPDATE AVAILABLE
════════════════════════════════════════════════════════════════════════════════

  Current version: 0.33.1
  Latest version:  0.34.0

  → Auto-install is enabled. Installing update...
  → Downloading update...
  ✓ Update installed successfully!
```

## Behavior in Different Scenarios

### Interactive Terminal

In an interactive terminal (normal usage):
- ✅ Update check runs
- ✅ Notification displayed
- ✅ User prompted for installation

### Non-Interactive Terminal

In non-interactive environments (pipes, scripts):
- ❌ Update check skipped
- No prompts or notifications

### CI/CD Environments

In CI/CD environments (detected automatically):
- ❌ Update check skipped
- No interference with automated workflows

Detected CI/CD environments:
- GitHub Actions
- GitLab CI
- CircleCI
- Travis CI
- Jenkins
- Any environment with `CI=true`

### With Specific Commands

When running specific commands:
- ❌ Update check skipped for most commands
- ✅ Update check runs only for interactive sessions

```bash
# Update check runs (interactive session)
vtcode

# Update check skipped (specific command)
vtcode ask "What is Rust?"
vtcode update check
vtcode analyze
```

## Timeout and Error Handling

### Timeout Protection

Update checks have a 5-second timeout to prevent blocking startup:

```rust
// Check with timeout
tokio::time::timeout(
    std::time::Duration::from_secs(5),
    manager.check_for_updates()
).await
```

If the check takes longer than 5 seconds, it's silently cancelled.

### Error Handling

All errors during update checks are handled gracefully:

- Network errors: Silently ignored
- GitHub API errors: Silently ignored
- Configuration errors: Silently ignored
- Timeout errors: Silently ignored

Your vtcode session starts normally regardless of update check status.

## User Experience

### Minimal Disruption

The update check is designed to be non-intrusive:

- ⚡ Fast: 5-second timeout ensures quick startup
- 🔇 Silent on errors: No error messages if check fails
- 🎯 Contextual: Only runs in interactive sessions
- 🚫 Skippable: Easy to disable or skip

### Clear Communication

When updates are available:

- 📢 Prominent notification with clear formatting
- 📝 Release highlights (first 5 lines)
- 🎨 Color-coded information
- ✅ Clear action items

### Safe Installation

When installing updates:

- 💾 Automatic backup before installation
- 🔄 Automatic rollback on failure
- ✓ Checksum verification
- 📍 Clear success/failure messages

## Advanced Configuration

### Custom Update Check Logic

You can customize when update checks run by modifying environment variables:

```bash
# Check only on weekdays
if [ $(date +%u) -lt 6 ]; then
    export VT_UPDATE_CHECK=true
else
    export VT_UPDATE_CHECK=false
fi

# Check only during work hours
hour=$(date +%H)
if [ $hour -ge 9 ] && [ $hour -le 17 ]; then
    export VT_UPDATE_CHECK=true
else
    export VT_UPDATE_CHECK=false
fi
```

### Logging Update Checks

Enable debug logging to see update check details:

```bash
export RUST_LOG=debug
vtcode
```

This will show:
- When update checks run
- Check results
- Any errors encountered

### Integration with Shell Prompt

Show update status in your shell prompt:

```bash
# Add to ~/.bashrc or ~/.zshrc
vtcode_update_status() {
    if command -v vtcode &> /dev/null; then
        if vtcode update check 2>/dev/null | grep -q "update is available"; then
            echo " [update available]"
        fi
    fi
}

# Add to PS1
PS1='$(vtcode_update_status)'$PS1
```

## Troubleshooting

### Update Check Not Running

If update checks aren't running:

1. Check if disabled:
   ```bash
   echo $VT_UPDATE_CHECK
   ```

2. Check configuration:
   ```bash
   vtcode update config
   ```

3. Verify you're in an interactive terminal:
   ```bash
   tty
   ```

4. Check for CI environment variables:
   ```bash
   env | grep CI
   ```

### Update Check Taking Too Long

If startup feels slow:

1. Check network connectivity:
   ```bash
   curl -I https://api.github.com
   ```

2. Disable update checks:
   ```bash
   export VT_UPDATE_CHECK=false
   ```

3. Increase timeout (requires code modification)

### Update Notification Not Showing

If you don't see update notifications:

1. Verify updates are available:
   ```bash
   vtcode update check --verbose
   ```

2. Check terminal color support:
   ```bash
   echo $TERM
   ```

3. Try with explicit color:
   ```bash
   vtcode --color always
   ```

## Best Practices

1. **Keep Checks Enabled**: Stay informed about updates
2. **Review Release Notes**: Check what's new before updating
3. **Test in Development**: Update dev environment first
4. **Use Stable Channel**: For production environments
5. **Enable Auto-Install**: For development environments
6. **Disable in CI/CD**: Already handled automatically
7. **Monitor Backups**: Keep recent backups available

## See Also

- [Self-Update Guide](./guides/self-update.md)
- [Quick Reference](./SELF_UPDATE_QUICK_REFERENCE.md)
- [Configuration Guide](./user-guide/configuration.md)
- [Troubleshooting Guide](./user-guide/troubleshooting.md)
