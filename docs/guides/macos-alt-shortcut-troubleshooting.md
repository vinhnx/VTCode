# macOS Alt Shortcut Troubleshooting Guide

## Overview

On macOS, the Alt key (also labeled as Option) may not send proper key events to VT Code or other terminal applications due to terminal emulator configuration. This guide provides solutions for common issues where Alt shortcuts don't work as expected.

## Common Symptoms

- Alt+key combinations don't trigger expected actions in VT Code
- The terminal appears to ignore Alt modifier presses
- Alt+arrow keys don't work for navigation
- Only Ctrl and Cmd shortcuts work, but Alt shortcuts are unresponsive

## Root Causes

There are three primary reasons Alt shortcuts fail on macOS:

### 1. Terminal Emulator Not Configured to Send Alt Codes

Most macOS terminal emulators intercept the Alt key for special characters (accented letters, symbols) rather than passing it as a modifier to the application. This is the most common cause.

### 2. Shell Configuration Interfering

Certain shell configurations (bash, zsh) may have key bindings that consume Alt key events before they reach the TUI application.

### 3. Incompatible Terminal Emulator

Some older terminal emulators (including the default Terminal.app) have limited support for modern keyboard protocols like Kitty or Helix protocols that properly distinguish Alt modifiers.

## Solutions

### Solution 1: Use a Modern Terminal Emulator (Recommended)

Upgrade to a terminal emulator with proper Alt key support:

#### Recommended Options

1. **iTerm2** (Free, widely used)
   - Works out-of-box with proper Alt key support
   - Download: https://iterm2.com/

2. **Warp** (Free, modern, feature-rich)
   - Built-in support for Alt keys and modern terminal protocols
   - Download: https://www.warp.dev/

3. **Ghostty** (MIT licensed, modern)
   - Excellent terminal protocol support (Kitty keyboard protocol)
   - Download: https://ghostty.org/

4. **Alacritty** (Free, minimal, cross-platform)
   - Modern keyboard handling with Kitty protocol support
   - Install: `brew install alacritty`

5. **WezTerm** (Free, Lua-configurable)
   - Modern keyboard protocol support
   - Install: `brew install wezterm`

After installing any of these emulators, Alt shortcuts in VT Code should work immediately without additional configuration.

### Solution 2: Enable Alt Key Passthrough in iTerm2

If you're using iTerm2, ensure Alt key is configured correctly:

1. **Open iTerm2 Settings:**
   - Menu bar → iTerm2 → Settings (or Preferences)

2. **Navigate to Profile Settings:**
   - Profiles → [Your Profile] → Keys

3. **Configure Alt Key:**
   - Find the option labeled "Left option key acts as" or "Right option key acts as"
   - Select: **+Esc** (this allows Alt key to be recognized as a modifier)
   - Alternative: Set to **Meta** if available

4. **Apply and Restart:**
   - Close and reopen iTerm2
   - Test Alt shortcuts in VT Code

### Solution 3: Configure Shell Bindings

If your shell (bash/zsh) has conflicting key bindings, you can disable them for Alt keys:

#### For Zsh (in ~/.zshrc)

```bash
# Remove shell's Alt key bindings to allow terminal app to handle them
bindkey -r '\M-'     # Alt modifier

# Or more specifically, clear problematic Alt bindings:
bindkey -r '\M-f'    # Alt+f
bindkey -r '\M-b'    # Alt+b
bindkey -r '\M-d'    # Alt+d
```

Reload your shell:
```bash
exec zsh
```

#### For Bash (in ~/.bashrc)

```bash
# Disable metamacro key handling for Alt
set +o meta-flag

# Or explicitly remove Alt bindings
bind -r '\M-'
```

Reload your shell:
```bash
exec bash
```

### Solution 4: Verify Terminal Emulator Sends Alt Events

Test whether your terminal is sending Alt key events correctly:

```bash
# Install a simple key test utility
cargo install keytest

# Or use the built-in test
cat << 'EOF' > /tmp/test_alt.sh
#!/bin/bash
# Press keys and check what the terminal sends
echo "Press any key (press Ctrl+C to exit):"
stty -echo -icanon
while true; do
    read -r -n1 key
    od -c <<< "$key" | head -1
done
EOF
chmod +x /tmp/test_alt.sh
/tmp/test_alt.sh
```

When you press Alt+a, you should see output like:
```
\033 a
```

This indicates the terminal is properly sending an escape sequence for the Alt key.

### Solution 5: Enable Kitty Keyboard Protocol (Advanced)

For terminal emulators that support it (Kitty, Ghostty, Wezterm, Alacritty), VT Code can use the advanced Kitty keyboard protocol for better key handling:

The Kitty keyboard protocol is automatically detected and used by VT Code when available. VT Code will automatically enable it if your terminal supports it.

To verify support:
```bash
# Check if VT Code detects your terminal capability
vtcode ask "Show my terminal type"
```

Look for output mentioning "Kitty" or "XTerm" keyboard protocol support.

## Platform-Specific Guidance

