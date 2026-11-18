//! Theme-picker swatch catalog extracted from the theme registry.

use super::*;

/// One selectable entry in the theme picker: a palette theme or one of the
/// kit's system light/dark bases.
#[derive(Clone, Copy)]
pub(in crate::desktop) enum SwatchEntry {
    System(ThemeMode),
    Palette(DesktopTheme),
}

impl SwatchEntry {
    pub(in crate::desktop) fn id(self) -> String {
        match self {
            Self::System(ThemeMode::Light) => SYSTEM_LIGHT_ID.to_string(),
            Self::System(ThemeMode::Dark) => SYSTEM_DARK_ID.to_string(),
            Self::Palette(theme) => theme.id.to_string(),
        }
    }

    pub(in crate::desktop) fn label(self) -> &'static str {
        match self {
            Self::System(ThemeMode::Light) => "System Light",
            Self::System(ThemeMode::Dark) => "System Dark",
            Self::Palette(theme) => theme.label,
        }
    }

    /// The two colors a picker swatch previews.
    pub(in crate::desktop) fn preview(self) -> (Hsla, Hsla) {
        match self {
            Self::System(ThemeMode::Light) => (rgb(0xFFFFFF).into(), rgb(0x1F2937).into()),
            Self::System(ThemeMode::Dark) => (rgb(0x0F172A).into(), rgb(0xE2E8F0).into()),
            Self::Palette(theme) => (theme.background, theme.foreground),
        }
    }

    pub(in crate::desktop) fn accent(self) -> Option<Hsla> {
        match self {
            Self::System(_) => None,
            Self::Palette(theme) => Some(theme.accent),
        }
    }

    pub(in crate::desktop) fn mode(self) -> ThemeMode {
        match self {
            Self::System(mode) => mode,
            Self::Palette(theme) => theme.mode,
        }
    }
}

/// The two system entries shown first in the picker.
pub(in crate::desktop) const SYSTEM_SWATCHES: [SwatchEntry; 2] = [
    SwatchEntry::System(ThemeMode::Light),
    SwatchEntry::System(ThemeMode::Dark),
];

const FEATURED_THEME_IDS: [&str; 3] = [DEFAULT_THEME_ID, ThemeId::ciapre::DARK, ThemeId::ciapre::BLUE];

/// Picker order within one mode group leads with the VT Code themes of
/// that mode, then the matching system base and the remaining shared
/// palette catalog in its existing order.
pub(in crate::desktop) fn picker_swatches_for(mode: ThemeMode) -> impl Iterator<Item = SwatchEntry> {
    let themes = all_themes();
    FEATURED_THEME_IDS
        .iter()
        .filter_map(|id| themes.iter().find(|theme| theme.id == *id))
        .copied()
        .filter(move |theme| theme.mode == mode)
        .map(SwatchEntry::Palette)
        .chain(SYSTEM_SWATCHES.into_iter().filter(move |entry| entry.mode() == mode))
        .chain(
            themes
                .iter()
                .copied()
                .filter(move |theme| theme.mode == mode && !FEATURED_THEME_IDS.contains(&theme.id))
                .map(SwatchEntry::Palette),
        )
}

/// Picker order grouped by mode: every light swatch first, then every dark
/// swatch. The settings picker renders the two groups as separate rows.
pub(in crate::desktop) fn picker_swatches() -> impl Iterator<Item = SwatchEntry> {
    picker_swatches_for(ThemeMode::Light).chain(picker_swatches_for(ThemeMode::Dark))
}

/// Indexable snapshot of one mode's picker row. The registry is static and
/// custom themes render in their own section, so the snapshot never goes
/// stale; the virtualized rows index it by visible position.
pub(in crate::desktop) fn picker_entries_for(mode: ThemeMode) -> &'static [SwatchEntry] {
    static LIGHT: std::sync::LazyLock<Vec<SwatchEntry>> =
        std::sync::LazyLock::new(|| picker_swatches_for(ThemeMode::Light).collect());
    static DARK: std::sync::LazyLock<Vec<SwatchEntry>> =
        std::sync::LazyLock::new(|| picker_swatches_for(ThemeMode::Dark).collect());
    match mode {
        ThemeMode::Light => &LIGHT,
        ThemeMode::Dark => &DARK,
    }
}

/// Label frequencies across both picker rows, so duplicate labels keep
/// their disambiguating id prefix no matter which row is visible.
pub(in crate::desktop) fn picker_label_counts() -> &'static HashMap<&'static str, usize> {
    static COUNTS: std::sync::LazyLock<HashMap<&'static str, usize>> = std::sync::LazyLock::new(|| {
        let mut counts = HashMap::new();
        for entry in picker_swatches() {
            *counts.entry(entry.label()).or_insert(0_usize) += 1;
        }
        counts
    });
    &COUNTS
}
