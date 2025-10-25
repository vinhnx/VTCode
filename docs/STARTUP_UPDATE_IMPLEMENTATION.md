# Startup Update Check Implementation

This document describes the implementation of automatic update checks on application startup.

## Overview

The startup update check feature automatically verifies if newer versions of vtcode are available when the application launches. When updates are detected, it displays a prominent notification and provides an interactive prompt for users to install the update.

## Implementation

### Module Structure

The startup update check is implemented in `src/startup/update_check.rs`:

```rust
src/startup/
├── mod.rs              # Startup module
├── first_run.rs        # First-run setup
└── update_check.rs     # Update check on startup (NEW)
```

### Key Functions

#### 1. `check_for_updates_on_startup()`

Main entry point for startup update checks:

```rust
pub async fn check_for_updates_on_startup() -> Result<()>
```

**Responsibilities:**
- Checks if update checks are enabled
- Creates update manager
- Checks for updates with timeout
- Displays notification if available
- Prompts user for installation

**Features:**
- 5-second timeout to prevent blocking startup
- Graceful error handling (silent failures)
- Respects configuration settings

#### 2. `display_update_notification()`

Displays a prominent update notification:

```rust
fn display_update_notification(status: &UpdateStatus) -> Result<()>
```

**Output Example:**
```
════════════════════════════════════════════════════════════════════════════════
  UPDATE AVAILABLE
════════════════════════════════════════════════════════════════════════════════

  Current version: 0.33.1
  Latest version:  0.34.0

  Release highlights:
    • New self-update system
    • Enhanced security features
    • Bug fixes

  → Run 'vtcode update install' to update
════════════════════════════════════════════════════════════════════════════════
```

**Features:**
- Color-coded output using `console` crate
- Shows current and latest versions
- Displays first 5 lines of release notes
- Clear call-to-action

#### 3. `prompt_for_update()`

Prompts user to install the update:

```rust
async fn prompt_for_update(manager: UpdateManager, status: &UpdateStatus) -> Result<()>
```

**Behavior:**
- Checks if terminal is interactive
- Respects auto-install configuration
- Uses `dialoguer` for user prompt
- Calls `perform_update()` if user confirms

#### 4. `perform_update()`

Performs the actual update installation:

```rust
async fn perform_update(mut manager: UpdateManager, _status: &UpdateStatus) -> Result<()>
```

**Features:**
- Shows progress spinner using `indicatif`
- Downloads and verifies update
- Displays success/failure messages
- Shows backup location
- Provides restart instructions

#### 5. `should_check_for_updates()`

Determines if update checks should run:

```rust
pub fn should_check_for_updates() -> bool
```

**Checks:**
- `VT_UPDATE_CHECK` environment variable
- CI/CD environment detection
- Interactive terminal detection

**Returns:** `false` if:
- `VT_UPDATE_CHECK=false`
- Running in CI/CD environment
- Non-interactive terminal

#### 6. `is_ci_environment()`

Detects CI/CD environments:

```rust
fn is_ci_environment() -> bool
```

**Detected Environments:**
- GitHub Actions (`GITHUB_ACTIONS`)
- GitLab CI (`GITLAB_CI`)
- CircleCI (`CIRCLECI`)
- Travis CI (`TRAVIS`)
- Jenkins (`JENKINS_URL`)
- Generic CI (`CI`, `CONTINUOUS_INTEGRATION`)

### Integration with Main Binary

In `src/main.rs`:

```rust
// Check for updates on startup (only for interactive commands)
if vtcode::startup::update_check::should_check_for_updates() && args.command.is_none() {
    // Run update check before starting interactive session
    if let Err(e) = vtcode::startup::update_check::check_for_updates_on_startup().await {
        tracing::debug!("Update check failed: {}", e);
    }
}
```

**Behavior:**
- Only runs for interactive sessions (no specific command)
- Runs before starting the main application
- Errors are logged but don't block startup

## Configuration

### Environment Variables

#### `VT_UPDATE_CHECK`

Controls whether startup update checks are enabled:

```bash
# Enable (default)
export VT_UPDATE_CHECK=true

# Disable
export VT_UPDATE_CHECK=false
```

#### `VTCODE_UPDATE_ENABLED`

Controls whether the update system is enabled:

```bash
export VTCODE_UPDATE_ENABLED=true
```

#### `VTCODE_UPDATE_FREQUENCY`

Controls how often updates are checked:

```bash
export VTCODE_UPDATE_FREQUENCY=daily  # always, daily, weekly, never
```

#### `VTCODE_UPDATE_AUTO_INSTALL`

