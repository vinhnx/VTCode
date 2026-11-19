//! Theme-picker swatch catalog extracted from the theme registry.

use super::*;
use std::borrow::Cow;
use std::collections::HashSet;

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

/// One selectable entry in the virtualized theme grid. Built-in swatches are
/// copied from the static catalog; saved custom themes own their name and
/// colors, so this entry is cloned into the virtual list's `'static` item
/// closure instead of being borrowed from the registry.
#[derive(Clone)]
pub(in crate::desktop) enum ThemeGridEntry {
    BuiltIn(SwatchEntry),
    Custom(CustomTheme),
}

impl ThemeGridEntry {
    pub(in crate::desktop) fn id(&self) -> String {
        match self {
            Self::BuiltIn(entry) => entry.id(),
            Self::Custom(theme) => theme.id.clone(),
        }
    }

    pub(in crate::desktop) fn label(&self) -> Cow<'static, str> {
        match self {
            Self::BuiltIn(entry) => Cow::Borrowed(entry.label()),
            Self::Custom(theme) => Cow::Owned(theme.name.clone()),
        }
    }

    /// The two colors a picker swatch previews.
    pub(in crate::desktop) fn preview(&self) -> (Hsla, Hsla) {
        match self {
            Self::BuiltIn(entry) => entry.preview(),
            Self::Custom(theme) => (theme.background, theme.foreground),
        }
    }

    pub(in crate::desktop) fn accent(&self) -> Option<Hsla> {
        match self {
            Self::BuiltIn(entry) => entry.accent(),
            // Same re-seated accent the palette applies, so the preview never
            // shows a chip that would fail contrast once selected.
            Self::Custom(theme) => Some(theme.seeds().accent),
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

/// Indexable snapshot of one mode's built-in picker row. The registry is
/// static, so the virtualized rows can index it by visible position. Saved
/// custom themes are appended by [`grid_entries_for`] from preferences.
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

/// One mode's grid rows: the static built-in catalog followed by saved custom
/// themes of that mode. Customs load from `preferences.json` on launch, so a
/// saved theme rejoins the grid on every relaunch without registry changes.
pub(in crate::desktop) fn grid_entries_for(mode: ThemeMode, customs: &[CustomTheme]) -> Vec<ThemeGridEntry> {
    picker_entries_for(mode)
        .iter()
        .copied()
        .map(ThemeGridEntry::BuiltIn)
        .chain(
            customs
                .iter()
                .filter(move |theme| theme.mode == mode)
                .cloned()
                .map(ThemeGridEntry::Custom),
        )
        .collect()
}

/// Labels appearing more than once across both grid rows. Custom names are
/// unique among themselves but may shadow a built-in label, so both sources
/// feed the count and duplicates keep their disambiguating id prefix.
pub(in crate::desktop) fn grid_duplicate_labels(customs: &[CustomTheme]) -> HashSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in picker_swatches() {
        *counts.entry(entry.label().to_string()).or_insert(0) += 1;
    }
    for theme in customs {
        *counts.entry(theme.name.clone()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter_map(|(label, count)| (count > 1).then_some(label))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    fn custom(id: &str, name: &str, mode: ThemeMode) -> CustomTheme {
        CustomTheme {
            id: id.to_string(),
            name: name.to_string(),
            mode,
            background: rgb(0x101010).into(),
            foreground: rgb(0xf0f0f0).into(),
            accent: rgb(0x7aa2f7).into(),
            secondary: rgb(0xbb9af7).into(),
            success: rgb(0x63c779).into(),
            danger: rgb(0xf7768e).into(),
            warning: rgb(0xe0af68).into(),
        }
    }

    #[test]
    fn grid_entries_include_saved_customs_in_their_mode() {
        let customs = [
            custom("custom-night", "Night", ThemeMode::Dark),
            custom("custom-paper", "Paper", ThemeMode::Light),
        ];
        for (mode, expected_id, expected_label) in [
            (ThemeMode::Dark, "custom-night", "Night"),
            (ThemeMode::Light, "custom-paper", "Paper"),
        ] {
            let entries = grid_entries_for(mode, &customs);
            assert_eq!(entries.len(), picker_entries_for(mode).len() + 1);
            let last = entries.last().expect("custom theme is appended");
            assert_eq!(last.id(), expected_id);
            assert_eq!(last.label().as_ref(), expected_label);
            assert!(matches!(last, ThemeGridEntry::Custom(_)));
            assert!(
                entries
                    .iter()
                    .take(entries.len() - 1)
                    .all(|entry| matches!(entry, ThemeGridEntry::BuiltIn(_)))
            );
        }
    }

    #[test]
    fn custom_names_shadowing_builtin_labels_are_disambiguated() {
        let label = picker_swatches().next().expect("catalog is not empty").label().to_string();
        let customs = [custom("custom-shadow", &label, ThemeMode::Dark)];
        assert!(grid_duplicate_labels(&customs).contains(&label));
    }
}
