use super::*;

pub(crate) fn insert_curated_themes(map: &mut HashMap<&'static str, ThemeDefinition>) {
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
                alert: RgbColor(0xC1, 0x6A, 0x68),
                logo_accent: RgbColor(0xCC, 0x8A, 0x3E),
            },
        },
    );
    map.insert(
        "noir",
        ThemeDefinition {
            id: "noir",
            label: "Noir",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAE, 0xA4, 0x7F),
                background: RgbColor(0x0A, 0x0A, 0x0B),
                foreground: RgbColor(0xF4, 0xF4, 0xF5),
                secondary_accent: RgbColor(0xCC, 0x8A, 0x3E),
                alert: RgbColor(0xC1, 0x6A, 0x68),
                logo_accent: RgbColor(0xCC, 0x8A, 0x3E),
            },
        },
    );
}