Controls whether updates are installed automatically:

```bash
export VTCODE_UPDATE_AUTO_INSTALL=false
```

### CLI Configuration

```bash
# Configure update frequency
vtcode update config --frequency daily

# Enable auto-install
vtcode update config --auto-install true

# Disable updates
vtcode update config --enabled false
```

## User Experience

### Startup Flow

1. **User launches vtcode**
   ```bash
   vtcode
   ```

2. **Update check runs** (if enabled and interactive)
   - Timeout: 5 seconds
   - Silent on errors

3. **If update available:**
   - Display prominent notification
   - Show release highlights
   - Prompt for installation

4. **If user confirms:**
   - Download update
   - Verify checksums
   - Create backup
   - Install update
   - Show success message

5. **Application continues** normally

### Non-Intrusive Design

The update check is designed to be minimally disruptive:

- ⚡ **Fast**: 5-second timeout
- 🔇 **Silent**: No errors shown
- 🎯 **Contextual**: Only in interactive sessions
- 🚫 **Skippable**: Easy to disable

### Error Handling

All errors are handled gracefully:

```rust
// Check with timeout
match tokio::time::timeout(
    std::time::Duration::from_secs(5),
    manager.check_for_updates(),
).await {
    Ok(Ok(status)) => status,
    _ => {
        // Silently fail on timeout or error
        return Ok(());
    }
}
```

**Error Scenarios:**
- Network errors → Silent
- GitHub API errors → Silent
- Configuration errors → Silent
- Timeout errors → Silent

## Testing

### Unit Tests

Located in `src/startup/update_check.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_should_check_for_updates() { }
    
    #[test]
    fn test_is_ci_environment() { }
    
    #[test]
    fn test_display_minimal_notification() { }
}
```

### Manual Testing

```bash
# Test with updates available
vtcode

# Test with updates disabled
export VT_UPDATE_CHECK=false
vtcode

# Test in CI environment
export CI=true
vtcode

# Test with auto-install
vtcode update config --auto-install true
vtcode
```

## Dependencies

### Existing Dependencies Used

- `console`: Color-coded terminal output
- `dialoguer`: Interactive prompts
- `indicatif`: Progress indicators
- `is-terminal`: Terminal detection
- `tokio`: Async runtime

### No New Dependencies

The implementation uses only existing dependencies from the project.

## Performance

### Startup Impact

- **Typical case**: < 1 second (cached check)
- **Network check**: 1-3 seconds (with timeout)
- **Timeout**: 5 seconds maximum
- **Error case**: < 100ms (immediate return)

### Optimization Strategies

1. **Timeout Protection**: 5-second timeout prevents blocking
2. **Frequency Control**: Respects configured check frequency
3. **Caching**: Uses cached results when available
4. **Conditional Execution**: Only runs in interactive sessions
5. **Silent Failures**: No error handling overhead

## Security

### Safe by Default

- ✅ HTTPS-only downloads
- ✅ Checksum verification
- ✅ Automatic backups
- ✅ Rollback on failure
- ✅ No automatic execution without confirmation

### User Control

- Users must confirm installation
- Can disable startup checks
- Can configure auto-install
- Can skip prompts

## Future Enhancements

Potential improvements:

1. **Background Checks**: Check in background thread
2. **Update Notifications**: Desktop notifications
3. **Update History**: Track update history
4. **Rollback UI**: Interactive rollback selection
5. **Update Scheduling**: Schedule updates for specific times
6. **Bandwidth Control**: Limit download speed
7. **Update Analytics**: Track update success rates

## Troubleshooting

### Update Check Not Running

**Symptoms:**
- No update notification on startup

**Solutions:**
1. Check `VT_UPDATE_CHECK` environment variable
2. Verify running in interactive terminal
3. Check update configuration
4. Enable debug logging

### Slow Startup

**Symptoms:**
- vtcode takes long to start

**Solutions:**
1. Disable update checks: `export VT_UPDATE_CHECK=false`
2. Increase check frequency: `vtcode update config --frequency weekly`
3. Check network connectivity

### Update Prompt Not Showing

**Symptoms:**
- Update available but no prompt

**Solutions:**
1. Verify terminal is interactive
2. Check color support
3. Try manual check: `vtcode update check`

## See Also

- [Self-Update Guide](./guides/self-update.md)
- [Startup Update Check Guide](./STARTUP_UPDATE_CHECK.md)
- [Quick Reference](./SELF_UPDATE_QUICK_REFERENCE.md)
- [Implementation Details](./SELF_UPDATE_IMPLEMENTATION.md)
