//! Canonical list-row builders for TUI modals and command menus.
//!
//! Call sites should not assemble `InlineListItem { .. }` literals directly.
//! These helpers encode the shared visual contract: group headers, title ·
//! value · description rows, toned actions, and current/choice rows.
//!
//! Wire types stay in `vtcode_commons::ui_protocol` (protocol layer); this
//! module is the design-system factory that returns them.

pub use vtcode_commons::ui_protocol::{InlineItemKind, InlineListItem, InlineListSelection, InlineStatus, InlineTone};

/// Section header: bold title with blank spacing above and below.
/// Renders as `» Title` (leading anchor) to diff it from items.
#[must_use]
pub fn group_header(title: impl Into<String>) -> InlineListItem {
    InlineListItem::group_header(title)
}

/// Full-width rule between option groups.
#[must_use]
pub fn group_divider() -> InlineListItem {
    InlineListItem::group_divider()
}

/// Non-selectable dimmed note (never a group header).
#[must_use]
pub fn hint(text: impl Into<String>) -> InlineListItem {
    InlineListItem {
        title: text.into(),
        kind: InlineItemKind::Hint,
        ..InlineListItem::default()
    }
}

/// Title + optional accent value + dimmed description.
///
/// Used for settings keys and model capability rows (`value` is the live
/// value or capability summary; `description` is help text).
#[must_use]
pub fn setting(
    title: impl Into<String>,
    value: Option<String>,
    description: Option<String>,
    selection: Option<InlineListSelection>,
) -> InlineListItem {
    InlineListItem {
        title: title.into(),
        value,
        subtitle: description,
        selection,
        kind: InlineItemKind::Setting,
        badge_tone: InlineTone::Accent,
        ..InlineListItem::default()
    }
}

/// Imperative row (Back, Reset, Refresh, Pick model, …) with an explicit tone.
#[must_use]
pub fn action(
    title: impl Into<String>,
    description: impl Into<String>,
    badge: Option<String>,
    tone: InlineTone,
    selection: Option<InlineListSelection>,
) -> InlineListItem {
    let title = title.into();
    let description = description.into();
    let search_value = format!("{title} {description}").to_ascii_lowercase();
    InlineListItem {
        subtitle: (!description.trim().is_empty()).then_some(description),
        title,
        badge,
        selection,
        kind: InlineItemKind::Action,
        badge_tone: tone,
        search_value: Some(search_value),
        ..InlineListItem::default()
    }
}

/// Selectable choice in a list (reasoning level, service tier, list entry).
#[must_use]
pub fn choice(
    title: impl Into<String>,
    description: Option<String>,
    selection: Option<InlineListSelection>,
) -> InlineListItem {
    let title = title.into();
    let search_value = format!("{title} {}", description.clone().unwrap_or_default()).to_ascii_lowercase();
    InlineListItem {
        subtitle: description,
        title,
        selection,
        kind: InlineItemKind::Item,
        search_value: Some(search_value),
        ..InlineListItem::default()
    }
}

/// Choice row that is live/current (kept setting, active model).
#[must_use]
pub fn current_choice(
    title: impl Into<String>,
    description: Option<String>,
    selection: Option<InlineListSelection>,
) -> InlineListItem {
    choice(title, description, selection).with_badge("Current", InlineTone::Current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_header_is_non_selectable_header() {
        let row = group_header("Anthropic");
        assert!(row.selection.is_none());
        assert_eq!(row.kind, InlineItemKind::Header);
        assert!(row.is_header());
    }

    #[test]
    fn setting_carries_value_and_accent_tone() {
        let row = setting("Theme", Some("mono".into()), Some("ANSI theme".into()), None);
        assert_eq!(row.value.as_deref(), Some("mono"));
        assert_eq!(row.subtitle.as_deref(), Some("ANSI theme"));
        assert_eq!(row.kind, InlineItemKind::Setting);
        assert_eq!(row.badge_tone, InlineTone::Accent);
    }

    #[test]
    fn action_uses_explicit_tone_and_skips_empty_description() {
        let row = action("Reset", "", Some("Destructive".to_string()), InlineTone::Danger, None);
        assert_eq!(row.kind, InlineItemKind::Action);
        assert_eq!(row.badge_tone, InlineTone::Danger);
        assert!(row.subtitle.is_none());
    }

    #[test]
    fn current_choice_marks_current_badge() {
        let row = current_choice("Keep current (high)", Some("desc".into()), None);
        assert_eq!(row.badge.as_deref(), Some("Current"));
        assert_eq!(row.badge_tone, InlineTone::Current);
    }

    #[test]
    fn choice_seeds_search_from_title_and_description() {
        let row = choice("Flex", Some("lower cost".to_string()), None);
        assert_eq!(row.search_value.as_deref(), Some("flex lower cost"));
    }

    #[test]
    fn hint_is_not_a_header() {
        let row = hint("Press Esc to reset");
        assert_eq!(row.kind, InlineItemKind::Hint);
        assert!(!row.is_header());
    }
}
