//! Change-kind helper labels and the fixed row-size cache.

use super::*;

/// Reuse fixed virtual-list geometry until its row count or height changes.
#[derive(Default)]
pub(in crate::desktop::view) struct FixedRowSizeCache {
    row_height_bits: u32,
    sizes: Rc<Vec<Size<Pixels>>>,
}

impl FixedRowSizeCache {
    pub(in crate::desktop::view) fn get(&mut self, row_count: usize, row_height: f32) -> Rc<Vec<Size<Pixels>>> {
        let row_height_bits = row_height.to_bits();
        if self.sizes.len() != row_count || self.row_height_bits != row_height_bits {
            self.sizes = Rc::new(vec![size(px(0.), px(row_height)); row_count]);
            self.row_height_bits = row_height_bits;
        }
        Rc::clone(&self.sizes)
    }
}

/// Stable per-kind slug for change-filter element ids.
pub(in crate::desktop::view) fn change_kind_slug(kind: DiffFileKind) -> &'static str {
    match kind {
        DiffFileKind::Add => "added",
        DiffFileKind::Delete => "deleted",
        DiffFileKind::Update => "modified",
    }
}

/// Localized name for a change kind, shared by rows and the kind filter.
pub(in crate::desktop::view) fn change_kind_name(kind: DiffFileKind) -> String {
    match kind {
        DiffFileKind::Add => rust_i18n::t!("app.inspector.kind_added").to_string(),
        DiffFileKind::Delete => rust_i18n::t!("app.inspector.kind_deleted").to_string(),
        DiffFileKind::Update => rust_i18n::t!("app.inspector.kind_modified").to_string(),
    }
}

/// One-letter badge and tone for a change kind, shared by rows and the filter.
pub(in crate::desktop::view) fn change_kind_badge(
    kind: DiffFileKind,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> (&'static str, gpui_kit::Hsla) {
    match kind {
        DiffFileKind::Add => ("A", colors.success),
        DiffFileKind::Delete => ("D", colors.danger),
        DiffFileKind::Update => ("M", colors.muted_foreground),
    }
}
