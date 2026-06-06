//! Panel widget for rendering bordered containers.
//!
//! Re-exports `Panel` and `PanelStyleProvider` from `vtcode_design::panel`.
//! Provides a convenience constructor that uses terminal-aware border types.

use ratatui::style::{Modifier, Style};

pub use vtcode_design::panel::{Panel, PanelStyleProvider, PanelStyles};

use crate::ui::tui::session::{styling::SessionStyles, terminal_capabilities};

/// Implement `PanelStyleProvider` for `SessionStyles` so that `Panel` can
/// render with the active theme's styles.
impl PanelStyleProvider for SessionStyles {
    fn default_style(&self) -> Style {
        SessionStyles::default_style(self)
    }

    fn accent_style(&self) -> Style {
        SessionStyles::accent_style(self)
    }

    fn border_style(&self) -> Style {
        SessionStyles::border_style(self)
    }
}

/// Implement `PanelStyles` for `SessionStyles`.
impl PanelStyles for SessionStyles {
    fn muted_style(&self) -> Style {
        self.default_style().add_modifier(Modifier::DIM)
    }

    fn title_style(&self) -> Style {
        self.accent_style().add_modifier(Modifier::BOLD)
    }

    fn border_active_style(&self) -> Style {
        self.border_style()
            .remove_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    }

    fn divider_style(&self) -> Style {
        self.border_style().add_modifier(Modifier::DIM)
    }
}

/// Create a new `Panel` with terminal-aware border type as default.
///
/// This preserves the previous behavior where `Panel::new` automatically
/// selected the border type based on terminal capabilities.
pub fn new_panel<'a>(styles: &'a SessionStyles) -> Panel<'a, SessionStyles> {
    Panel::new(styles).border_type(terminal_capabilities::get_border_type())
}