### Using macOS with Homebrew

If you installed VT Code via Homebrew, it will use your default terminal emulator. To use a specific emulator:

```bash
# Open VT Code in your preferred emulator
# Example with iTerm2
open -a iTerm "$(which vtcode)"
```

Or create an alias in your shell configuration:
```bash
# In ~/.zshrc or ~/.bashrc
alias vtcode-iterm='open -a iTerm "$(which vtcode)"'
alias vtcode-ghostty='open -a Ghostty "$(which vtcode)"'
```

### Default Terminal.app Limitations

If you're using the built-in Terminal.app:

1. **Limitation**: Terminal.app has limited keyboard protocol support
2. **Action**: Consider switching to iTerm2, Warp, or Ghostty for better compatibility

## Troubleshooting Steps

### Step 1: Identify Your Terminal Emulator

Run this in your terminal:
```bash
echo $TERM_PROGRAM
```

This will output:
- `iTerm.app` → iTerm2
- `WarpTerminal` → Warp
- `Apple_Terminal` → Terminal.app (limited support)
- Empty → Check manually or use another method

### Step 2: Check VT Code's Terminal Detection

Run VT Code and check the status line for detected terminal type:
```bash
vtcode
```

Look at the bottom status line—it should show your terminal type (e.g., "Ghostty", "iTerm", "Kitty").

### Step 3: Test Alt Key in VT Code

1. Start VT Code
2. Try pressing Alt+h (help/hello)
3. If nothing happens, Alt key is not being passed through

### Step 4: Verify It's Not a Binding Conflict

Check your shell's key bindings:
```bash
# For zsh
bindkey | grep -E '(\[|\M)'

# For bash
bind -p | grep -E 'alt|escape'
```

If you see Alt bindings listed, try disabling them using **Solution 3** above.

### Step 5: Check for Security/Accessibility Permissions

On newer macOS versions, verify terminal emulator has accessibility permissions:

1. **System Settings → Privacy & Security → Accessibility**
2. **Ensure your terminal emulator is listed** (e.g., iTerm2, Ghostty)
3. If missing, click the `+` button and add it

## Common Alt Shortcuts in VT Code

Once Alt keys are working, these shortcuts should be available:

| Shortcut | Action |
|----------|--------|
| Alt+h | Help/hint (context-dependent) |
| Alt+a | Alternative action/mode |
| Alt+j | Jump/navigation down |
| Alt+k | Jump/navigation up |
| Alt+↑ | Scroll up (if applicable) |
| Alt+↓ | Scroll down (if applicable) |
| Ctrl+c | Interrupt/cancel (always works) |

Note: Exact shortcuts depend on your VT Code version and context. Use Alt+h or check help text in the UI for the most current list.

## Testing Your Configuration

After making changes, test with this sequence:

1. **Restart your terminal emulator** (important!)
2. **Launch VT Code:**
   ```bash
   vtcode
   ```
3. **Try Alt+h** to trigger help
4. **Check the status line** for terminal type detection
5. **Verify Alt modifiers work** in the UI

## If Nothing Works

If you've tried all solutions and Alt keys still don't work:

1. **Update VT Code:**
   ```bash
   cargo install --force vtcode
   # or
   brew upgrade vtcode
   ```

2. **Switch to a different terminal emulator** (see **Solution 1**)

3. **Use Ctrl shortcuts as a workaround** (most commands have Ctrl alternatives)

4. **Report the issue** with:
   - Your terminal emulator and version
   - Output of `echo $TERM_PROGRAM`
   - Output of `vtcode ask "Show my terminal type"`
   - Steps taken to troubleshoot

## Related Documentation

- [TUI Event Handling Guide](./tui-event-handling.md) - Technical details on how VT Code handles keyboard input
- [Terminal Rendering Best Practices](./terminal-rendering-best-practices.md) - Terminal emulator compatibility details
- [Keyboard Protocol Testing](../testing/KEYBOARD_PROTOCOL_TESTING.md) - Advanced keyboard protocol testing
- [ANSI Escape Sequences Reference](../reference/ansi-escape-sequences.md) - Terminal control sequences including Alt key codes

## Quick Checklist

- [ ] Using a modern terminal emulator (iTerm2, Warp, Ghostty, Alacritty, or WezTerm)
- [ ] Alt key configured correctly in terminal settings (if applicable)
- [ ] Shell key bindings don't conflict with Alt keys
- [ ] Terminal emulator has accessibility permissions (macOS)
- [ ] VT Code is up-to-date
- [ ] Tested Alt+h or Alt+arrow keys in VT Code

## External Resources

- [iTerm2 Documentation](https://iterm2.com/documentation-scripting.html)
- [Ghostty Documentation](https://ghostty.org/)
- [Warp Terminal](https://www.warp.dev/)
- [Alacritty Configuration](https://github.com/alacritty/alacritty/wiki/Suggested-OSC-8-implementations)
- [WezTerm Configuration](https://wezfurlong.org/wezterm/config/files.html)
