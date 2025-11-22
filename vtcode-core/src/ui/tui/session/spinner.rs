use std::time::Instant;

/// Spinner state for showing AI thinking indicator
#[derive(Clone)]
pub struct ThinkingSpinner {
    is_active: bool,
    started_at: Instant,
    spinner_index: usize,
    last_update: Instant,
    spinner_line_index: Option<usize>, // Track which message line is the spinner
}

impl ThinkingSpinner {
    pub fn new() -> Self {
        Self {
            is_active: false,
            started_at: Instant::now(),
            spinner_index: 0,
            last_update: Instant::now(),
            spinner_line_index: None,
        }
    }

    pub fn start(&mut self, line_index: usize) {
        self.is_active = true;
        self.spinner_line_index = Some(line_index);
        self.started_at = Instant::now();
        self.last_update = Instant::now();
        self.spinner_index = 0;
    }

    pub fn stop(&mut self) {
        self.is_active = false;
        self.spinner_line_index = None;
    }

    pub fn update(&mut self) {
        if self.is_active && self.last_update.elapsed().as_millis() >= 80 {
            self.spinner_index = (self.spinner_index + 1) % 4;
            self.last_update = Instant::now();
        }
    }

    pub fn current_frame(&self) -> &'static str {
        match self.spinner_index {
            0 => "⠋",
            1 => "⠙",
            2 => "⠹",
            3 => "⠸",
            _ => "⠋",
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn spinner_line_index(&self) -> Option<usize> {
        self.spinner_line_index
    }
}

impl Default for ThinkingSpinner {
    fn default() -> Self {
        Self::new()
    }
}
