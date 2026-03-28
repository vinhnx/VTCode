use anstyle::RgbColor;
use catppuccin::PALETTE;
use hashbrown::HashMap;
use once_cell::sync::Lazy;

use crate::types::{ThemeDefinition, ThemePalette, ThemeSuite};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum CatppuccinFlavorKind {
    Latte,
    Frappe,
    Macchiato,
    Mocha,
}

impl CatppuccinFlavorKind {
    const fn id(self) -> &'static str {
        match self {
            Self::Latte => "catppuccin-latte",
            Self::Frappe => "catppuccin-frappe",
            Self::Macchiato => "catppuccin-macchiato",
            Self::Mocha => "catppuccin-mocha",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Latte => "Catppuccin Latte",
            Self::Frappe => "Catppuccin Frappé",
            Self::Macchiato => "Catppuccin Macchiato",
            Self::Mocha => "Catppuccin Mocha",
        }
    }

    fn flavor(self) -> catppuccin::Flavor {
        match self {
            Self::Latte => PALETTE.latte,
            Self::Frappe => PALETTE.frappe,
            Self::Macchiato => PALETTE.macchiato,
            Self::Mocha => PALETTE.mocha,
        }
    }
}

static CATPPUCCIN_FLAVORS: &[CatppuccinFlavorKind] = &[
    CatppuccinFlavorKind::Latte,
    CatppuccinFlavorKind::Frappe,
    CatppuccinFlavorKind::Macchiato,
    CatppuccinFlavorKind::Mocha,
];

