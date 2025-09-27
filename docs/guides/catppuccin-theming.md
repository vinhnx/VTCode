# Catppuccin Theming in VT Code

This guide explains how VT Code integrates the [Catppuccin](https://github.com/catppuccin/rust)
color palettes and shows how to apply the same patterns in your own Rust
applications. It combines palette loading, CLI styling, and TUI consistency so
that your tools share a unified visual identity.

## Integration Overview

VT Code ships with first-class Catppuccin support. The core UI theme registry now
hydrates `ThemePalette` definitions directly from the `catppuccin` crate instead
of hand-maintained hex values. Four flavors are available out of the box:

- `catppuccin-latte` (light)
- `catppuccin-frappe`
- `catppuccin-macchiato`
- `catppuccin-mocha`

The default Ciapre presets remain available, so existing configurations continue
to work. Selecting a Catppuccin flavor automatically updates ANSI styling, the
CLI banner, and inline widgets because they all read from the unified
`ThemeStyles` cache.

## Palette Mapping

Catppuccin exposes the `PALETTE` constant. Each flavor provides rich metadata,
including canonical accents, ANSI codes, and HSL/RGB triples. VT Code converts
that data into its reusable `ThemePalette` structure:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum CatppuccinFlavorKind {
    Latte,
    Frappe,
    Macchiato,
    Mocha,
}

impl CatppuccinFlavorKind {
    const fn id(self) -> &'static str { /* ... */ }
    const fn label(self) -> &'static str { /* ... */ }
    fn flavor(self) -> catppuccin::Flavor { /* ... */ }
}

const CATPPUCCIN_FLAVORS: &[CatppuccinFlavorKind] = &[
    CatppuccinFlavorKind::Latte,
    CatppuccinFlavorKind::Frappe,
    CatppuccinFlavorKind::Macchiato,
    CatppuccinFlavorKind::Mocha,
];

fn register_catppuccin_themes(map: &mut HashMap<&'static str, ThemeDefinition>) {
    for &flavor_kind in CATPPUCCIN_FLAVORS {
        let flavor = flavor_kind.flavor();
        map.insert(flavor_kind.id(), ThemeDefinition {
            id: flavor_kind.id(),
            label: flavor_kind.label(),
            palette: ThemePalette {
                primary_accent: catppuccin_rgb(flavor.colors.lavender),
                secondary_accent: catppuccin_rgb(flavor.colors.sapphire),
                foreground: catppuccin_rgb(flavor.colors.text),
                background: catppuccin_rgb(flavor.colors.base),
                alert: catppuccin_rgb(flavor.colors.red),
                logo_accent: catppuccin_rgb(flavor.colors.peach),
            },
        });
    }
}
```

A small helper converts Catppuccin's `Color` into `anstyle::RgbColor`, enabling
contrast-aware styling throughout the CLI renderer:

```rust
fn catppuccin_rgb(color: catppuccin::Color) -> RgbColor {
    RgbColor(color.rgb.r, color.rgb.g, color.rgb.b)
}
```

Because the registry feeds the shared `ThemeStyles` cache, the ANSI renderer,
markdown formatter, and inline session all adapt to the same Catppuccin colors
without any additional plumbing.

## Key Library Touchpoints

The Catppuccin crate demonstrates integrations that map directly onto VT Code's
pipeline:

- **Inline UI conversion** – the examples demonstrate how `.fg(*color)` accepts
  any type implementing `Into<Color>`, which Catppuccin provides out of the box.
  VT Code leverages this by translating Catppuccin RGB values into the inline
  UI theme that drives the live terminal session.
- **Serde serialization** – the `serde` example serializes the entire palette to
  JSON, which is useful for exporting customized configurations or debugging
  runtime mappings.
- **ANSI painting** – the `term_grid` example pairs the Catppuccin palette with
  `ansi_term` to render swatches in plain terminals, mirroring how VT Code's
  `ThemeStyles` power rich CLI banners.

## Adding Catppuccin to a New Project

1. **Install dependencies**
   ```bash
   cargo add catppuccin
   # Optional integrations:
   cargo add catppuccin --features serde     # Serialize palettes
   cargo add catppuccin --features ansi-term # ANSI color swatches
   ```
2. **Choose palette anchors** – pick Catppuccin colors for your key UI tokens.
   ```rust
   use catppuccin::PALETTE;

   struct ThemePalette {
       primary: RgbColor,
       secondary: RgbColor,
       foreground: RgbColor,
       background: RgbColor,
       alert: RgbColor,
   }

   fn palette_from(flavor: catppuccin::Flavor) -> ThemePalette {
       let colors = flavor.colors;
       ThemePalette {
           primary: RgbColor(colors.lavender.rgb.r, colors.lavender.rgb.g, colors.lavender.rgb.b),
           secondary: RgbColor(colors.sapphire.rgb.r, colors.sapphire.rgb.g, colors.sapphire.rgb.b),
           foreground: RgbColor(colors.text.rgb.r, colors.text.rgb.g, colors.text.rgb.b),
           background: RgbColor(colors.base.rgb.r, colors.base.rgb.g, colors.base.rgb.b),
           alert: RgbColor(colors.red.rgb.r, colors.red.rgb.g, colors.red.rgb.b),
       }
   }
   ```
3. **Unify CLI and TUI styling** – compute derived styles once and reuse them
   everywhere. VT Code funnels palette data through a central cache so both the
   ANSI renderer and inline widgets share the same color decisions. Expose a
   helper like `active_styles()` that wraps your palette in higher-level styles.
4. **Respect contrast and fallbacks** – VT Code adjusts Catppuccin colors with
   WCAG contrast checks before applying them to text. When adopting this pattern,
   keep a list of lighter/darker fallbacks and prefer automated contrast guards
   over ad-hoc tweaks.
5. **Persist user preference** – surface theme IDs (`catppuccin-mocha`, etc.) in
   configuration and CLI commands so users can switch flavors without rebuilding
   binaries. Cache the selection and rehydrate it at startup for a seamless
   experience.

Following these steps ensures your Rust CLI and TUI experiences inherit the same
Catppuccin polish that VT Code provides out of the box.
