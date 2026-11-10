use std::time::{Duration, Instant};

use ratatui_cheese::spinner::SpinnerType;

use crate::tui::config::constants::ui;

/// Spinner state for showing AI thinking indicator
#[derive(Clone)]
pub(crate) struct ThinkingSpinner {
    pub(crate) is_active: bool,
    started_at: Instant,
    spinner_index: usize,
    last_update: Instant,
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    label: String,
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
        if self.is_active
            && self.last_update.elapsed().as_millis() >= u128::from(ui::TUI_THINKING_SPINNER_FRAME_INTERVAL_MS)
        {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
            self.last_update = Instant::now();
            return true;
        }
        false
    }

    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn current_frame(&self) -> &'static str {
        SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()]
    }
}

/// Pulse spinner frame for a normalized animation phase.
///
/// Uses `ratatui-cheese`'s `SpinnerType::Pulse` frames so file edit/write/update
/// status indicators share the same pulse animation as the rest of the toolkit.
pub(crate) fn pulse_spinner_frame_for_phase(phase: f32) -> &'static str {
    let frames = SpinnerType::Pulse.frames();
    if frames.is_empty() {
        return "";
    }

    let clamped = phase.clamp(0.0, 0.999_999);
    let index = (clamped * frames.len() as f32) as usize;
    frames[index.min(frames.len() - 1)]
}

/// Terminal title spinner frame derived from the shared shimmer phase so the
/// title and the footer's global loading shimmer stay in lockstep and can
/// never drift apart. Two Braille rotations per 2s shimmer sweep keeps a
/// readable ~100ms cadence (~10 title writes/s worst case).
pub(crate) fn title_spinner_frame(shimmer_phase: f32) -> &'static str {
    const TITLE_TURNS_PER_SWEEP: f32 = 2.0;
    spinner_frame_for_phase((shimmer_phase * TITLE_TURNS_PER_SWEEP).fract())
}

/// Braille spinner frame for a normalized 0..1 phase.
fn spinner_frame_for_phase(phase: f32) -> &'static str {
    let frames = SPINNER_FRAMES.len();
    let scaled = (phase.fract().clamp(0.0, 0.999_999) * frames as f32) as usize;
    SPINNER_FRAMES[scaled.min(frames - 1)]
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
    /// Status text the sweep currently decorates; a change pauses the sweep
    /// briefly so redraws do not jump the highlight across new text.
    held_text: String,
    hold_until: Option<Instant>,
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
            held_text: String::new(),
            hold_until: None,
        }
    }

    /// Hold the sweep steady while `text` redraws: a changed status text
    /// pauses advancement for one short settle window instead of letting the
    /// highlight jump across the new text on the next frame.
    pub fn hold_on_text_change(&mut self, text: &str) {
        if self.held_text != text {
            self.held_text = text.to_string();
            self.hold_until = Some(Instant::now() + Duration::from_millis(200));
        }
    }

    pub fn update(&mut self) -> bool {
        if self.phase_step == 0.0
            || self.hold_until.is_some_and(|until| Instant::now() < until)
            || self.last_update.elapsed() < self.frame_interval
        {
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

#[cfg(test)]
mod tests {
    use super::ShimmerState;

    #[test]
    fn sweep_holds_briefly_on_status_text_change() {
        let mut state = ShimmerState::new();
        // Seed the first text with its initial hold already expired: `new`
        // starts at `now`, so sleep past the frame interval only.
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(state.update(), "fresh state advances once the frame lands");
        let advanced = state.phase();
        assert!(advanced > 0.0, "sweep moved, got {advanced}");

        // A redrawn status text freezes the phase for one settle window.
        state.hold_on_text_change("Running Read main.rs");
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(!state.update(), "changed text must hold the sweep");
        assert_eq!(state.phase(), advanced);

        // The same text again does not re-arm the hold: after the window the
        // sweep resumes.
        std::thread::sleep(std::time::Duration::from_millis(220));
        state.hold_on_text_change("Running Read main.rs");
        assert!(state.update(), "sweep resumes once the settle window passes");
    }
}
