//! Desktop layout tokens: one source of truth for transcript row geometry.
//!
//! Lives beside the color and radius tokens in `themes/` so every desktop
//! design-system value has one home, while staying native to GPUI (`Rems`).
//!
//! Tool rows, activity groups, notices, and assistant prose all sit on the
//! same 32px title spine. That number used to be recomputed in three places
//! (an assistant inset constant, the shared icon slot, and the notice row), so
//! each file carried its own copy of the same arithmetic. The tokens here name
//! the relationship instead, and are `Rems` so they zoom with the base font.
//!
//! Corner radius is deliberately absent: it is a theme token set once in
//! [`super::apply_desktop_radii`] and read via
//! `cx.theme().radius` / `cx.theme().radius_lg`. Margins are not used between
//! transcript rows — the scroller owns row spacing and padding owns insets —
//! so there is no margin token to scatter. Font weights come from the Kit
//! helpers (`font_semibold` for a title, plain for a body); only the size steps
//! need naming, because the same two sizes are restated across every row.

use gpui_kit::{Rems, rems};

/// Gap and padding are the two spacing roles transcript rows use.
pub(in crate::desktop) mod gap {
    use super::*;

    /// 8px: between the leading slot and the title, and before a trailing
    /// action. Matches the `gap_2` tier used by every disclosure row.
    pub(in crate::desktop) const TITLE: Rems = rems(0.5);

    /// 4px: within one control (action label to chevron) and between a title
    /// and its details row. Matches the `gap_1` tier used by tool rows.
    pub(in crate::desktop) const COMPACT: Rems = rems(0.25);
}

pub(in crate::desktop) mod padding {
    use super::*;

    /// 8px: the small-button inset that starts every title lane. The notice
    /// row is not a button, so it restates this inset explicitly.
    pub(in crate::desktop) const ROW_INSET: Rems = rems(0.5);

    /// 24px: completes `ROW_INSET + ICON_SLOT + gap::TITLE` so a body under a
    /// title starts on the same spine as the title itself.
    pub(in crate::desktop) const BODY_INSET: Rems = rems(1.5);

    /// 32px: the shared title spine, `ROW_INSET + ICON_SLOT + gap::TITLE`.
    /// Assistant prose uses this as its horizontal inset; the notice row uses
    /// the three component tokens so the relationship stays explicit.
    pub(in crate::desktop) const TITLE_SPINE: Rems = rems(2.);
}

/// Fixed 16px lane that holds a tool indicator, a chevron, or a notice marker.
/// Kept beside the spacing tokens because every title spine is measured from
/// it and an accidental tier change would move all rows at once.
pub(in crate::desktop) const ICON_SLOT: Rems = rems(1.);

/// Type sizes shared by transcript rows. Weights stay as the Kit helpers
/// (`font_semibold` for titles, none for bodies) because they are already
/// named; only the repeated size literals are centralized here.
pub(in crate::desktop) mod typography {
    use super::*;

    /// 14px: disclosure titles, status labels, and row bodies (`text_sm`).
    pub(in crate::desktop) const ROW: Rems = rems(0.875);

    /// 12px: quiet trailing action labels (`text_xs`).
    pub(in crate::desktop) const ACTION: Rems = rems(0.75);
}

// The title spine is a derived value, not an independent constant. Pin both
// relationships so changing one tier fails the build instead of silently
// moving notice, tool, and assistant rows off the same line. The chosen rem
// values are exactly representable, so equality is safe here.
const _: () = assert!(padding::ROW_INSET.0 + ICON_SLOT.0 + gap::TITLE.0 == padding::TITLE_SPINE.0);
const _: () = assert!(padding::ROW_INSET.0 + padding::BODY_INSET.0 == padding::TITLE_SPINE.0);
