# External Editor Configuration

The `/edit` command uses the [`editor-command`](https://docs.rs/editor-command/latest/editor_command/) crate to automatically detect and launch your preferred text editor.

## Overview

The `editor-command` crate handles editor detection and invocation across all major platforms (macOS, Linux, Windows) and integrates seamlessly with VT Code's TUI environment.

## Configuration

Editor settings are configured in the `[tools.editor]` section of `vtcode.toml`:

```toml
[tools.editor]
# Enable external editor support for /edit command
enabled = true

# Leave empty to use automatic detection based on environment variables
# Supports command arguments (example: "code --wait")
# Examples: "vim", "nvim", "emacs", "nano", "code", "code --wait", "zed", "subl -w"
preferred_editor = ""

# Suspend TUI event loop while editor is running
suspend_tui = true
```

## Editor Detection Order

When no `preferred_editor` is specified, VT Code uses a two-stage detection process:

### Stage 1: Environment Variables (Primary)

1. **`VISUAL` environment variable** (highest priority)
2. **`EDITOR` environment variable**

### Stage 2: Fallback Detection (if Stage 1 fails)

If neither environment variable is set, VT Code tries common editors in PATH:

**Unix/Linux/macOS:**

-   `nvim` (Neovim - preferred)
-   `vim`
-   `vi`
-   `nano`
-   `emacs`

**Windows:**

-   `code` (Visual Studio Code)
-   `notepad++`
-   `notepad`

## Usage Examples

### Launch interactive editor (no file)

```
/edit
```

This opens your default editor with a temporary file. The file contents are returned and inserted into your input when you save and close the editor.

### Edit specific file

```
/edit src/main.rs
```

Opens `src/main.rs` in your preferred editor.

### Edit relative paths

```
/edit path/to/file.txt
```

Paths are resolved relative to the workspace root.

## Setting Your Preferred Editor

### Via Environment Variables

**macOS/Linux:**

```bash
export EDITOR=nvim        # Lower priority
export VISUAL=vim         # Higher priority (takes precedence)
```

**Windows (PowerShell):**

```powershell
$env:EDITOR = "code"
$env:VISUAL = "code"
```

### Via Configuration File

Edit `vtcode.toml`:

```toml
[tools.editor]
preferred_editor = "nvim"  # Override automatic detection
```

## Supported Editors

The crate automatically detects these editors:

**CLI Editors:**

-   `vim`, `vi`
-   `nvim` (Neovim)
-   `nano`
-   `emacs`
-   `pico`

**GUI Editors:**

-   `code` (Visual Studio Code)
-   `zed`
-   `gedit` (GNOME)
-   `geany`
-   `code-oss`
-   `subl` (Sublime Text)
-   `atom`
-   `mate` (TextMate)
-   `open -a TextEdit` (macOS)

## Terminal State Management

The `suspend_tui = true` setting (recommended) ensures:

1. TUI event loop is suspended before launching the editor
2. Terminal alternate screen is properly exited
3. Pending events are drained (prevents input garbage)
4. Raw mode is disabled for the editor
5. Terminal state is restored when the editor closes
6. Screen is cleared to remove artifacts

This prevents terminal corruption and input conflicts when switching between VT Code and external editors.

## Troubleshooting

### "Failed to detect editor" error

This error means no editor was found. VT Code checks both environment variables and fallback editors.

**Solution (choose one):**

1. **Set EDITOR environment variable** (recommended):

    ```bash
    export EDITOR=vim
    # or
    export VISUAL=nvim
    ```

2. **Install a common editor**:

    ```bash
    # macOS
    brew install neovim

    # Ubuntu/Debian
    sudo apt install neovim

    # or use nano (usually pre-installed)
    which nano
    ```

3. **Explicitly configure in vtcode.toml**:
    ```toml
    [tools.editor]
    preferred_editor = "vim"
    ```

### Editor is in PATH but not detected

Verify the editor is accessible:

```bash
which nvim  # or your editor name
echo $EDITOR
echo $VISUAL
```

If the editor is in PATH but not detected, set `EDITOR` explicitly:

```bash
export EDITOR=/usr/bin/nvim
```

### Editor behavior is unusual

-   Ensure `suspend_tui = true` to prevent terminal state issues
-   Check if your editor has special terminal requirements
-   Consider using a different editor if problems persist

### Changes not saved in temporary files

When using `/edit` without a file argument:

1. A temporary file is created
2. Your editor is launched
3. When you save and close the editor, content is returned
4. The temporary file is automatically deleted

Ensure you actually save the file in your editor (e.g., `:w` in vim) before closing.

## Integration with VT Code Features

The `/edit` command works with:

-   **File browser** - Select files to edit using `@` symbol or `/files`
-   **Workspace context** - Editors open with workspace root as working directory
-   **Tool policies** - Controlled via `[tools.policies]` section (default: `"allow"`)

## See Also

-   [editor-command crate documentation](https://docs.rs/editor-command/latest/editor_command/)
-   [VT Code configuration guide](../config/CONFIGURATION_PRECEDENCE.md)
-   [Tools overview](./TOOLS_OVERVIEW.md)
