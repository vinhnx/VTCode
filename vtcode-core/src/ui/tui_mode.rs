use std::sync::atomic::{AtomicBool, Ordering};

static TUI_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_tui_mode(active: bool) {
    TUI_MODE_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn is_tui_mode() -> bool {
    TUI_MODE_ACTIVE.load(Ordering::SeqCst)
}
