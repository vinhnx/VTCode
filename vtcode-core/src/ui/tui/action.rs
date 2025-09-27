#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAction {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Redraw,
    Submit(String),
    Cancel,
    Exit,
    Interrupt,
    Scroll(ScrollAction),
}
