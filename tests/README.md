# VTCode TUI Snapshot Tests

This directory contains snapshot tests for the VTCode terminal user interface (TUI), following Ratatui best practices for UI testing.

## Test Categories

### 1. Component Tests (`tui_snapshot_tests.rs`)
- Tests individual UI components like themes, message segments, and header contexts
- Uses string representation snapshots for debugging
- Ensures component structures remain consistent

### 2. Integration Tests (`ratatui_integration_tests.rs`) 
- Tests actual terminal rendering using `TestBackend`
- Verifies that the Ratatui rendering system works correctly
- Captures terminal output snapshots for visual regression testing

### 3. Comprehensive Tests (`improved_tui_snapshot_tests.rs`)
- Tests complete TUI functionality including session creation
- Verifies that UI components work together properly
- Includes actual rendering simulations with various content types

## Running Tests

To run the snapshot tests:

```bash
# Run all snapshot tests
cargo test --test tui_snapshot_tests
cargo test --test ratatui_integration_tests  
cargo test --test improved_tui_snapshot_tests

# Or run them all together
cargo test tui_snapshot
```

## Snapshot Management

### Accepting New Snapshots
When a test generates a new snapshot (first run or after changes), accept it with:

```bash
cargo insta accept --include-ignored
```

### Reviewing Changes
To review and selectively accept snapshot changes:

```bash
cargo insta review --include-ignored
```

### Updating Snapshots
If UI changes are intentional, update snapshots with:

```bash
cargo insta test
```

## Test Structure

Each test file follows these principles:
- Uses `insta` crate for snapshot management
- Tests both individual components and integration scenarios
- Includes descriptive test names and snapshot identifiers
- Follows Ratatui testing patterns using `TestBackend`

## Dependencies

The tests require:
- `insta` for snapshot testing
- `ratatui` with `TestBackend` for terminal simulation
- Access to public TUI interfaces from `vtcode_core`

## Best Practices

- Each snapshot test should focus on a specific aspect of the UI
- Use descriptive snapshot names for easy identification
- Test both positive and edge cases
- Ensure tests are deterministic and reproducible
- Keep test inputs simple but representative