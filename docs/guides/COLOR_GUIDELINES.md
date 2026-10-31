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

Inspired by [Ghostty's minimum-contrast feature](https://ghostty.org/docs/config/reference#minimum-contrast), VT Code
enforces WCAG contrast ratios:

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

Based on [terminal color portability research](https://blog.xoria.org/terminal-colors/), only **11 of 32** ANSI colors
are safe across common terminal themes (Basic, Tango, Solarized).

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

1. **Contour dark/light query** (`CSI ? 996 n`, answered explicitly by Contour, Ghostty 1.0+, Kitty 0.38.1+)
2. **OSC 11** background-color query with luminance inference
3. **COLORFGBG** environment variable (rxvt, xterm, many others)
4. **TERM_PROGRAM** heuristics (iTerm2, Ghostty, Apple Terminal)
5. **Default**: Dark (most common for development terminals)

```toml
[ui]
# Auto-select theme based on terminal detection
color_scheme_mode = "auto"

# Force light mode
# color_scheme_mode = "light"

# Force dark mode (default behavior)
# color_scheme_mode = "dark"
```

### Following Live Palette Changes

With `color_scheme_mode = "auto"` on unix, VT Code also subscribes to unsolicited dark/light reports (the Contour VT
extension via `CSI ? 2031 h`), so the theme follows the terminal live — for example when the OS switches between light
and dark mode, or when the terminal profile changes. The replacement theme prefers a light/dark twin from the same theme
suite (e.g. `catppuccin-mocha` ⇄ `catppuccin-latte`) and falls back to the default suggestion otherwise. Automatic
switching never rewrites your saved theme preference; `"light"` and `"dark"` disable following entirely. Terminals that
do not support the extension simply never send reports.

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

VT Code includes light and dark themes. Run `/theme` to browse them (the picker shows each id and its
light/dark scheme). The tables below cover the hand-curated entries; the authoritative catalog is the theme registry
(`available_themes()` in `vtcode-ui`), which also registers the longbridge/gpui-kit sets and additional classic schemes
(Dracula, GitHub, Atom One, Tomorrow, Nord, Material, Spacegray, and more). The gpui-kit sets are shared with the GPUI
desktop client, so the same theme id renders the same palette in the terminal and in the desktop app.

| Theme                        | Mode  | Description                       |
| ---------------------------- | ----- | --------------------------------- |
| ciapre                       | Dark  | Default warm amber theme          |
| ciapre-dark                  | Dark  | Alternative warm amber theme      |
| ciapre-blue                  | Dark  | Blue variant of Ciapre            |
| ansi-classic                 | Dark  | Classic ANSI palette              |
| mono                         | Dark  | Monochrome                        |
| vitesse-black                | Dark  | Pure black background             |
| vitesse-dark                 | Dark  | Dark gray background              |
| vitesse-dark-soft            | Dark  | Softer dark background            |
| vitesse-light                | Light | White background                  |
| vitesse-light-soft           | Light | Cream background                  |
| catppuccin-latte             | Light | Pastel light theme                |
| catppuccin-frappe            | Dark  | Muted dark theme                  |
| catppuccin-macchiato         | Dark  | Rich dark theme                   |
| catppuccin-mocha             | Dark  | Deep dark theme                   |
| solarized-dark               | Dark  | Solarized, expanded background    |
| solarized-dark-hc            | Dark  | Solarized with higher contrast    |
| solarized-light              | Light | Solarized light                   |
| gruvbox-dark                 | Dark  | Gruvbox dark                      |
| gruvbox-dark-hard            | Dark  | Gruvbox dark, hard background     |
| gruvbox-light                | Light | Gruvbox light                     |
| gruvbox-light-hard           | Light | Gruvbox light, hard background    |
| gruvbox-material             | Dark  | Gruvbox Material                  |
| gruvbox-material-dark        | Dark  | Gruvbox Material dark             |
| gruvbox-material-light       | Light | Gruvbox Material light            |

### gpui-kit theme sets

Ported from <https://github.com/longbridge/gpui-kit/tree/main/themes> and shared with the desktop client:

| Theme                      | Mode  | Set           |
| -------------------------- | ----- | ------------- |
| adventure                  | Dark  | Adventure     |
| adventure-time             | Dark  | Adventure     |
| alduin                     | Dark  | Alduin        |
| asciinema                  | Dark  | Asciinema     |
| aurora-light               | Light | Aurora        |
| ayu-dark                   | Dark  | Ayu           |
| ayu-light                  | Light | Ayu           |
| everforest-dark            | Dark  | Everforest    |
| everforest-light           | Light | Everforest    |
| fahrenheit                 | Dark  | Fahrenheit    |
| flexoki-dark               | Dark  | Flexoki       |
| flexoki-light              | Light | Flexoki       |
| harper                     | Dark  | Harper        |
| hybrid-dark                | Dark  | Hybrid        |
| hybrid-light               | Light | Hybrid        |
| jellybeans                 | Dark  | Jellybeans    |
| kibble                     | Dark  | Kibble        |
| macos-classic-dark         | Dark  | macOS Classic |
| macos-classic-light        | Light | macOS Classic |
| mellifluous-dark           | Dark  | Mellifluous   |
| mellifluous-light          | Light | Mellifluous   |
| molokai-dark               | Dark  | Molokai       |
| molokai-light              | Light | Molokai       |
| spaceduck                  | Dark  | Spaceduck     |
| tokyo-night                | Dark  | Tokyo         |
| tokyo-storm                | Dark  | Tokyo         |
| tokyo-moon                 | Dark  | Tokyo         |
| twilight                   | Dark  | Twilight      |

Multi-variant sets form a theme suite, so automatic light/dark switching stays inside the set (for example
`everforest-dark` ⇄ `everforest-light`); sets without a light variant fall back to the default suggestion.
Upstream `base.*` slots feed the ANSI hues; the brand color keeps its hue and is lifted toward the foreground only
until it clears the contrast floor as terminal text.

## Theme Validation

VT Code validates theme contrast at startup and logs warnings for colors that don't meet the configured minimum contrast
ratio.

## Desktop Sync

The desktop app builds its full theme catalog from this registry (`builtin_theme_colors`), so every CLI theme is
selectable on desktop. Sync between the two surfaces follows these rules:

- Desktop → CLI: palette themes sync by id; the desktop-only System Light/Dark entries sync as their per-mode
  default built-in theme. The CLI config writer accepts registry ids only.
- CLI → Desktop: the service reports the effective CLI theme (unknown config values resolve to the default, matching
  CLI startup), so follow/import always receives a registered id. Custom `.vtcode/theme.toml` color overrides do not
  transfer; desktop follows the base registry theme.
- Code syntax colors: the CLI maps UI themes to bat themes per id, while desktop code-token colors stay on the Kit
  mode default (Kit 0.8.0 exposes no per-palette syntax-token API); desktop code-block surfaces follow the palette.

## API Reference

For developers extending VT Code:

```rust
use vtcode_ui::theme::{
    ColorAccessibilityConfig,
    builtin_theme_colors,
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
