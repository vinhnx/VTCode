//! Thin alias for the canonical TUI design-system row builders.
//!
//! Command and modal code should construct list rows through
//! [`vtcode_ui::design::list`] rather than raw `InlineListItem` literals.
#![allow(unused_imports, reason = "facade re-exports for call-site convenience")]

pub(crate) use vtcode_commons::ui_protocol::InlineTone as Tone;
pub(crate) use vtcode_ui::design::keys;
pub(crate) use vtcode_ui::design::list::{action, choice, current_choice, group_divider, group_header, hint, setting};
