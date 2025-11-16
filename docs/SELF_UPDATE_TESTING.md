# Self-Update Testing Guide

This guide explains how to test the self-update functionality in vtcode.

## Testing with Cargo Run

### 1. Test Update Check (Safe)

You can safely test the update check functionality with `cargo run`:

```bash
# Test basic update check
cargo run -- update check

# Test verbose update check
cargo run -- update check --verbose

# Test configuration
cargo run -- update config

# Test with different channels
cargo run -- update config --channel beta
cargo run -- update check
```

**What happens:**

-   Checks GitHub for latest release
-   Compares versions
-   Shows available updates
-   Does NOT install anything

### 2. Test Slash Command

```bash
# Start vtcode
cargo run

# In the chat session, try:
/update
/update check
/update status
/update install
```

**What happens:**

-   `/update` checks for updates
-   `/update status` shows configuration
-   `/update install` shows instructions (doesn't actually install)

### 3. Test Startup Check

```bash
# Enable startup checks
export VT_UPDATE_CHECK=true

# Run vtcode
cargo run

# You should see update notification if available
```

**What happens:**

-   Checks for updates on startup
-   Shows notification if update available
-   Prompts for installation (but won't work with cargo run)

### 4. Disable Startup Checks

```bash
# Disable for testing
export VT_UPDATE_CHECK=false

# Run vtcode
cargo run

# No update check on startup
```

## Testing with Installed Binary

For full testing including installation, you need an installed binary:

### 1. Build and Install

```bash
# Build release version
cargo build --release

# Install to local bin (Unix)
sudo cp target/release/vtcode /usr/local/bin/vtcode

# Or install to user bin (no sudo needed)
mkdir -p ~/.local/bin
cp target/release/vtcode ~/.local/bin/vtcode
export PATH="$HOME/.local/bin:$PATH"

# Verify installation
which vtcode
vtcode --version
```

### 2. Test Full Update Flow

```bash
# Check for updates
vtcode update check

# Install updates (if available)
vtcode update install

# Verify new version
vtcode --version
```

### 3. Test Rollback

```bash
# List backups
vtcode update backups

# Rollback to previous version
vtcode update rollback

# Verify version
vtcode --version
```

## Testing Individual Components

### 1. Test Update Checker

```bash
# Run the example
cargo run --example self_update_example
```

### 2. Test Configuration

```bash
# Test environment variables
export VTCODE_UPDATE_ENABLED=true
export VTCODE_UPDATE_CHANNEL=beta
export VTCODE_UPDATE_FREQUENCY=daily

cargo run -- update config
```

### 3. Test with Different Channels

```bash
# Test stable channel
cargo run -- update config --channel stable
cargo run -- update check

# Test beta channel
cargo run -- update config --channel beta
cargo run -- update check

# Test nightly channel
cargo run -- update config --channel nightly
cargo run -- update check
```

## Mock Testing (Development)

For development testing without actual updates:

### 1. Test with Mock Data

Create a test that mocks the GitHub API:

```rust
#[tokio::test]
async fn test_update_check_mock() {
    // Mock GitHub API response
    // Test update check logic
    // Verify behavior
}
```

### 2. Test Error Handling

```bash
# Test with invalid configuration
export VTCODE_UPDATE_CHANNEL=invalid
cargo run -- update check

# Test with network issues
# (disconnect network)
cargo run -- update check
```

### 3. Test Timeout

```bash
# The update check has a 5-second timeout
# Test that it doesn't block startup
time cargo run
```

## Integration Testing

### 1. Test Startup Flow

```bash
# Test normal startup with update check
export VT_UPDATE_CHECK=true
cargo run

# Test startup without update check
export VT_UPDATE_CHECK=false
cargo run

# Test in CI environment
export CI=true
cargo run
# Should skip update check
```

### 2. Test Slash Command Integration

```bash
# Start session
cargo run

# Test commands in order:
/update status
/update check
/update install
/help
/exit
```

### 3. Test Configuration Persistence

```bash
# Set configuration
cargo run -- update config --channel beta --frequency weekly

# Verify it persists
cargo run -- update config

# Check that it's used
cargo run -- update check
```

## Unit Testing

Run the test suite:

```bash
# Run all update tests
cargo test --package vtcode-core update

# Run specific test
cargo test --package vtcode-core test_update_config_default

# Run with output
cargo test --package vtcode-core update -- --nocapture
```

## Manual Testing Checklist

### Basic Functionality

-   [ ] `cargo run -- update check` works
-   [ ] `cargo run -- update config` shows configuration
-   [ ] `/update` slash command works in chat
-   [ ] `/update status` shows correct information
-   [ ] Startup check runs (when enabled)
-   [ ] Startup check skips (when disabled)

### Configuration

-   [ ] Environment variables work
-   [ ] CLI configuration works
-   [ ] Configuration persists
-   [ ] Different channels work
-   [ ] Different frequencies work

### Error Handling

-   [ ] Network errors handled gracefully
-   [ ] Invalid configuration handled
-   [ ] Timeout works (5 seconds)
-   [ ] CI environment detected
-   [ ] Non-interactive terminal detected

### Edge Cases

-   [ ] No internet connection
-   [ ] GitHub API rate limiting
-   [ ] Invalid channel name
-   [ ] Invalid frequency
-   [ ] Missing configuration

## Automated Testing

### CI/CD Testing

```yaml
# .github/workflows/test.yml
name: Test Self-Update

on: [push, pull_request]

jobs:
    test:
        runs-on: ubuntu-latest-arm64
        steps:
            - uses: actions/checkout@v2
            - name: Test update check
              run: cargo test --package vtcode-core update
            - name: Test CLI commands
              run: |
                  cargo build --release
                  ./target/release/vtcode update check
                  ./target/release/vtcode update config
```

## Troubleshooting Tests

### Test Fails: "No update available"

This is expected if you're on the latest version. To test with updates available:

1. Modify `CURRENT_VERSION` in `vtcode-core/src/update/mod.rs` to an older version
2. Rebuild and test
3. Revert changes after testing

### Test Fails: "Network error"

Check your internet connection and GitHub API access:

```bash
# Test GitHub API access
curl -I https://api.github.com

# Test with authentication
curl -H "Authorization: Bearer YOUR_TOKEN" https://api.github.com/repos/vinhnx/vtcode/releases/latest
```

### Test Fails: "Permission denied"

When testing installation:

```bash
# Use user-local installation
mkdir -p ~/.local/bin
cp target/release/vtcode ~/.local/bin/
export PATH="$HOME/.local/bin:$PATH"
```

### Test Hangs

If tests hang, check:

1. Network connectivity
2. GitHub API rate limiting
3. Timeout settings (should be 5 seconds)

## Performance Testing

### Measure Startup Time

```bash
# Without update check
export VT_UPDATE_CHECK=false
time cargo run -- --version

# With update check
export VT_UPDATE_CHECK=true
time cargo run -- --version

# Should be < 5 seconds difference
```

### Measure Update Check Time

```bash
# Time the update check
time cargo run -- update check
```

## Security Testing

### Test Checksum Verification

```bash
# Download an update
cargo run -- update check

# Verify checksum is checked
# (check logs with RUST_LOG=debug)
RUST_LOG=debug cargo run -- update check
```

### Test Signature Verification

```bash
# Enable signature verification
export VTCODE_UPDATE_VERIFY_SIGNATURES=true

# Check for updates
cargo run -- update check
```

## Best Practices for Testing

1. **Test in Isolation**: Use separate test environments
2. **Mock External Dependencies**: Mock GitHub API for unit tests
3. **Test Error Paths**: Ensure errors are handled gracefully
4. **Test Edge Cases**: Network failures, timeouts, invalid input
5. **Automate Tests**: Use CI/CD for automated testing
6. **Document Results**: Keep track of test results
7. **Test on Multiple Platforms**: Linux, macOS, Windows

## See Also

-   [Self-Update Guide](./guides/self-update.md)
-   [Implementation Details](./SELF_UPDATE_IMPLEMENTATION.md)
-   [Quick Reference](./SELF_UPDATE_QUICK_REFERENCE.md)
