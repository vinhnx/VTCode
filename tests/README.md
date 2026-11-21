# VT Code Snapshot Tests

This directory contains snapshot tests for the VT Code terminal user interface (TUI), following Ratatui best practices
for UI testing, plus the committed tool-schema stability baselines.

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

Snapshots are reviewable fixtures, not generated noise. Blessing rewrites baselines to match whatever the code currently
produces, so an unreviewed bless can silently absorb a regression. Generate the change, read the diff, then accept it —
never accept first and inspect later.

### Accepting New Snapshots

When a test generates a new snapshot (first run or after changes), review the pending change first (see below), then
accept it with:

```bash
cargo insta accept --include-ignored
```

### Reviewing Changes

To review and selectively accept snapshot changes:

```bash
cargo insta review --include-ignored
```

### Updating Snapshots

If UI changes are intentional, regenerate and accept them in one reviewed step:

```bash
cargo insta test --accept
```

Then inspect the snapshot diff before committing.

### Tool-schema stability baselines

`tests/snapshots/tool_schemas/` holds committed JSON baselines checked by `ci_validate_no_schema_drift`. They are not
insta snapshots and are not gitignored. When a tool schema change is intentional, bless them and review the diff:

```bash
VTCODE_BLESS_TOOL_SNAPSHOTS=1 cargo nextest run -p vtcode -E 'test(ci_validate_no_schema_drift)'
```

The same command without `VTCODE_BLESS_TOOL_SNAPSHOTS` verifies current output against the committed baselines.

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
