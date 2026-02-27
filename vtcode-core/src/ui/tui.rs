// Compatibility shim: TUI implementation source now lives in `vtcode-tui`.
// Keep the original module path (`vtcode_core::ui::tui`) intact by compiling
// the migrated source tree inside vtcode-core.

#[path = "../../../vtcode-tui/src/core_tui/mod.rs"]
mod migrated;

pub use migrated::*;
