/// Scroll region boundaries (inclusive top, inclusive bottom).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Region {
    pub(crate) top: usize,
    pub(crate) bottom: usize,
}
