//! Theme synchronization feature configuration generator.
//!
//! Generates terminal-specific color scheme configuration to match VT Code themes.
//! Supports dark and light theme variants.

use crate::terminal_setup::detector::TerminalType;
use anyhow::{Result, anyhow};

/// Default VT Code dark theme colors
pub struct VTCodeDarkTheme {
    pub background: &'static str,
    pub foreground: &'static str,
    pub cursor: &'static str,
    pub selection_bg: &'static str,
    // ANSI colors (0-7)
    pub black: &'static str,
    pub red: &'static str,
    pub green: &'static str,
    pub yellow: &'static str,
    pub blue: &'static str,
    pub magenta: &'static str,
    pub cyan: &'static str,
    pub white: &'static str,
    // Bright ANSI colors (8-15)
    pub bright_black: &'static str,
    pub bright_red: &'static str,
    pub bright_green: &'static str,
    pub bright_yellow: &'static str,
    pub bright_blue: &'static str,
    pub bright_magenta: &'static str,
    pub bright_cyan: &'static str,
    pub bright_white: &'static str,
}

impl Default for VTCodeDarkTheme {
    fn default() -> Self {
        Self {
            background: "#1e1e1e",
            foreground: "#d4d4d4",
            cursor: "#ffffff",
            selection_bg: "#264f78",
            // ANSI colors
            black: "#000000",
            red: "#cd3131",
            green: "#0dbc79",
            yellow: "#e5e510",
            blue: "#2472c8",
            magenta: "#bc3fbc",
            cyan: "#11a8cd",
            white: "#e5e5e5",
            // Bright variants
            bright_black: "#666666",
            bright_red: "#f14c4c",
            bright_green: "#23d18b",
            bright_yellow: "#f5f543",
            bright_blue: "#3b8eea",
            bright_magenta: "#d670d6",
            bright_cyan: "#29b8db",
            bright_white: "#ffffff",
        }
    }
}

impl VTCodeDarkTheme {
    fn base16_colors(&self) -> [&'static str; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }
}

#[derive(Clone, Copy, Debug)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Clone, Copy, Debug)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

impl Rgb {
    fn from_hex(hex: &str) -> Result<Self> {
        let trimmed = hex.trim_start_matches('#');
        if trimmed.len() != 6 {
            return Err(anyhow!("Invalid hex color '{}': expected #RRGGBB", hex));
        }

        let r = u8::from_str_radix(&trimmed[0..2], 16)
            .map_err(|_| anyhow!("Invalid red component in '{}'", hex))?;
        let g = u8::from_str_radix(&trimmed[2..4], 16)
            .map_err(|_| anyhow!("Invalid green component in '{}'", hex))?;
        let b = u8::from_str_radix(&trimmed[4..6], 16)
            .map_err(|_| anyhow!("Invalid blue component in '{}'", hex))?;

        Ok(Self { r, g, b })
    }

    fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    fn to_lab(self) -> Lab {
        let r = srgb_to_linear(self.r as f64 / 255.0);
        let g = srgb_to_linear(self.g as f64 / 255.0);
        let b = srgb_to_linear(self.b as f64 / 255.0);

        let x = r * 0.412_456_4 + g * 0.357_576_1 + b * 0.180_437_5;
        let y = r * 0.212_672_9 + g * 0.715_152_2 + b * 0.072_175;
        let z = r * 0.019_333_9 + g * 0.119_192 + b * 0.950_304_1;

        let fx = lab_f(x / 0.95047);
        let fy = lab_f(y);
        let fz = lab_f(z / 1.08883);

        Lab {
            l: 116.0 * fy - 16.0,
            a: 500.0 * (fx - fy),
            b: 200.0 * (fy - fz),
        }
    }

