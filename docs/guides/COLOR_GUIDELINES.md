# Terminal Color Guidelines

VT Code implements terminal color standards for accessibility, portability, and user preference compliance.

## Standards Implemented

### NO_COLOR Standard

VT Code respects the [NO_COLOR standard](https://no-color.org/):

- When `NO_COLOR` environment variable is set (and not empty), ANSI color output is suppressed
- The `--no-color` CLI flag also disables colors
- User configuration can override `NO_COLOR` per the standard

```bash
# Disable colors via environment
NO_COLOR=1 vtcode

# Disable colors via CLI flag
vtcode --no-color
```

### Minimum Contrast (Ghostty-inspired)

Inspired by [Ghostty's minimum-contrast feature](https://ghostty.org/docs/config/reference#minimum-contrast), VT Code enforces WCAG contrast ratios:

| Level      | Ratio | Use Case                                   |
| ---------- | ----- | ------------------------------------------ |
| WCAG AA    | 4.5:1 | Default, suitable for most users           |
| WCAG AAA   | 7.0:1 | Enhanced, recommended for low-vision users |
| Large Text | 3.0:1 | Minimum for 18pt+ or 14pt bold text        |
| Disabled   | 1.0:1 | No contrast enforcement                    |

Configure in `vtcode.toml`:

```toml
[ui]
minimum_contrast = 4.5  # WCAG AA (default)
# minimum_contrast = 7.0  # WCAG AAA for enhanced accessibility
```

### Safe ANSI Color Palette

Based on [terminal color portability research](https://blog.xoria.org/terminal-colors/), only **11 of 32** ANSI colors are safe across common terminal themes (Basic, Tango, Solarized).

#### Safe Colors (Portable)

| Regular     | Bright         |
| ----------- | -------------- |
| red (1)     | brred (9)      |
| green (2)   | brgreen (10)   |
| yellow (3)  | —              |
| blue (4)    | —              |
| magenta (5) | brmagenta (13) |
| cyan (6)    | brcyan (14)    |

#### Problematic Colors (Avoid)

| Color         | Issue                                                 |
| ------------- | ----------------------------------------------------- |
| black (0)     | Low contrast on dark backgrounds                      |
| white (7)     | Low contrast on light backgrounds                     |
| brblack (8)   | **Invisible** in Solarized Dark (hijacked for base03) |
| bryellow (11) | Low contrast on light backgrounds                     |
| brblue (12)   | Low contrast in Basic Dark                            |
| brwhite (15)  | Low contrast on light backgrounds                     |

Enable safe-only mode in `vtcode.toml`:

```toml
[ui]
safe_colors_only = true
```

## Configuration Reference

All color accessibility settings in `[ui]` section:

```toml
[ui]
# WCAG minimum contrast ratio (4.5 = AA, 7.0 = AAA, 1.0 = disabled)
minimum_contrast = 4.5

# Legacy terminal compatibility: avoid bold (may map to bright)
bold_is_bright = false

# Restrict to 11 portable ANSI colors
safe_colors_only = false

# Auto-detect light/dark terminal: "auto", "light", "dark"
color_scheme_mode = "auto"
```

## Light/Dark Mode Detection

VT Code can auto-detect terminal color scheme via:

1. **COLORFGBG** environment variable (rxvt, xterm, many others)
2. **TERM_PROGRAM** heuristics (iTerm2, Ghostty, Apple Terminal)
3. **Default**: Dark (most common for development terminals)

```toml
[ui]
# Auto-select theme based on terminal detection
color_scheme_mode = "auto"

# Force light mode
# color_scheme_mode = "light"

# Force dark mode (default behavior)
# color_scheme_mode = "dark"
```

## Bold-is-Bright Compatibility

Some legacy terminals map bold text to bright colors. This can cause visibility issues when:

- Bold red becomes bright red (different shade)
- Bold black becomes bright black (gray)
- Text color changes unexpectedly

Enable compatibility mode to avoid bold styling:

```toml
[ui]
bold_is_bright = true
```

## Available Themes

VT Code includes light and dark themes:

| Theme                | Mode  | Description              |
| -------------------- | ----- | ------------------------ |
| ciapre               | Dark  | Default warm amber theme |
| ciapre-dark          | Dark  | Alternative warm amber theme |
| ciapre-blue          | Dark  | Blue variant of Ciapre   |
| ansi-classic         | Dark  | Classic ANSI palette     |
| vitesse-black        | Dark  | Pure black background    |
| vitesse-dark         | Dark  | Dark gray background     |
| vitesse-dark-soft    | Dark  | Softer dark background   |
| vitesse-light        | Light | White background         |
| vitesse-light-soft   | Light | Cream background         |
| catppuccin-latte     | Light | Pastel light theme       |
| catppuccin-frappe    | Dark  | Muted dark theme         |
| catppuccin-macchiato | Dark  | Rich dark theme          |
| catppuccin-mocha     | Dark  | Deep dark theme          |

## Theme Validation

VT Code validates theme contrast at startup and logs warnings for colors that don't meet the configured minimum contrast ratio.

## API Reference

For developers extending VT Code:

```rust
use vtcode_core::ui::theme::{
    ColorAccessibilityConfig,
    set_color_accessibility_config,
    get_minimum_contrast,
    is_bold_bright_mode,
    is_safe_colors_only,
    validate_theme_contrast,
    is_light_theme,
    suggest_theme_for_terminal,
};

use vtcode_core::utils::ansi_capabilities::{
    detect_color_scheme,
    ColorScheme,
    is_no_color,
    is_clicolor_force,
};
```

## References

- [NO_COLOR Standard](https://no-color.org/)
- [Ghostty minimum-contrast](https://ghostty.org/docs/config/reference#minimum-contrast)
- [Terminal Colors Research](https://blog.xoria.org/terminal-colors/)
- [WCAG 2.1 Contrast Requirements](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html)
