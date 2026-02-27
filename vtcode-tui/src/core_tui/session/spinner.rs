use std::time::{Duration, Instant};

use crate::config::constants::ui;

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

    pub fn update(&mut self) -> bool {
        if self.is_active && self.last_update.elapsed().as_millis() >= 80 {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
            self.last_update = Instant::now();
            return true;
        }
        false
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }
}

/// Spinner animation frames (Braille pattern for smooth animation)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl Default for ThinkingSpinner {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ShimmerState {
    phase: f32,
    phase_step: f32,
    last_update: Instant,
    frame_interval: Duration,
}

impl ShimmerState {
    pub fn new() -> Self {
        let frame_interval = Duration::from_millis(ui::TUI_SHIMMER_FRAME_INTERVAL_MS);
        let sweep_duration = Duration::from_millis(ui::TUI_SHIMMER_SWEEP_DURATION_MS);
        let phase_step = if sweep_duration.is_zero() {
            0.0
        } else {
            frame_interval.as_secs_f32() / sweep_duration.as_secs_f32()
        };
        Self {
            phase: 0.0,
            phase_step,
            last_update: Instant::now(),
            frame_interval,
        }
    }

    pub fn update(&mut self) -> bool {
        if self.phase_step == 0.0 || self.last_update.elapsed() < self.frame_interval {
            return false;
        }

        self.last_update = Instant::now();
        self.phase += self.phase_step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        true
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }
}

impl Default for ShimmerState {
    fn default() -> Self {
        Self::new()
    }
}
