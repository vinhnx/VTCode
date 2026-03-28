use crate::runtime::active_theme_id;

/// Get the recommended syntax highlighting theme for a given UI theme.
///
/// For code blocks and syntax highlighting:
/// ```rust
/// use vtcode_theme::{active_theme_id, get_syntax_theme_for_ui_theme};
///
/// let ui_theme = active_theme_id();
/// let syntax_theme = get_syntax_theme_for_ui_theme(&ui_theme);
/// assert!(!syntax_theme.is_empty());
/// ```
pub fn get_syntax_theme_for_ui_theme(ui_theme: &str) -> &'static str {
    match ui_theme.to_lowercase().as_str() {
        "ayu" => "ayu-dark",
        "ayu-mirage" => "ayu-mirage",
        "catppuccin-latte" => "catppuccin-latte",
        "catppuccin-frappe" => "catppuccin-frappe",
        "catppuccin-macchiato" => "catppuccin-macchiato",
        "catppuccin-mocha" => "catppuccin-mocha",
        "solarized-dark" | "solarized-dark-hc" => "Solarized (dark)",
        "solarized-light" => "Solarized (light)",
        "gruvbox-dark" | "gruvbox-dark-hard" => "gruvbox-dark",
        "gruvbox-light" | "gruvbox-light-hard" => "gruvbox-light",
        "gruvbox-material" | "gruvbox-material-dark" => "gruvbox-dark",
        "gruvbox-material-light" => "gruvbox-light",
        "tomorrow" => "Tomorrow",
        "tomorrow-night" => "Tomorrow Night",
        "tomorrow-night-blue" => "Tomorrow Night Blue",
        "tomorrow-night-bright" => "Tomorrow Night Bright",
        "tomorrow-night-eighties" => "Tomorrow Night Eighties",
        "tomorrow-night-burns" => "Tomorrow Night",
        "github-dark" => "GitHub Dark",
        "github" => "GitHub",
        "atom-one-dark" => "OneDark",
        "atom-one-light" => "OneLight",
        "atom" => "base16-ocean.dark",
        "spacegray" | "spacegray-bright" | "spacegray-eighties" | "spacegray-eighties-dull" => {
            "base16-ocean.dark"
        }
        "material-ocean" | "material-dark" | "material" => "Material Dark",
        "dracula" => "Dracula",
        "monokai-classic" => "monokai-classic",
        "night-owl" => "Night Owl",
        "zenburn" => "Zenburn",
        "jetbrains-darcula" => "base16-ocean.dark",
        "man-page" => "base16-ocean.dark",
        "homebrew" => "base16-ocean.dark",
        "framer" => "base16-ocean.dark",
        "espresso" => "base16-ocean.dark",
        "adventure-time" => "base16-ocean.dark",
        "afterglow" => "base16-ocean.dark",
        "apple-classic" => "base16-ocean.dark",
        "apple-system-colors" => "base16-ocean.dark",
        "apple-system-colors-light" => "base16-ocean.light",
        "vitesse-light" | "vitesse-light-soft" => "base16-ocean.light",
        "ciapre" | "ciapre-dark" | "ciapre-blue" => "base16-ocean.dark",
        "vitesse-black" | "vitesse-dark" | "vitesse-dark-soft" => "base16-ocean.dark",
        "mono" | "ansi-classic" => "base16-ocean.dark",
        _ => "base16-ocean.dark",
    }
}

pub fn get_active_syntax_theme() -> &'static str {
    get_syntax_theme_for_ui_theme(&active_theme_id())
}
