# /update Slash Command

The `/update` slash command allows you to check for vtcode updates directly from within a chat session.

## Usage

```
/update [check|install|status]
```

## Subcommands

### `/update` or `/update check`

Check if a new version of vtcode is available.

**Example:**
```
/update
```

**Output:**
```
Checking for updates...
Current version: 0.33.1
Latest version:  0.34.0
An update is available!
Release highlights:
  • New self-update system
  • Enhanced security features
  • Bug fixes
Run '/update install' or 'vtcode update install' to install the update.
```

### `/update install`

Displays instructions for installing updates.

**Note:** Installing updates from within a session is not recommended. The command will guide you to exit and run the update from your terminal.

**Example:**
```
/update install
```

**Output:**
```
Installing updates from within a session is not recommended.
Please exit and run 'vtcode update install' from your terminal.
This ensures a clean update process and proper restart.
```

### `/update status`

Display the current update system configuration.

**Example:**
```
/update status
```

**Output:**
```
Update system status:
  Enabled: true
  Channel: stable
  Frequency: Daily
  Auto-download: false
  Auto-install: false
```

## When to Use

### Check for Updates During Development

```
User: /update
Agent: Checking for updates...
        Current version: 0.33.1
        Latest version:  0.34.0
        An update is available!
```

### Verify Update Configuration

```
User: /update status
Agent: Update system status:
        Enabled: true
        Channel: stable
        ...
```

### Quick Update Check

Instead of exiting your session and running `vtcode update check`, you can quickly check from within the chat:

```
User: /update
```

## Comparison with CLI Commands

| Slash Command | CLI Equivalent | Notes |
|---------------|----------------|-------|
| `/update` | `vtcode update check` | Check for updates |
| `/update check` | `vtcode update check --verbose` | Detailed check |
| `/update status` | `vtcode update config` | View configuration |
| `/update install` | `vtcode update install` | Must exit session first |

## Best Practices

1. **Check Regularly**: Use `/update` periodically to stay informed about new versions
2. **Exit Before Installing**: Always exit your session before installing updates
3. **Review Release Notes**: Check the release highlights before updating
4. **Use Status**: Verify your update configuration with `/update status`

## Integration with Startup Checks

The `/update` command complements the automatic startup update checks:

- **Startup checks**: Automatic when launching vtcode
- **`/update` command**: Manual check during a session

Both use the same update system and configuration.

## Examples

### Example 1: Quick Update Check

```
User: /update

Agent: Checking for updates...
       Current version: 0.33.1
       Latest version:  0.34.0
       An update is available!
       Release highlights:
         • New self-update system
         • Enhanced security features
       Run '/update install' or 'vtcode update install' to install the update.
```

### Example 2: No Update Available

```
User: /update

Agent: Checking for updates...
       Current version: 0.34.0
       Latest version:  0.34.0
       You are running the latest version.
```

### Example 3: Check Configuration

```
User: /update status

Agent: Update system status:
         Enabled: true
         Channel: stable
         Frequency: Daily
         Auto-download: false
         Auto-install: false
```

### Example 4: Install Guidance

```
User: /update install

Agent: Installing updates from within a session is not recommended.
       Please exit and run 'vtcode update install' from your terminal.
       This ensures a clean update process and proper restart.
```

## Error Handling

### Network Error

```
User: /update

Agent: Checking for updates...
       Failed to check for updates: Network error
```

### Configuration Error

```
User: /update status

Agent: Update system status:
       Failed to load configuration: Invalid configuration
```

## Tips

1. **Combine with /status**: Use `/status` to see your current version, then `/update` to check for updates
2. **Use in Scripts**: The `/update` command can be used in automated workflows
3. **Check Before Long Sessions**: Run `/update` before starting a long coding session
4. **Verify After Update**: After updating, use `/update` to confirm the new version

## See Also

- [Self-Update Guide](./guides/self-update.md)
- [Startup Update Check](./STARTUP_UPDATE_CHECK.md)
- [Quick Reference](./SELF_UPDATE_QUICK_REFERENCE.md)
- [Slash Commands Guide](./user-guide/slash-commands.md)