static REGISTRY: Lazy<HashMap<&'static str, ThemeDefinition>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "ciapre-dark",
        ThemeDefinition {
            id: "ciapre-dark",
            label: "Ciapre Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                background: RgbColor(0x26, 0x26, 0x26),
                foreground: RgbColor(0xBF, 0xB3, 0x8F),
                secondary_accent: RgbColor(0xD9, 0x9A, 0x4E),
                alert: RgbColor(0xFF, 0x8A, 0x8A),
                logo_accent: RgbColor(0xD9, 0x9A, 0x4E),
            },
        },
    );
    map.insert(
        "ciapre-blue",
        ThemeDefinition {
            id: "ciapre-blue",
            label: "Ciapre Blue",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                background: RgbColor(0x17, 0x1C, 0x26),
                foreground: RgbColor(0xBF, 0xB3, 0x8F),
                secondary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                alert: RgbColor(0xFF, 0x8A, 0x8A),
                logo_accent: RgbColor(0xD9, 0x9A, 0x4E),
            },
        },
    );
    map.insert(
        "ciapre",
        ThemeDefinition {
            id: "ciapre",
            label: "Ciapre",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAE, 0xA4, 0x7F),
                background: RgbColor(0x18, 0x18, 0x18),
                foreground: RgbColor(0xAE, 0xA4, 0x7F),
                secondary_accent: RgbColor(0xCC, 0x8A, 0x3E),
                alert: RgbColor(0xAC, 0x38, 0x35),
                logo_accent: RgbColor(0xCC, 0x8A, 0x3E),
            },
        },
    );
    map.insert(
        "solarized-dark",
        ThemeDefinition {
            id: "solarized-dark",
            label: "Solarized Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0x83, 0x94, 0x96),
                background: RgbColor(0x00, 0x2B, 0x36),
                foreground: RgbColor(0x83, 0x94, 0x96),
                secondary_accent: RgbColor(0x26, 0x8B, 0xD2),
                alert: RgbColor(0xDC, 0x32, 0x2F),
                logo_accent: RgbColor(0xB5, 0x89, 0x00),
            },
        },
    );
    map.insert(
        "solarized-light",
        ThemeDefinition {
            id: "solarized-light",
            label: "Solarized Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x58, 0x6E, 0x75),
                background: RgbColor(0xFD, 0xF6, 0xE3),
                foreground: RgbColor(0x58, 0x6E, 0x75),
                secondary_accent: RgbColor(0x26, 0x8B, 0xD2),
                alert: RgbColor(0xDC, 0x32, 0x2F),
                logo_accent: RgbColor(0xB5, 0x89, 0x00),
            },
        },
    );
    map.insert(
        "solarized-dark-hc",
        ThemeDefinition {
            id: "solarized-dark-hc",
            label: "Solarized Dark Higher Contrast",
            palette: ThemePalette {
                primary_accent: RgbColor(0x83, 0x94, 0x96),
                background: RgbColor(0x00, 0x28, 0x31),
                foreground: RgbColor(0xE9, 0xE3, 0xCC),
                secondary_accent: RgbColor(0x20, 0x76, 0xC7),
                alert: RgbColor(0xD1, 0x1C, 0x24),
                logo_accent: RgbColor(0xA5, 0x77, 0x06),
            },
        },
    );
    map.insert(
        "gruvbox-dark",
        ThemeDefinition {
            id: "gruvbox-dark",
            label: "Gruvbox Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xA8, 0x99, 0x84),
                background: RgbColor(0x28, 0x28, 0x28),
                foreground: RgbColor(0xA8, 0x99, 0x84),
                secondary_accent: RgbColor(0x45, 0x85, 0x88),
                alert: RgbColor(0xCC, 0x24, 0x1D),
                logo_accent: RgbColor(0xD7, 0x99, 0x21),
            },
        },
    );
    map.insert(
        "gruvbox-dark-hard",
        ThemeDefinition {
            id: "gruvbox-dark-hard",
            label: "Gruvbox Dark Hard",
            palette: ThemePalette {
                primary_accent: RgbColor(0xA8, 0x99, 0x84),
                background: RgbColor(0x1D, 0x20, 0x21),
                foreground: RgbColor(0xA8, 0x99, 0x84),
                secondary_accent: RgbColor(0x45, 0x85, 0x88),
                alert: RgbColor(0xCC, 0x24, 0x1D),
                logo_accent: RgbColor(0xD7, 0x99, 0x21),
            },
        },
    );
    map.insert(
        "gruvbox-light",
        ThemeDefinition {
            id: "gruvbox-light",
            label: "Gruvbox Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x7C, 0x6F, 0x64),
                background: RgbColor(0xFB, 0xF4, 0xE8),
                foreground: RgbColor(0x7C, 0x6F, 0x64),
                secondary_accent: RgbColor(0x45, 0x85, 0x88),
                alert: RgbColor(0x9D, 0x00, 0x06),
                logo_accent: RgbColor(0xB5, 0x76, 0x14),
            },
        },
    );
    map.insert(
        "gruvbox-light-hard",
        ThemeDefinition {
            id: "gruvbox-light-hard",
            label: "Gruvbox Light Hard",
            palette: ThemePalette {
                primary_accent: RgbColor(0x50, 0x49, 0x45),
                background: RgbColor(0xF9, 0xF5, 0xD7),
                foreground: RgbColor(0x50, 0x49, 0x45),
                secondary_accent: RgbColor(0x45, 0x85, 0x88),
                alert: RgbColor(0x9D, 0x00, 0x06),
                logo_accent: RgbColor(0xB5, 0x76, 0x14),
            },
        },
    );
    map.insert(
        "gruvbox-material",
        ThemeDefinition {
            id: "gruvbox-material",
            label: "Gruvbox Material",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD4, 0xBE, 0x98),
                background: RgbColor(0x28, 0x2E, 0x33),
                foreground: RgbColor(0xD4, 0xBE, 0x98),
                secondary_accent: RgbColor(0x89, 0xB4, 0x82),
                alert: RgbColor(0xEA, 0x69, 0x62),
                logo_accent: RgbColor(0xE7, 0xA7, 0x2F),
            },
        },
    );
    map.insert(
        "gruvbox-material-dark",
        ThemeDefinition {
            id: "gruvbox-material-dark",
            label: "Gruvbox Material Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD4, 0xBE, 0x98),
                background: RgbColor(0x28, 0x2E, 0x33),
                foreground: RgbColor(0xD4, 0xBE, 0x98),
                secondary_accent: RgbColor(0x89, 0xB4, 0x82),
                alert: RgbColor(0xEA, 0x69, 0x62),
                logo_accent: RgbColor(0xE7, 0xA7, 0x2F),
            },
        },
    );
    map.insert(
        "gruvbox-material-light",
        ThemeDefinition {
            id: "gruvbox-material-light",
            label: "Gruvbox Material Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x65, 0x49, 0x31),
                background: RgbColor(0xFB, 0xF1, 0xC7),
                foreground: RgbColor(0x65, 0x49, 0x31),
                secondary_accent: RgbColor(0x4C, 0x7A, 0x5D),
                alert: RgbColor(0xC1, 0x4A, 0x4A),
                logo_accent: RgbColor(0xB4, 0x71, 0x09),
            },
        },
    );
    map.insert(
        "ayu",
        ThemeDefinition {
            id: "ayu",
            label: "Ayu Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xE6, 0xE1, 0xCF),
                background: RgbColor(0x0F, 0x14, 0x19),
                foreground: RgbColor(0xE6, 0xE1, 0xCF),
                secondary_accent: RgbColor(0x39, 0xBA, 0xE6),
                alert: RgbColor(0xFF, 0x33, 0x33),
                logo_accent: RgbColor(0xE6, 0xB4, 0x50),
            },
        },
    );
    map.insert(
        "ayu-mirage",
        ThemeDefinition {
            id: "ayu-mirage",
            label: "Ayu Mirage",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCB, 0xCC, 0xCC),
                background: RgbColor(0x1F, 0x24, 0x2D),
                foreground: RgbColor(0xCB, 0xCC, 0xCC),
                secondary_accent: RgbColor(0x5C, 0xC5, 0xFF),
                alert: RgbColor(0xFF, 0x66, 0x66),
                logo_accent: RgbColor(0xFF, 0xCC, 0x66),
            },
        },
    );
    map.insert(
        "dracula",
        ThemeDefinition {
            id: "dracula",
            label: "Dracula",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xF8, 0xF2),
                background: RgbColor(0x28, 0x2A, 0x36),
                foreground: RgbColor(0xF8, 0xF8, 0xF2),
                secondary_accent: RgbColor(0x8B, 0xE9, 0xFD),
                alert: RgbColor(0xFF, 0x55, 0x55),
                logo_accent: RgbColor(0xFF, 0xB8, 0x6C),
            },
        },
    );
    map.insert(
        "github-dark",
        ThemeDefinition {
            id: "github-dark",
            label: "GitHub Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC9, 0xD1, 0xD9),
                background: RgbColor(0x0D, 0x11, 0x17),
                foreground: RgbColor(0xC9, 0xD1, 0xD9),
                secondary_accent: RgbColor(0x58, 0xA6, 0xFF),
                alert: RgbColor(0xF8, 0x51, 0x49),
                logo_accent: RgbColor(0xD2, 0x99, 0x22),
            },
        },
    );
    map.insert(
        "github",
        ThemeDefinition {
            id: "github",
            label: "GitHub Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x24, 0x29, 0x2E),
                background: RgbColor(0xFF, 0xFF, 0xFF),
                foreground: RgbColor(0x24, 0x29, 0x2E),
                secondary_accent: RgbColor(0x03, 0x66, 0xD6),
                alert: RgbColor(0xD7, 0x3A, 0x49),
                logo_accent: RgbColor(0xB0, 0x88, 0x00),
            },
        },
    );
    map.insert(
        "atom-one-dark",
        ThemeDefinition {
            id: "atom-one-dark",
            label: "Atom One Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAB, 0xB2, 0xBF),
                background: RgbColor(0x28, 0x2C, 0x34),
                foreground: RgbColor(0xAB, 0xB2, 0xBF),
                secondary_accent: RgbColor(0x61, 0xAF, 0xEF),
                alert: RgbColor(0xE0, 0x6C, 0x75),
                logo_accent: RgbColor(0xE5, 0xC0, 0x7B),
            },
        },
    );
    map.insert(
        "atom-one-light",
        ThemeDefinition {
            id: "atom-one-light",
            label: "Atom One Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x38, 0x3A, 0x42),
                background: RgbColor(0xFA, 0xFA, 0xFA),
                foreground: RgbColor(0x38, 0x3A, 0x42),
                secondary_accent: RgbColor(0x40, 0x73, 0xC9),
                alert: RgbColor(0xE4, 0x56, 0x49),
                logo_accent: RgbColor(0x98, 0x62, 0x00),
            },
        },
    );
    map.insert(
        "tomorrow",
        ThemeDefinition {
            id: "tomorrow",
            label: "Tomorrow",
            palette: ThemePalette {
                primary_accent: RgbColor(0x4D, 0x4D, 0x4C),
                background: RgbColor(0xFF, 0xFF, 0xFF),
                foreground: RgbColor(0x4D, 0x4D, 0x4C),
                secondary_accent: RgbColor(0x42, 0x7B, 0xA4),
                alert: RgbColor(0xC8, 0x28, 0x29),
                logo_accent: RgbColor(0xAE, 0x7B, 0x03),
            },
        },
    );
    map.insert(
        "tomorrow-night",
        ThemeDefinition {
            id: "tomorrow-night",
            label: "Tomorrow Night",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC5, 0xC8, 0xC6),
                background: RgbColor(0x1D, 0x1F, 0x21),
                foreground: RgbColor(0xC5, 0xC8, 0xC6),
                secondary_accent: RgbColor(0x81, 0xA2, 0xBE),
                alert: RgbColor(0xCC, 0x66, 0x66),
                logo_accent: RgbColor(0xDE, 0x93, 0x5F),
            },
        },
    );
    map.insert(
        "tomorrow-night-blue",
        ThemeDefinition {
            id: "tomorrow-night-blue",
            label: "Tomorrow Night Blue",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF),
                background: RgbColor(0x00, 0x2E, 0x4E),
                foreground: RgbColor(0xFF, 0xFF, 0xFF),
                secondary_accent: RgbColor(0x7A, 0xBD, 0xFF),
                alert: RgbColor(0xFF, 0x9D, 0x9D),
                logo_accent: RgbColor(0xFF, 0xC8, 0x80),
            },
        },
    );
    map.insert(
        "tomorrow-night-bright",
        ThemeDefinition {
            id: "tomorrow-night-bright",
            label: "Tomorrow Night Bright",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDE, 0xDE, 0xDE),
                background: RgbColor(0x00, 0x00, 0x00),
                foreground: RgbColor(0xDE, 0xDE, 0xDE),
                secondary_accent: RgbColor(0x7A, 0xBD, 0xFF),
                alert: RgbColor(0xFF, 0x87, 0x87),
                logo_accent: RgbColor(0xFF, 0xC8, 0x80),
            },
        },
    );
    map.insert(
        "tomorrow-night-eighties",
        ThemeDefinition {
            id: "tomorrow-night-eighties",
            label: "Tomorrow Night Eighties",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCC, 0xCC, 0xCC),
                background: RgbColor(0x2D, 0x2D, 0x2D),
                foreground: RgbColor(0xCC, 0xCC, 0xCC),
                secondary_accent: RgbColor(0x66, 0x99, 0xCC),
                alert: RgbColor(0xF2, 0x77, 0x7A),
                logo_accent: RgbColor(0xF9, 0x91, 0x57),
            },
        },
    );
    map.insert(
        "tomorrow-night-burns",
        ThemeDefinition {
            id: "tomorrow-night-burns",
            label: "Tomorrow Night Burns",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD0, 0xD0, 0xD0),
                background: RgbColor(0x15, 0x12, 0x0E),
                foreground: RgbColor(0xD0, 0xD0, 0xD0),
                secondary_accent: RgbColor(0x6C, 0x99, 0xB4),
                alert: RgbColor(0xB7, 0x4E, 0x4E),
                logo_accent: RgbColor(0xC4, 0x8D, 0x53),
            },
        },
    );
    map.insert(
        "material-ocean",
        ThemeDefinition {
            id: "material-ocean",
            label: "Material Ocean",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEE, 0xEE, 0xEE),
                background: RgbColor(0x0F, 0x11, 0x1A),
                foreground: RgbColor(0xEE, 0xEE, 0xEE),
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF),
                alert: RgbColor(0xF0, 0x71, 0x78),
                logo_accent: RgbColor(0xFF, 0xCB, 0x6B),
            },
        },
    );
    map.insert(
        "material-dark",
        ThemeDefinition {
            id: "material-dark",
            label: "Material Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEE, 0xEE, 0xEE),
                background: RgbColor(0x26, 0x32, 0x38),
                foreground: RgbColor(0xEE, 0xEE, 0xEE),
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF),
                alert: RgbColor(0xF0, 0x71, 0x78),
                logo_accent: RgbColor(0xFF, 0xCB, 0x6B),
            },
        },
    );
    map.insert(
        "material",
        ThemeDefinition {
            id: "material",
            label: "Material",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEE, 0xEE, 0xEE),
                background: RgbColor(0x26, 0x32, 0x38),
                foreground: RgbColor(0xEE, 0xEE, 0xEE),
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF),
                alert: RgbColor(0xF0, 0x71, 0x78),
                logo_accent: RgbColor(0xFF, 0xCB, 0x6B),
            },
        },
    );
    map.insert(
        "monokai-classic",
        ThemeDefinition {
            id: "monokai-classic",
            label: "Monokai Classic",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xF8, 0xF2),
                background: RgbColor(0x27, 0x28, 0x22),
                foreground: RgbColor(0xF8, 0xF8, 0xF2),
                secondary_accent: RgbColor(0x66, 0xD9, 0xEF),
                alert: RgbColor(0xF9, 0x26, 0x72),
                logo_accent: RgbColor(0xFD, 0x97, 0x1F),
            },
        },
    );
    map.insert(
        "night-owl",
        ThemeDefinition {
            id: "night-owl",
            label: "Night Owl",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD6, 0xDE, 0xEB),
                background: RgbColor(0x01, 0x17, 0x27),
                foreground: RgbColor(0xD6, 0xDE, 0xEB),
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF),
                alert: RgbColor(0xEF, 0x53, 0x50),
                logo_accent: RgbColor(0xFA, 0xC8, 0x63),
            },
        },
    );
    map.insert(
        "zenburn",
        ThemeDefinition {
            id: "zenburn",
            label: "Zenburn",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDC, 0xDC, 0xCC),
                background: RgbColor(0x3F, 0x3F, 0x3F),
                foreground: RgbColor(0xDC, 0xDC, 0xCC),
                secondary_accent: RgbColor(0x8C, 0xA8, 0x7D),
                alert: RgbColor(0xCC, 0x93, 0x93),
                logo_accent: RgbColor(0xDF, 0xAF, 0x8F),
            },
        },
    );
    map.insert(
        "jetbrains-darcula",
        ThemeDefinition {
            id: "jetbrains-darcula",
            label: "JetBrains Darcula",
            palette: ThemePalette {
                primary_accent: RgbColor(0xA9, 0xB7, 0xC6),
                background: RgbColor(0x2B, 0x2B, 0x2B),
                foreground: RgbColor(0xA9, 0xB7, 0xC6),
                secondary_accent: RgbColor(0x68, 0x8B, 0xB5),
                alert: RgbColor(0xCC, 0x78, 0x75),
                logo_accent: RgbColor(0xBB, 0xA2, 0x6C),
            },
        },
    );
    map.insert(
        "spacegray",
        ThemeDefinition {
            id: "spacegray",
            label: "Spacegray",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBD, 0xC3, 0xCE),
                background: RgbColor(0x20, 0x22, 0x2B),
                foreground: RgbColor(0xBD, 0xC3, 0xCE),
                secondary_accent: RgbColor(0x7F, 0xA0, 0xC0),
                alert: RgbColor(0xB0, 0x6B, 0x6B),
                logo_accent: RgbColor(0xC0, 0x99, 0x70),
            },
        },
    );
    map.insert(
        "spacegray-bright",
        ThemeDefinition {
            id: "spacegray-bright",
            label: "Spacegray Bright",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF2, 0xF2, 0xF2),
                background: RgbColor(0x1A, 0x1A, 0x1A),
                foreground: RgbColor(0xF2, 0xF2, 0xF2),
                secondary_accent: RgbColor(0x88, 0xB0, 0xD0),
                alert: RgbColor(0xD0, 0x70, 0x70),
                logo_accent: RgbColor(0xD0, 0xA0, 0x60),
            },
        },
    );
    map.insert(
        "spacegray-eighties",
        ThemeDefinition {
            id: "spacegray-eighties",
            label: "Spacegray Eighties",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEC, 0xEC, 0xEC),
                background: RgbColor(0x22, 0x22, 0x2B),
                foreground: RgbColor(0xEC, 0xEC, 0xEC),
                secondary_accent: RgbColor(0x7A, 0x9F, 0xC2),
                alert: RgbColor(0xC7, 0x6B, 0x6B),
                logo_accent: RgbColor(0xC2, 0x95, 0x62),
            },
        },
    );
    map.insert(
        "spacegray-eighties-dull",
        ThemeDefinition {
            id: "spacegray-eighties-dull",
            label: "Spacegray Eighties Dull",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC2, 0xC8, 0xD0),
                background: RgbColor(0x2D, 0x30, 0x3A),
                foreground: RgbColor(0xC2, 0xC8, 0xD0),
                secondary_accent: RgbColor(0x6E, 0x8F, 0xB0),
                alert: RgbColor(0xB0, 0x6A, 0x6A),
                logo_accent: RgbColor(0xB0, 0x8C, 0x60),
            },
        },
    );
    map.insert(
        "apple-classic",
        ThemeDefinition {
            id: "apple-classic",
            label: "Apple Classic",
            palette: ThemePalette {
                primary_accent: RgbColor(0x00, 0x00, 0x00),
                background: RgbColor(0xF6, 0xF4, 0xEC),
                foreground: RgbColor(0x00, 0x00, 0x00),
                secondary_accent: RgbColor(0x00, 0x66, 0x99),
                alert: RgbColor(0xB0, 0x00, 0x00),
                logo_accent: RgbColor(0x8B, 0x6F, 0x00),
            },
        },
    );
    map.insert(
        "apple-system-colors",
        ThemeDefinition {
            id: "apple-system-colors",
            label: "Apple System Colors",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF5, 0xF5, 0xF7),
                background: RgbColor(0x1C, 0x1C, 0x1E),
                foreground: RgbColor(0xF5, 0xF5, 0xF7),
                secondary_accent: RgbColor(0x0A, 0x84, 0xFF),
                alert: RgbColor(0xFF, 0x45, 0x3A),
                logo_accent: RgbColor(0xFF, 0x9F, 0x0A),
            },
        },
    );
    map.insert(
        "apple-system-colors-light",
        ThemeDefinition {
            id: "apple-system-colors-light",
            label: "Apple System Colors Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x1C, 0x1C, 0x1E),
                background: RgbColor(0xF5, 0xF5, 0xF7),
                foreground: RgbColor(0x1C, 0x1C, 0x1E),
                secondary_accent: RgbColor(0x0A, 0x84, 0xFF),
                alert: RgbColor(0xFF, 0x3B, 0x30),
                logo_accent: RgbColor(0xFF, 0x95, 0x00),
            },
        },
    );
    map.insert(
        "vitesse-black",
        ThemeDefinition {
            id: "vitesse-black",
            label: "Vitesse Black",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA),
                background: RgbColor(0x12, 0x12, 0x12),
                foreground: RgbColor(0xDB, 0xD7, 0xCA),
                secondary_accent: RgbColor(0x4D, 0x93, 0x8F),
                alert: RgbColor(0xAB, 0x59, 0x59),
                logo_accent: RgbColor(0xFF, 0xA6, 0x57),
            },
        },
    );
    map.insert(
        "vitesse-dark",
        ThemeDefinition {
            id: "vitesse-dark",
            label: "Vitesse Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA),
                background: RgbColor(0x18, 0x18, 0x18),
                foreground: RgbColor(0xDB, 0xD7, 0xCA),
                secondary_accent: RgbColor(0x4D, 0x93, 0x8F),
                alert: RgbColor(0xAB, 0x59, 0x59),
                logo_accent: RgbColor(0xFF, 0xA6, 0x57),
            },
        },
    );
    map.insert(
        "vitesse-dark-soft",
        ThemeDefinition {
            id: "vitesse-dark-soft",
            label: "Vitesse Dark Soft",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA),
                background: RgbColor(0x20, 0x1F, 0x1F),
                foreground: RgbColor(0xDB, 0xD7, 0xCA),
                secondary_accent: RgbColor(0x4D, 0x93, 0x8F),
                alert: RgbColor(0xAB, 0x59, 0x59),
                logo_accent: RgbColor(0xFF, 0xA6, 0x57),
            },
        },
    );
    map.insert(
        "vitesse-light",
        ThemeDefinition {
            id: "vitesse-light",
            label: "Vitesse Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x39, 0x3A, 0x34),
                background: RgbColor(0xF6, 0xF1, 0xE5),
                foreground: RgbColor(0x39, 0x3A, 0x34),
                secondary_accent: RgbColor(0x1C, 0x6B, 0x48),
                alert: RgbColor(0xAB, 0x59, 0x59),
                logo_accent: RgbColor(0xB0, 0x70, 0x00),
            },
        },
    );
    map.insert(
        "vitesse-light-soft",
        ThemeDefinition {
            id: "vitesse-light-soft",
            label: "Vitesse Light Soft",
            palette: ThemePalette {
                primary_accent: RgbColor(0x39, 0x3A, 0x34),
                background: RgbColor(0xFA, 0xF5, 0xEB),
                foreground: RgbColor(0x39, 0x3A, 0x34),
                secondary_accent: RgbColor(0x1C, 0x6B, 0x48),
                alert: RgbColor(0xAB, 0x59, 0x59),
                logo_accent: RgbColor(0xB0, 0x70, 0x00),
            },
        },
    );
    map.insert(
        "homebrew",
        ThemeDefinition {
            id: "homebrew",
            label: "Homebrew",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCC, 0xCC, 0xCC),
                background: RgbColor(0x00, 0x00, 0x00),
                foreground: RgbColor(0xCC, 0xCC, 0xCC),
                secondary_accent: RgbColor(0x00, 0x99, 0xCC),
                alert: RgbColor(0xCC, 0x33, 0x33),
                logo_accent: RgbColor(0xCC, 0x99, 0x00),
            },
        },
    );
    map.insert(
        "man-page",
        ThemeDefinition {
            id: "man-page",
            label: "Man Page",
            palette: ThemePalette {
                primary_accent: RgbColor(0x00, 0x00, 0x00),
                background: RgbColor(0xF2, 0xF2, 0xE6),
                foreground: RgbColor(0x00, 0x00, 0x00),
                secondary_accent: RgbColor(0x00, 0x66, 0x66),
                alert: RgbColor(0x99, 0x00, 0x00),
                logo_accent: RgbColor(0x7A, 0x5D, 0x00),
            },
        },
    );
    map.insert(
        "framer",
        ThemeDefinition {
            id: "framer",
            label: "Framer",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF5, 0xF5, 0xF5),
                background: RgbColor(0x10, 0x10, 0x10),
                foreground: RgbColor(0xF5, 0xF5, 0xF5),
                secondary_accent: RgbColor(0x7A, 0x5A, 0xFF),
                alert: RgbColor(0xFF, 0x5C, 0x5C),
                logo_accent: RgbColor(0xFF, 0xAF, 0x3F),
            },
        },
    );
    map.insert(
        "espresso",
        ThemeDefinition {
            id: "espresso",
            label: "Espresso",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD0, 0xC8, 0xB8),
                background: RgbColor(0x2A, 0x21, 0x1C),
                foreground: RgbColor(0xD0, 0xC8, 0xB8),
                secondary_accent: RgbColor(0x6F, 0xB3, 0xC2),
                alert: RgbColor(0xD2, 0x52, 0x52),
                logo_accent: RgbColor(0xC8, 0x92, 0x2D),
            },
        },
    );
    map.insert(
        "adventure-time",
        ThemeDefinition {
            id: "adventure-time",
            label: "Adventure Time",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xF8, 0xF8),
                background: RgbColor(0x1F, 0x1D, 0x45),
                foreground: RgbColor(0xF8, 0xF8, 0xF8),
                secondary_accent: RgbColor(0x8A, 0x9B, 0xFF),
                alert: RgbColor(0xD7, 0x56, 0x56),
                logo_accent: RgbColor(0xF5, 0xB8, 0x4A),
            },
        },
    );
    map.insert(
        "afterglow",
        ThemeDefinition {
            id: "afterglow",
            label: "Afterglow",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD6, 0xD6, 0xD6),
                background: RgbColor(0x22, 0x22, 0x22),
                foreground: RgbColor(0xD6, 0xD6, 0xD6),
                secondary_accent: RgbColor(0x7F, 0xB0, 0xD0),
                alert: RgbColor(0xE5, 0x72, 0x72),
                logo_accent: RgbColor(0xD0, 0xA0, 0x60),
            },
        },
    );
    map.insert(
        "mono",
        ThemeDefinition {
            id: "mono",
            label: "Mono",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF),
                background: RgbColor(0x00, 0x00, 0x00),
                foreground: RgbColor(0xDB, 0xD7, 0xCA),
                secondary_accent: RgbColor(0xBB, 0xBB, 0xBB),
                alert: RgbColor(0xFF, 0xFF, 0xFF),
                logo_accent: RgbColor(0xFF, 0xFF, 0xFF),
            },
        },
    );
    map.insert(
        "ansi-classic",
        ThemeDefinition {
            id: "ansi-classic",
            label: "ANSI Classic",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC0, 0xC0, 0xC0),
                background: RgbColor(0x00, 0x00, 0x00),
                foreground: RgbColor(0xC0, 0xC0, 0xC0),
                secondary_accent: RgbColor(0x00, 0xAA, 0xAA),
                alert: RgbColor(0xAA, 0x00, 0x00),
                logo_accent: RgbColor(0xAA, 0xAA, 0x00),
            },
        },
    );

    register_catppuccin_themes(&mut map);
    map
});

