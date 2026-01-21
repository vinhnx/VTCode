use std::time::Instant;

/// Spinner state for showing AI thinking indicator
#[derive(Clone)]
pub(crate) struct ThinkingSpinner {
    pub(crate) is_active: bool,
    started_at: Instant,
    spinner_index: usize,
    last_update: Instant,
    #[allow(dead_code)]
    pub(crate) label: String,
}

impl ThinkingSpinner {
    pub fn new() -> Self {
        Self {
            is_active: false,
            started_at: Instant::now(),
            spinner_index: 0,
            last_update: Instant::now(),
            label: String::new(),
        }
    }

    pub fn start(&mut self) {
        self.is_active = true;
        self.started_at = Instant::now();
        self.last_update = Instant::now();
        self.spinner_index = 0;
    }

    pub fn stop(&mut self) {
        self.is_active = false;
    }

    pub fn update(&mut self) {
        if self.is_active && self.last_update.elapsed().as_millis() >= 80 {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
            self.last_update = Instant::now();
        }
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Get the current spinner frame character
    pub fn current_frame(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()]
    }
}

/// Spinner animation frames (Braille pattern for smooth animation)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Default for ThinkingSpinner {
    fn default() -> Self {
        Self::new()
    }
}