    fn from_lab(lab: Lab) -> Self {
        let fy = (lab.l + 16.0) / 116.0;
        let fx = fy + (lab.a / 500.0);
        let fz = fy - (lab.b / 200.0);

        let x = 0.95047 * lab_f_inv(fx);
        let y = lab_f_inv(fy);
        let z = 1.08883 * lab_f_inv(fz);

        let r_linear = x * 3.240_454_2 + y * -1.537_138_5 + z * -0.498_531_4;
        let g_linear = x * -0.969_266 + y * 1.876_010_8 + z * 0.041_556;
        let b_linear = x * 0.055_643_4 + y * -0.204_025_9 + z * 1.057_225_2;

        Self {
            r: to_u8(linear_to_srgb(r_linear)),
            g: to_u8(linear_to_srgb(g_linear)),
            b: to_u8(linear_to_srgb(b_linear)),
        }
    }
}

fn srgb_to_linear(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(channel: f64) -> f64 {
    if channel <= 0.0031308 {
        12.92 * channel
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn lab_f(value: f64) -> f64 {
    if value > 216.0 / 24389.0 {
        value.cbrt()
    } else {
        (24389.0 / 27.0 * value + 16.0) / 116.0
    }
}

fn lab_f_inv(value: f64) -> f64 {
    let cube = value * value * value;
    if cube > 216.0 / 24389.0 {
        cube
    } else {
        (116.0 * value - 16.0) / (24389.0 / 27.0)
    }
}

fn to_u8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn lerp_lab(t: f64, start: Lab, end: Lab) -> Lab {
    Lab {
        l: start.l + t * (end.l - start.l),
        a: start.a + t * (end.a - start.a),
        b: start.b + t * (end.b - start.b),
    }
}

fn generate_256_palette(theme: &VTCodeDarkTheme, harmonious: bool) -> Result<Vec<Rgb>> {
    let base16 = theme
        .base16_colors()
        .iter()
        .map(|color| Rgb::from_hex(color))
        .collect::<Result<Vec<_>>>()?;

    let background = Rgb::from_hex(theme.background)?;
    let foreground = Rgb::from_hex(theme.foreground)?;

    let mut base8_lab = [
        background.to_lab(),
        base16[1].to_lab(),
        base16[2].to_lab(),
        base16[3].to_lab(),
        base16[4].to_lab(),
        base16[5].to_lab(),
        base16[6].to_lab(),
        foreground.to_lab(),
    ];

    let is_light_theme = base8_lab[7].l < base8_lab[0].l;
    if is_light_theme && !harmonious {
        base8_lab.swap(0, 7);
    }

    let mut palette = base16;

    for r in 0..6 {
        let t_r = r as f64 / 5.0;
        let c0 = lerp_lab(t_r, base8_lab[0], base8_lab[1]);
        let c1 = lerp_lab(t_r, base8_lab[2], base8_lab[3]);
        let c2 = lerp_lab(t_r, base8_lab[4], base8_lab[5]);
        let c3 = lerp_lab(t_r, base8_lab[6], base8_lab[7]);

        for g in 0..6 {
            let t_g = g as f64 / 5.0;
            let c4 = lerp_lab(t_g, c0, c1);
            let c5 = lerp_lab(t_g, c2, c3);

            for b in 0..6 {
                let t_b = b as f64 / 5.0;
                let color = lerp_lab(t_b, c4, c5);
                palette.push(Rgb::from_lab(color));
            }
        }
    }

    for shade in 0..24 {
        let t = (shade as f64 + 1.0) / 25.0;
        let color = lerp_lab(t, base8_lab[0], base8_lab[7]);
        palette.push(Rgb::from_lab(color));
    }

    Ok(palette)
}

fn ghostty_palette_lines(palette: &[Rgb]) -> String {
    palette
        .iter()
        .enumerate()
        .map(|(index, color)| format!("palette = {index}={}", color.to_hex()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn kitty_palette_lines(palette: &[Rgb]) -> String {
    palette
        .iter()
        .enumerate()
        .map(|(index, color)| format!("color{index} {}", color.to_hex()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate theme configuration for the specified terminal
pub fn generate_config(terminal: TerminalType) -> Result<String> {
    let theme = VTCodeDarkTheme::default();
    let generated_palette = generate_256_palette(&theme, false)?;

    let config = match terminal {
        TerminalType::Ghostty => {
            let mut config = format!(
                r#"# VT Code Dark Theme for Ghostty
background = {background}
foreground = {foreground}
cursor-color = {cursor}
selection-background = {selection_bg}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
            );
            config.push_str("\n# ANSI + 256-color palette generated from base colors\n");
            config.push_str(&ghostty_palette_lines(&generated_palette));
            config.push('\n');
            config
        }

        TerminalType::Kitty => {
            let mut config = format!(
                r#"# VT Code Dark Theme for Kitty
background {background}
foreground {foreground}
cursor {cursor}
selection_background {selection_bg}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
            );
            config.push_str("\n# ANSI + 256-color palette generated from base colors\n");
            config.push_str(&kitty_palette_lines(&generated_palette));
            config.push('\n');
            config
        }

        TerminalType::Alacritty => {
            let mut config = format!(
                r#"# VT Code Dark Theme for Alacritty
[colors.primary]
background = '{background}'
foreground = '{foreground}'

[colors.cursor]
cursor = '{cursor}'

[colors.selection]
background = '{selection_bg}'

[colors.normal]
black = '{black}'
red = '{red}'
green = '{green}'
yellow = '{yellow}'
blue = '{blue}'
magenta = '{magenta}'
cyan = '{cyan}'
white = '{white}'

[colors.bright]
black = '{bright_black}'
red = '{bright_red}'
green = '{bright_green}'
yellow = '{bright_yellow}'
blue = '{bright_blue}'
magenta = '{bright_magenta}'
cyan = '{bright_cyan}'
white = '{bright_white}'
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            );

            config.push_str("\n# Extended indexed colors (16-255)\n");
            for (index, color) in generated_palette.iter().enumerate().skip(16) {
                config.push_str("[[colors.indexed_colors]]\n");
                config.push_str(&format!("index = {index}\n"));
                config.push_str(&format!("color = '{}'\n\n", color.to_hex()));
            }

            config
        }

        TerminalType::WezTerm => {
            format!(
                r#"-- VT Code Dark Theme for WezTerm
return {{
  colors = {{
    background = "{background}",
    foreground = "{foreground}",
    cursor_bg = "{cursor}",
    selection_bg = "{selection_bg}",
  }},
}}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
            )
        }

        TerminalType::TerminalApp => {
            r#"Terminal.app theme sync requires profile color configuration.
Configure profile colors in Terminal → Settings → Profiles.
"#
            .to_string()
        }

        TerminalType::Xterm => {
            r#"xterm theme sync is configured via X resources (e.g. ~/.Xresources).
"#
            .to_string()
        }

        TerminalType::Zed => {
            format!(
                r#"// VT Code Dark Theme for Zed
{{
  "theme": {{
    "mode": "dark",
    "terminal": {{
      "background": "{background}",
      "foreground": "{foreground}",
      "cursor": "{cursor}",
      "selectionBackground": "{selection_bg}",
      "ansiBlack": "{black}",
      "ansiRed": "{red}",
      "ansiGreen": "{green}",
      "ansiYellow": "{yellow}",
      "ansiBlue": "{blue}",
      "ansiMagenta": "{magenta}",
      "ansiCyan": "{cyan}",
      "ansiWhite": "{white}",
      "ansiBrightBlack": "{bright_black}",
      "ansiBrightRed": "{bright_red}",
      "ansiBrightGreen": "{bright_green}",
      "ansiBrightYellow": "{bright_yellow}",
      "ansiBrightBlue": "{bright_blue}",
      "ansiBrightMagenta": "{bright_magenta}",
      "ansiBrightCyan": "{bright_cyan}",
      "ansiBrightWhite": "{bright_white}"
    }}
  }}
}}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            )
        }

        TerminalType::Warp => r#"# Warp Theme Synchronization
# Warp uses its own theme system
# To create a custom theme:
# 1. Open Warp Settings
# 2. Go to Appearance → Themes
# 3. Click "New Theme" or "Import Theme"
# 4. Use the VT Code color values provided in the wizard

# VT Code colors are displayed in the terminal setup output
# You can manually configure them in Warp's theme editor
"#
        .to_string(),

        TerminalType::WindowsTerminal => {
            format!(
                r#"{{
  "schemes": [
    {{
      "name": "VT Code Dark",
      "background": "{background}",
      "foreground": "{foreground}",
      "cursorColor": "{cursor}",
      "selectionBackground": "{selection_bg}",
      "black": "{black}",
      "red": "{red}",
      "green": "{green}",
      "yellow": "{yellow}",
      "blue": "{blue}",
      "purple": "{magenta}",
      "cyan": "{cyan}",
      "white": "{white}",
      "brightBlack": "{bright_black}",
      "brightRed": "{bright_red}",
      "brightGreen": "{bright_green}",
      "brightYellow": "{bright_yellow}",
      "brightBlue": "{bright_blue}",
      "brightPurple": "{bright_magenta}",
      "brightCyan": "{bright_cyan}",
      "brightWhite": "{bright_white}"
    }}
  ],
  "profiles": {{
    "defaults": {{
      "colorScheme": "VT Code Dark"
    }}
  }}
}}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            )
        }

        TerminalType::Hyper => {
            format!(
                r#"// VT Code Dark Theme for Hyper
module.exports = {{
  config: {{
    backgroundColor: '{background}',
    foregroundColor: '{foreground}',
    cursorColor: '{cursor}',
    selectionColor: '{selection_bg}',
    colors: {{
      black: '{black}',
      red: '{red}',
      green: '{green}',
      yellow: '{yellow}',
      blue: '{blue}',
      magenta: '{magenta}',
      cyan: '{cyan}',
      white: '{white}',
      lightBlack: '{bright_black}',
      lightRed: '{bright_red}',
      lightGreen: '{bright_green}',
      lightYellow: '{bright_yellow}',
      lightBlue: '{bright_blue}',
      lightMagenta: '{bright_magenta}',
      lightCyan: '{bright_cyan}',
      lightWhite: '{bright_white}',
    }}
  }}
}};
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            )
        }

        TerminalType::Tabby => {
            format!(
                r#"# VT Code Dark Theme for Tabby
appearance:
  colorScheme:
    name: "VT Code Dark"
    foreground: "{foreground}"
    background: "{background}"
    cursor: "{cursor}"
    selection: "{selection_bg}"
    colors:
      - "{black}"
      - "{red}"
      - "{green}"
      - "{yellow}"
      - "{blue}"
      - "{magenta}"
      - "{cyan}"
      - "{white}"
      - "{bright_black}"
      - "{bright_red}"
      - "{bright_green}"
      - "{bright_yellow}"
      - "{bright_blue}"
      - "{bright_magenta}"
      - "{bright_cyan}"
      - "{bright_white}"
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            )
        }

        TerminalType::ITerm2 => r#"Manual iTerm2 Theme Configuration:

1. Open iTerm2 Preferences (Cmd+,)
2. Go to Profiles → Colors
3. Click "Color Presets..." → "Import..."
4. Or manually configure colors:

Background: #1e1e1e
Foreground: #d4d4d4
Cursor: #ffffff
Selection: #264f78

ANSI Colors:
Black: #000000, Red: #cd3131, Green: #0dbc79, Yellow: #e5e510
Blue: #2472c8, Magenta: #bc3fbc, Cyan: #11a8cd, White: #e5e5e5

Bright Colors:
Black: #666666, Red: #f14c4c, Green: #23d18b, Yellow: #f5f543
Blue: #3b8eea, Magenta: #d670d6, Cyan: #29b8db, White: #ffffff

Alternative: Download VT Code.itermcolors file and import
"#
        .to_string(),

        TerminalType::VSCode => {
            format!(
                r#"VS Code Terminal Theme Configuration:

The terminal automatically inherits your VS Code theme colors.

To customize terminal colors independently, add to settings.json:
{{
  "workbench.colorCustomizations": {{
    "terminal.background": "{background}",
    "terminal.foreground": "{foreground}",
    "terminalCursor.background": "{cursor}",
    "terminal.selectionBackground": "{selection_bg}",
    "terminal.ansiBlack": "{black}",
    "terminal.ansiRed": "{red}",
    "terminal.ansiGreen": "{green}",
    "terminal.ansiYellow": "{yellow}",
    "terminal.ansiBlue": "{blue}",
    "terminal.ansiMagenta": "{magenta}",
    "terminal.ansiCyan": "{cyan}",
    "terminal.ansiWhite": "{white}",
    "terminal.ansiBrightBlack": "{bright_black}",
    "terminal.ansiBrightRed": "{bright_red}",
    "terminal.ansiBrightGreen": "{bright_green}",
    "terminal.ansiBrightYellow": "{bright_yellow}",
    "terminal.ansiBrightBlue": "{bright_blue}",
    "terminal.ansiBrightMagenta": "{bright_magenta}",
    "terminal.ansiBrightCyan": "{bright_cyan}",
    "terminal.ansiBrightWhite": "{bright_white}"
  }}
}}
"#,
                background = theme.background,
                foreground = theme.foreground,
                cursor = theme.cursor,
                selection_bg = theme.selection_bg,
                black = theme.black,
                red = theme.red,
                green = theme.green,
                yellow = theme.yellow,
                blue = theme.blue,
                magenta = theme.magenta,
                cyan = theme.cyan,
                white = theme.white,
                bright_black = theme.bright_black,
                bright_red = theme.bright_red,
                bright_green = theme.bright_green,
                bright_yellow = theme.bright_yellow,
                bright_blue = theme.bright_blue,
                bright_magenta = theme.bright_magenta,
                bright_cyan = theme.bright_cyan,
                bright_white = theme.bright_white,
            )
        }

        TerminalType::Unknown => {
            anyhow::bail!("Cannot generate theme config for unknown terminal type");
        }
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vtcode_dark_theme_defaults() {
        let theme = VTCodeDarkTheme::default();
        assert_eq!(theme.background, "#1e1e1e");
        assert_eq!(theme.foreground, "#d4d4d4");
        assert_eq!(theme.cursor, "#ffffff");
    }

    #[test]
    fn test_generate_ghostty_config() {
        let config = generate_config(TerminalType::Ghostty).unwrap();
        assert!(config.contains("palette = 0="));
        assert!(config.contains("palette = 255="));
        assert!(config.contains("#1e1e1e"));
    }

    #[test]
    fn test_generate_kitty_config() {
        let config = generate_config(TerminalType::Kitty).unwrap();
        assert!(config.contains("color0 "));
        assert!(config.contains("color255 "));
    }

    #[test]
    fn test_generate_alacritty_config() {
        let config = generate_config(TerminalType::Alacritty).unwrap();
        assert!(config.contains("[colors"));
        assert!(config.contains("primary"));
        assert!(config.contains("index = 255"));
    }

    #[test]
    fn test_generate_windows_terminal_config() {
        let config = generate_config(TerminalType::WindowsTerminal).unwrap();
        assert!(config.contains("schemes"));
        assert!(config.contains("VT Code Dark"));
    }

    #[test]
    fn test_generate_vscode_instructions() {
        let config = generate_config(TerminalType::VSCode).unwrap();
        assert!(config.contains("workbench.colorCustomizations"));
        assert!(config.contains("terminal.ansi"));
    }

    #[test]
    fn test_unknown_terminal_error() {
        let result = generate_config(TerminalType::Unknown);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_config() {
        // This test exists for backward compatibility with the stub
        assert!(generate_config(TerminalType::Kitty).is_ok());
    }

    #[test]
    fn test_generated_palette_has_256_entries_and_preserves_base16() {
        let theme = VTCodeDarkTheme::default();
        let palette = generate_256_palette(&theme, false).unwrap();

        assert_eq!(palette.len(), 256);

        let expected_base16 = theme
            .base16_colors()
            .iter()
            .map(|c| Rgb::from_hex(c).unwrap().to_hex())
            .collect::<Vec<_>>();
        let actual_base16 = palette
            .iter()
            .take(16)
            .map(|rgb| rgb.to_hex())
            .collect::<Vec<_>>();

        assert_eq!(actual_base16, expected_base16);
    }
}