fn register_catppuccin_themes(map: &mut HashMap<&'static str, ThemeDefinition>) {
    for &flavor_kind in CATPPUCCIN_FLAVORS {
        let flavor = flavor_kind.flavor();
        map.insert(
            flavor_kind.id(),
            ThemeDefinition {
                id: flavor_kind.id(),
                label: flavor_kind.label(),
                palette: catppuccin_palette(flavor),
            },
        );
    }
}

fn catppuccin_palette(flavor: catppuccin::Flavor) -> ThemePalette {
    let colors = flavor.colors;
    ThemePalette {
        primary_accent: catppuccin_rgb(colors.lavender),
        background: catppuccin_rgb(colors.base),
        foreground: catppuccin_rgb(colors.text),
        secondary_accent: catppuccin_rgb(colors.sapphire),
        alert: catppuccin_rgb(colors.red),
        logo_accent: catppuccin_rgb(colors.peach),
    }
}

fn catppuccin_rgb(color: catppuccin::Color) -> RgbColor {
    RgbColor(color.rgb.r, color.rgb.g, color.rgb.b)
}

pub(crate) fn theme_definition(theme_id: &str) -> Option<&'static ThemeDefinition> {
    REGISTRY.get(theme_id)
}

#[cfg(test)]
pub(crate) fn all_theme_definitions() -> &'static HashMap<&'static str, ThemeDefinition> {
    &REGISTRY
}

