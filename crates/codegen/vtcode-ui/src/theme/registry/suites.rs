use super::*;

pub(crate) fn theme_definition(theme_id: &str) -> Option<&'static ThemeDefinition> {
    REGISTRY.get(theme_id)
}

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
    // Multi-variant theme sets group by their upstream (gpui-kit) theme set;
    // single-theme sets stay standalone so scheme-twin lookup cannot suggest
    // an unrelated palette.
    for set in GpuiKitThemeSet::ALL {
        if set.member_ids().contains(&theme_id) {
            return Some(set.suite_id());
        }
    }
    match theme_id {
        "nord" => Some("nord"),
        "mono" => Some("mono"),
        _ => None,
    }
}

fn suite_label(suite_id: &str) -> Option<&'static str> {
    let fixed = match suite_id {
        "mono" => Some("Mono"),
        "nord" => Some("Nord"),
        _ => None,
    };
    fixed.or_else(|| {
        GpuiKitThemeSet::ALL
            .iter()
            .find(|set| set.suite_id() == suite_id)
            .map(|set| set.label())
    })
}

/// Return the logical theme suite identifier for a built-in theme.
pub fn theme_suite_id(theme_id: &str) -> Option<&'static str> {
    suite_id_for_theme(theme_id)
}

/// Return the logical theme suite label for a built-in theme.
pub fn theme_suite_label(theme_id: &str) -> Option<&'static str> {
    suite_id_for_theme(theme_id).and_then(suite_label)
}

/// Grouping of the gpui-kit theme variants into selectable theme suites.
///
/// A suite is only meaningful when it carries both a light and a dark variant
/// (or is a deliberately curated family like Ciapre); single-theme sets stay
/// standalone so scheme-twin selection never suggests an unrelated palette.
enum GpuiKitThemeSet {
    Ciapre,
    Vitesse,
    Catppuccin,
    GpuiKit {
        suite_id: &'static str,
        label: &'static str,
        ids: &'static [&'static str],
    },
}

impl GpuiKitThemeSet {
    const ALL: &'static [Self] = &[
        Self::Ciapre,
        Self::Vitesse,
        Self::Catppuccin,
        Self::GpuiKit {
            suite_id: "adventure",
            label: "Adventure",
            ids: &["adventure", "adventure-time"],
        },
        Self::GpuiKit {
            suite_id: "ayu",
            label: "Ayu",
            ids: &["ayu-dark", "ayu-light"],
        },
        Self::GpuiKit {
            suite_id: "everforest",
            label: "Everforest",
            ids: &["everforest-dark", "everforest-light"],
        },
        Self::GpuiKit {
            suite_id: "flexoki",
            label: "Flexoki",
            ids: &["flexoki-dark", "flexoki-light"],
        },
        Self::GpuiKit {
            suite_id: "gruvbox",
            label: "Gruvbox",
            ids: &[
                "gruvbox-dark",
                "gruvbox-dark-hard",
                "gruvbox-light",
                "gruvbox-light-hard",
                "gruvbox-material",
                "gruvbox-material-dark",
                "gruvbox-material-light",
            ],
        },
        Self::GpuiKit {
            suite_id: "hybrid",
            label: "Hybrid",
            ids: &["hybrid-dark", "hybrid-light"],
        },
        Self::GpuiKit {
            suite_id: "macos-classic",
            label: "macOS Classic",
            ids: &["macos-classic-dark", "macos-classic-light"],
        },
        Self::GpuiKit {
            suite_id: "mellifluous",
            label: "Mellifluous",
            ids: &["mellifluous-dark", "mellifluous-light"],
        },
        Self::GpuiKit {
            suite_id: "molokai",
            label: "Molokai",
            ids: &["molokai-dark", "molokai-light"],
        },
        Self::GpuiKit {
            suite_id: "solarized",
            label: "Solarized",
            ids: &["solarized-dark", "solarized-dark-hc", "solarized-light"],
        },
        Self::GpuiKit {
            suite_id: "tokyo-night",
            label: "Tokyo Night",
            ids: &["tokyo-moon", "tokyo-night", "tokyo-storm"],
        },
    ];

    fn suite_id(&self) -> &'static str {
        match self {
            Self::Ciapre => "ciapre",
            Self::Vitesse => "vitesse",
            Self::Catppuccin => "catppuccin",
            Self::GpuiKit { suite_id, .. } => suite_id,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Ciapre => "Ciapre",
            Self::Vitesse => "Vitesse",
            Self::Catppuccin => "Catppuccin",
            Self::GpuiKit { label, .. } => label,
        }
    }

    fn member_ids(&self) -> &'static [&'static str] {
        match self {
            Self::Ciapre => &["ciapre", "ciapre-blue", "ciapre-dark"],
            Self::Vitesse => &[
                "vitesse-black",
                "vitesse-dark",
                "vitesse-dark-soft",
                "vitesse-light",
                "vitesse-light-soft",
            ],
            Self::Catppuccin => &[
                "catppuccin-frappe",
                "catppuccin-latte",
                "catppuccin-macchiato",
                "catppuccin-mocha",
            ],
            Self::GpuiKit { ids, .. } => ids,
        }
    }
}

/// Return the built-in theme suites and their member theme identifiers.
pub fn available_theme_suites() -> Vec<ThemeSuite> {
    const ORDER: &[&str] = &[
        "ciapre",
        "vitesse",
        "catppuccin",
        "adventure",
        "ayu",
        "everforest",
        "flexoki",
        "gruvbox",
        "hybrid",
        "macos-classic",
        "mellifluous",
        "molokai",
        "solarized",
        "tokyo-night",
        "mono",
        "nord",
    ];

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
