# vtcode-theme

Shared theme registry and runtime state for VT Code UI crates.

`vtcode-theme` provides a centralized theme system with 50+ built-in themes, runtime theme switching, contrast-aware color computation, and accessibility support. It is the single source of truth for all UI styling in VT Code.

## Highlights

- **50+ Built-in Themes** — Catppuccin (4 flavors), Solarized, Gruvbox, Dracula, GitHub, Ayu, Material, Monokai, Night Owl, Vitesse, and many more.
- **Theme Suites** — logical grouping of related themes (Catppuccin, Vitesse, Ciapre, Mono).
- **Contrast-Aware Styling** — all computed styles enforce a configurable minimum contrast ratio against the background.
- **Accessibility** — bold-is-bright mode, safe-colors-only mode, and per-theme contrast validation.
- **Runtime Theme Switching** — global active theme with thread-safe read/write access via `parking_lot::RwLock`.
- **Light/Dark Detection** — `is_light_theme`, `suggest_theme_for_terminal`, and `theme_matches_terminal_scheme` for automatic scheme matching.

## Modules

| Module | Purpose |
|---|---|
| `registry` | Theme registry with 50+ built-in `ThemeDefinition` entries and suite grouping |
| `runtime` | Global active theme state, style computation, banner colors, theme resolution |
| `types` | Core types: `ThemePalette`, `ThemeStyles`, `ThemeDefinition`, `ThemeSuite`, `ColorAccessibilityConfig` |
| `scheme` | Light/dark scheme detection and terminal scheme matching |
| `syntax` | Syntax theme mapping for UI contexts |
| `color_math` (private) | Color math utilities: contrast ratio, luminance balancing, mixing, lightening |

## Public entrypoints

### Theme registry

- `available_themes()` — sorted list of all built-in theme identifiers
- `available_theme_suites()` — theme suites with member theme lists
- `theme_label(theme_id)` — display label for a theme
- `theme_suite_id(theme_id)` / `theme_suite_label(theme_id)` — suite membership

### Runtime state

- `set_active_theme(theme_id)` — activate a built-in theme
- `active_theme_id()` / `active_theme_label()` — query the active theme
- `active_styles()` — clone the active `ThemeStyles`
- `resolve_theme(preferred)` — resolve a user preference to a valid theme ID
- `ensure_theme(theme_id)` — validate a theme exists and return its label
- `rebuild_active_styles()` — recompute styles after accessibility config changes

### Accessibility

- `set_color_accessibility_config(config)` — update contrast/bold/safe-colors settings
- `get_minimum_contrast()` / `is_bold_bright_mode()` / `is_safe_colors_only()` — query settings
- `validate_theme_contrast(theme_id)` — check a theme's palette against minimum contrast

### Styling

- `banner_color()` / `banner_style()` — accent color/style for banner-like copy
- `logo_accent_color()` — raw logo accent from the active theme

### Scheme detection

- `is_light_theme(theme_id)` — check if a theme uses a light background
- `suggest_theme_for_terminal()` — suggest a theme based on terminal background
- `theme_matches_terminal_scheme(theme_id)` — check if theme matches terminal scheme

## Usage

```rust
use vtcode_theme::{
    set_active_theme, active_styles, available_themes,
    resolve_theme, validate_theme_contrast,
};

// List available themes
let themes = available_themes();
assert!(themes.contains(&"catppuccin-mocha"));

// Activate a theme
set_active_theme("dracula").unwrap();
let styles = active_styles();
assert_eq!(styles.info.get_fg_color().is_some(), true);

// Resolve a user preference (falls back to default if invalid)
let resolved = resolve_theme(Some("invalid-theme".to_string()));

// Validate contrast
let result = validate_theme_contrast("catppuccin-mocha");
assert!(result.is_valid);
```

## API reference

See [docs.rs/vtcode-theme](https://docs.rs/vtcode-theme).

## Related docs

- [Architecture overview](../docs/ARCHITECTURE.md)
