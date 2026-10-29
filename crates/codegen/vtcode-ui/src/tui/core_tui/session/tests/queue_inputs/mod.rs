#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use super::super::*;
pub(super) use super::helpers::*;
pub(super) use crate::tui::core_tui::app::types::InlineEvent as AppInlineEvent;
pub(super) use crate::tui::core_tui::types::{ContentPart, SubmittedInput};
pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::AtomicUsize;

fn queue_edit_hint() -> String {
    if cfg!(target_os = "macos") {
        "\u{2325} + \u{2191} edit".to_string()
    } else {
        "Alt + \u{2191} edit".to_string()
    }
}

fn tmux_queue_edit_hint() -> String {
    if cfg!(target_os = "macos") {
        "\u{21E7} + \u{2190} edit".to_string()
    } else {
        "Shift + \u{2190} edit".to_string()
    }
}

mod app_editing;
mod busy_queue;
mod paste;
mod queue_edit;
mod submit;
mod suggest;
