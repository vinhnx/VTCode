# XDG Base Directory Specification Implementation

This document describes VT Code's implementation of the XDG Base Directory Specification, following the [Ratatui recipe pattern](https://ratatui.rs/recipes/apps/config-directories/).

## Overview

VT Code now follows platform-specific conventions for storing configuration and data files:

-   **Linux**: `~/.config/vtcode/` and `~/.local/share/vtcode/`
-   **macOS**: `~/Library/Application Support/com.vinhnx.vtcode/`
-   **Windows**: `%APPDATA%\vinhnx\vtcode\`

## Directory Structure

### Config Directory

Stores configuration files and user preferences:

```
Config Directory/
├── vtcode.toml          # Main configuration file
└── skills/              # User-defined skills
```

**Default Locations:**

-   Linux: `~/.config/vtcode/`
-   macOS: `~/Library/Application Support/com.vinhnx.vtcode/`
-   Windows: `%APPDATA%\vinhnx\vtcode\config\`

**Override:** Set `VTCODE_CONFIG` environment variable

### Data Directory

Stores cache, logs, and temporary data:

```
Data Directory/
├── cache/               # Tree-sitter parsers, embeddings
├── logs/                # Application logs
├── sessions/            # Conversation history
└── telemetry/           # Usage analytics (if enabled)
```

**Default Locations:**

-   Linux: `~/.local/share/vtcode/`
-   macOS: `~/Library/Application Support/com.vinhnx.vtcode/`
-   Windows: `%APPDATA%\vinhnx\vtcode\data\`

**Override:** Set `VTCODE_DATA` environment variable

## Migration Guide

### From Legacy `~/.vtcode/` Structure

Existing installations using `~/.vtcode/` will continue to work as a fallback:

1. **Automatic Migration** (Planned): Future versions will auto-migrate to XDG directories
2. **Manual Migration**:

    ```bash
    # On Linux
    mv ~/.vtcode ~/.config/vtcode

    # On macOS
    mkdir -p ~/Library/Application\ Support/com.vinhnx.vtcode
    mv ~/.vtcode/* ~/Library/Application\ Support/com.vinhnx.vtcode/
    ```

3. **Keep Legacy**: Set `VTCODE_CONFIG=~/.vtcode` to continue using old structure

## Environment Variables

### VTCODE_CONFIG

Override the configuration directory location:

```bash
export VTCODE_CONFIG=/path/to/config
vtcode --version  # Shows: Config directory: /path/to/config
```

### VTCODE_DATA

Override the data directory location:

```bash
export VTCODE_DATA=/path/to/data
vtcode --version  # Shows: Data directory: /path/to/data
```

## Implementation Details

### Dependencies

-   **`directories` crate** (v6.0): Provides `ProjectDirs` for XDG-compliant path resolution
-   **Project Qualifier**: `com.vinhnx.vtcode` (follows reverse domain naming)

### Helper Functions

```rust
use vtcode_config::defaults::{get_config_dir, get_data_dir};

// Get XDG-compliant config directory
let config_dir = get_config_dir()
    .expect("Unable to determine config directory");

// Get XDG-compliant data directory
let data_dir = get_data_dir()
    .expect("Unable to determine data directory");
```

### Resolution Order

1. **Environment Variable** (`VTCODE_CONFIG` / `VTCODE_DATA`)
2. **XDG Directories** via `ProjectDirs::from("com", "vinhnx", "vtcode")`
3. **Legacy Fallback** (`~/.vtcode/` and `~/.vtcode/cache/`)

## Testing

Verify XDG directory resolution:

```bash
# Check default directories
vtcode --version

# Test environment overrides
VTCODE_CONFIG=/tmp/config VTCODE_DATA=/tmp/data vtcode --version
```

## Benefits

1. **Cleaner Home Directory**: Follows OS conventions instead of cluttering `~/`
2. **Separation of Concerns**: Config and data stored separately
3. **User Control**: Easy customization via environment variables
4. **Platform Consistency**: Works naturally on Linux, macOS, and Windows
5. **Backwards Compatible**: Legacy `~/.vtcode/` still works as fallback

## References

-   [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html)
-   [Ratatui Config Directories Recipe](https://ratatui.rs/recipes/apps/config-directories/)
-   [directories-rs Documentation](https://docs.rs/directories/latest/directories/)

## Version Information

-   **Implemented**: v0.50.12
-   **Status**: Stable
-   **Breaking Changes**: None (legacy paths still supported)
