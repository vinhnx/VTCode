use super::*;

impl Session {
    pub fn default_style(&self) -> InlineTextStyle {
        self.styles.default_inline_style()
    }
}
