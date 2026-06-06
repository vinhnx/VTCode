/// Cursor position on the terminal grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub col: usize,
    pub row: usize,
}
