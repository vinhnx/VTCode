#![allow(deprecated)]
//! Deprecated re-export module for the Markdown storage subsystem.
//! New code should migrate to the [`markdown_ledger`] module.

#[deprecated(
    since = "0.22.0",
    note = "Use the `markdown_ledger` module instead of `markdown_storage`."
)]
pub use crate::markdown_ledger::*;
