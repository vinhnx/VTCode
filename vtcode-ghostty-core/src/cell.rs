use crate::style::Style;

/// A single terminal grid cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub(crate) ch: char,
    pub(crate) style: Style,
    pub(crate) wide_continuation: bool,
}

impl Cell {
    /// Create a blank cell with the given style.
    pub(crate) fn blank(style: Style) -> Self {
        Self {
            ch: ' ',
            style,
            wide_continuation: false,
        }
    }

    /// Create a printable cell.
    pub(crate) fn printable(ch: char, style: Style) -> Self {
        Self {
            ch,
            style,
            wide_continuation: false,
        }
    }

    /// Create a wide-character continuation cell (the right-hand half).
    pub(crate) fn wide_continuation(style: Style) -> Self {
        Self {
            ch: ' ',
            style,
            wide_continuation: true,
        }
    }

    /// The character in this cell.
    pub fn ch(&self) -> char {
        self.ch
    }

    /// The style of this cell.
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// Whether this cell is the right-hand half of a wide character.
    pub fn is_wide_continuation(&self) -> bool {
        self.wide_continuation
    }

    /// Whether this cell is blank (space, not a wide continuation).
    pub fn is_blank(&self) -> bool {
        self.ch == ' ' && !self.wide_continuation
    }
}