/// Return the list of built-in theme identifiers in sorted order.
pub fn available_themes() -> Vec<&'static str> {
    let mut keys: Vec<_> = REGISTRY.keys().copied().collect();
    keys.sort();
    keys
}

/// Return the display label for a built-in theme.
pub fn theme_label(theme_id: &str) -> Option<&'static str> {
    theme_definition(theme_id).map(|definition| definition.label)
}

fn suite_id_for_theme(theme_id: &str) -> Option<&'static str> {
    if theme_id.starts_with("catppuccin-") {
        Some("catppuccin")
    } else if theme_id.starts_with("vitesse-") {
        Some("vitesse")
    } else if theme_id.starts_with("ciapre-") {
        Some("ciapre")
    } else if theme_id == "mono" {
        Some("mono")
    } else {
        None
    }
}

fn suite_label(suite_id: &str) -> Option<&'static str> {
    match suite_id {
        "catppuccin" => Some("Catppuccin"),
        "vitesse" => Some("Vitesse"),
        "ciapre" => Some("Ciapre"),
        "mono" => Some("Mono"),
        _ => None,
    }
}

/// Return the logical theme suite identifier for a built-in theme.
pub fn theme_suite_id(theme_id: &str) -> Option<&'static str> {
    suite_id_for_theme(theme_id)
}

/// Return the logical theme suite label for a built-in theme.
pub fn theme_suite_label(theme_id: &str) -> Option<&'static str> {
    suite_id_for_theme(theme_id).and_then(suite_label)
}

/// Return the built-in theme suites and their member theme identifiers.
pub fn available_theme_suites() -> Vec<ThemeSuite> {
    const ORDER: &[&str] = &["ciapre", "vitesse", "catppuccin", "mono"];

    ORDER
        .iter()
        .filter_map(|suite_id| {
            let mut theme_ids: Vec<&'static str> = available_themes()
                .into_iter()
                .filter(|theme_id| suite_id_for_theme(theme_id) == Some(*suite_id))
                .collect();
            if theme_ids.is_empty() {
                return None;
            }
            theme_ids.sort_unstable();
            Some(ThemeSuite {
                id: suite_id,
                label: suite_label(suite_id)?,
                theme_ids,
            })
        })
        .collect()
}
