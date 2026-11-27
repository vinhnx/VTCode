//! Memoized visible-row projection for a terminal tab.
//!
//! `DesktopView` repaints the whole window on any notify (backend events,
//! composer typing, panel resize). Rebuilding the grid on every one of those
//! frames would resolve rows x columns worth of styles and strings for an
//! unchanged screen, so the projection is cached behind a `RefCell` and keyed
//! on the grid mutation revision plus the palette revision.

use std::cell::Ref;

use super::*;
use crate::desktop::terminal_screen::TerminalRowRun;

pub(super) struct RowCache {
    /// `None` until the first projection, so a fresh grid can never match.
    revision: Option<u64>,
    palette_revision: u64,
    rows: Vec<Vec<TerminalRowRun>>,
}

impl RowCache {
    pub(super) fn new() -> Self {
        Self {
            revision: None,
            palette_revision: 0,
            rows: Vec::new(),
        }
    }
}

impl TerminalTab {
    /// Visible rows for the current revision. The returned guard keeps the
    /// cached rows alive for the duration of one render pass.
    pub(in crate::desktop) fn display_rows(&self) -> Ref<'_, Vec<Vec<TerminalRowRun>>> {
        let revision = self.screen.revision();
        let palette_revision = self.palette_revision;
        {
            let cached = self.display_cache.borrow();
            if cached.revision == Some(revision) && cached.palette_revision == palette_revision {
                return Ref::map(cached, |cache| &cache.rows);
            }
        }
        let rows = self.screen.display_rows(&self.palette);
        {
            let mut cache = self.display_cache.borrow_mut();
            cache.revision = Some(revision);
            cache.palette_revision = palette_revision;
            cache.rows = rows;
        }
        Ref::map(self.display_cache.borrow(), |cache| &cache.rows)
    }

    /// Whether the cached projection matches the current grid/palette state.
    /// Test-only: guards the memoization contract without counting renders.
    #[cfg(test)]
    pub(in crate::desktop) fn display_rows_are_cached(&self) -> bool {
        let cache = self.display_cache.borrow();
        cache.revision == Some(self.screen.revision()) && cache.palette_revision == self.palette_revision
    }
}
