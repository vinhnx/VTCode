use super::*;

impl Session {
    pub(crate) fn default_style(&self) -> InlineTextStyle {
        self.styles.default_inline_style()
    }
}
