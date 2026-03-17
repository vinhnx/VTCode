// Compatibility facade: VT Code internals still import `vtcode_core::ui::tui`.
// The implementation now lives in the standalone `vtcode-tui` crate.

pub use vtcode_tui::app::*;
pub use vtcode_tui::core::convert_style;
