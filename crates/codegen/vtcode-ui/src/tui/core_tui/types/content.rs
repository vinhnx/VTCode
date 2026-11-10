//! Re-export of the shared input content type.
//!
//! `ContentPart` lives in `vtcode_commons::ui_protocol` so headless
//! `vtcode-core` builds share one type identity with the TUI.

pub use vtcode_commons::ui_protocol::ContentPart;
