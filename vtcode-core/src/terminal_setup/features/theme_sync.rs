//! Theme synchronization feature configuration generator.
//!
//! Generates terminal-specific color scheme configuration to match VT Code themes.
//! Supports dark and light theme variants.

use crate::terminal_setup::detector::TerminalType;
use anyhow::Result;

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

/// Generate theme configuration for the specified terminal
pub fn generate_config(terminal: TerminalType) -> Result<String> {
    let theme = VTCodeDarkTheme::default();

    let config = match terminal {
        TerminalType::Ghostty => {
            format!(
                r#"# VT Code Dark Theme for Ghostty
background = {background}
foreground = {foreground}
cursor-color = {cursor}
selection-background = {selection_bg}

# ANSI colors
palette = 0={black}
palette = 1={red}
palette = 2={green}
palette = 3={yellow}
palette = 4={blue}
palette = 5={magenta}
palette = 6={cyan}
palette = 7={white}
palette = 8={bright_black}
palette = 9={bright_red}
palette = 10={bright_green}
palette = 11={bright_yellow}
palette = 12={bright_blue}
palette = 13={bright_magenta}
palette = 14={bright_cyan}
palette = 15={bright_white}
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

        TerminalType::Kitty => {
            format!(
                r#"# VT Code Dark Theme for Kitty
background {background}
foreground {foreground}
cursor {cursor}
selection_background {selection_bg}

# ANSI colors
color0 {black}
color1 {red}
color2 {green}
color3 {yellow}
color4 {blue}
color5 {magenta}
color6 {cyan}
color7 {white}

# Bright colors
color8 {bright_black}
color9 {bright_red}
color10 {bright_green}
color11 {bright_yellow}
color12 {bright_blue}
color13 {bright_magenta}
color14 {bright_cyan}
color15 {bright_white}
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

        TerminalType::Alacritty => {
            format!(
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
            )
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
        assert!(config.contains("palette"));
        assert!(config.contains("#1e1e1e"));
    }

    #[test]
    fn test_generate_kitty_config() {
        let config = generate_config(TerminalType::Kitty).unwrap();
        assert!(config.contains("color0"));
        assert!(config.contains("color15"));
    }

    #[test]
    fn test_generate_alacritty_config() {
        let config = generate_config(TerminalType::Alacritty).unwrap();
        assert!(config.contains("[colors"));
        assert!(config.contains("primary"));
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
}
