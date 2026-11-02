use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::{NavigationDirection, TouchPhase};

const MAX_NAVIGATION_ENTRIES: usize = 100;

/// Horizontal scroll distance (device px) that navigates history, for
/// trackpads whose system "swipe between pages" gesture is off: those
/// trackpads report sideways movement as plain scroll deltas instead of the
/// discrete swipe events the `Navigate` mouse-button handlers already cover.
/// Sized so a deliberate sideways swipe fires once while ordinary diagonal
/// drift during vertical scrolling stays below it; vertical-dominant events
/// reset the accumulation outright (see [`SwipeScrollAccumulator`]).
pub(super) const SWIPE_SCROLL_THRESHOLD_PX: f32 = 120.0;

/// Fraction of [`SWIPE_SCROLL_THRESHOLD_PX`] past which the matching toolbar
/// arrow lights up as a "keep swiping to go back/forward" cue.
const SWIPE_SCROLL_ARMED_FRACTION: f32 = 0.25;

/// Quiet period after a scroll-triggered navigation during which further
/// scrolls keep accumulating (so the cue stays live) but cannot fire again.
/// Covers trackpad momentum that outlives the gesture's `Ended` phase.
const SWIPE_SCROLL_COOLDOWN: Duration = Duration::from_millis(800);

/// Whether macOS "Scroll direction: Natural" is on. A missing key means the
/// user never changed the setting, and the system default is natural. Scroll
/// deltas flip with this setting (finger direction), so the back/forward
/// mapping flips with it; the discrete swipe mapping in the backend stays put.
#[cfg(all(target_os = "macos", not(test)))]
pub(super) fn natural_scroll_enabled() -> bool {
    use objc2_foundation::{NSUserDefaults, ns_string};

    let defaults = NSUserDefaults::standardUserDefaults();
    // Static key: this runs on every workspace scroll event, so it must not
    // allocate. `boolForKey` reports false for a missing key, which would
    // invert the mapping for every user on the system default, so check
    // presence first.
    let key = ns_string!("com.apple.swipescrolldirection");
    if defaults.objectForKey(key).is_none() {
        return true;
    }
    defaults.boolForKey(key)
}

/// Headless/non-mac fallback: tests pin the natural mapping and inject the
/// flipped mapping directly into [`SwipeScrollAccumulator::push`].
#[cfg(any(not(target_os = "macos"), test))]
pub(super) fn natural_scroll_enabled() -> bool {
    true
}

/// Gesture state machine turning horizontal scrolls into history navigation.
///
/// Contract:
/// - `push` is total: non-finite deltas reset and return `None`; it never
///   panics (cooldown uses a saturating duration).
/// - Accumulation is single-signed: a direction reversal starts a fresh
///   gesture, so wiggling back and forth cannot ratchet to the threshold.
/// - Vertical-dominant input (`|dy| > 2|dx|`) resets without firing; it
///   belongs to the scrollable under the cursor.
/// - `Started` begins a fresh gesture; `Ended`/`Cancelled` reset without
///   firing, unwinding any in-progress interaction.
/// - At most one fire per [`SWIPE_SCROLL_COOLDOWN`] window; firing zeroes the
///   accumulation so a sustained scroll must travel the full threshold again.
#[derive(Debug, Default)]
pub(super) struct SwipeScrollAccumulator {
    accumulated_x_px: f32,
    natural_scroll: bool,
    last_fire: Option<Instant>,
}

impl SwipeScrollAccumulator {
    pub(super) fn push(
        &mut self,
        dx_px: f32,
        dy_px: f32,
        phase: TouchPhase,
        now: Instant,
        natural_scroll: bool,
    ) -> Option<NavigationDirection> {
        self.natural_scroll = natural_scroll;
        if !dx_px.is_finite() || !dy_px.is_finite() {
            self.reset();
            return None;
        }
        match phase {
            TouchPhase::Started => self.reset(),
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.reset();
                return None;
            }
            TouchPhase::Moved => {}
        }
        if dy_px.abs() > dx_px.abs() * 2.0 {
            self.reset();
            return None;
        }
        if self.accumulated_x_px != 0.0 && dx_px != 0.0 && (self.accumulated_x_px > 0.0) != (dx_px > 0.0) {
            self.reset();
        }
        self.accumulated_x_px += dx_px;
        if self.accumulated_x_px.abs() < SWIPE_SCROLL_THRESHOLD_PX {
            return None;
        }
        if self
            .last_fire
            .is_some_and(|fired| now.saturating_duration_since(fired) < SWIPE_SCROLL_COOLDOWN)
        {
            return None;
        }
        let direction = swipe_direction_for_dx(self.accumulated_x_px, self.natural_scroll);
        self.last_fire = Some(now);
        self.accumulated_x_px = 0.0;
        direction
    }

    /// Direction the in-progress gesture leans toward, driving the toolbar
    /// arrow cue. `None` below the armed fraction, after a reset, or when no
    /// gesture is in flight.
    pub(super) fn armed(&self) -> Option<NavigationDirection> {
        if self.accumulated_x_px.abs() < SWIPE_SCROLL_THRESHOLD_PX * SWIPE_SCROLL_ARMED_FRACTION {
            return None;
        }
        swipe_direction_for_dx(self.accumulated_x_px, self.natural_scroll)
    }

    /// Drops the in-progress gesture. The fire cooldown survives: momentum
    /// after `Ended` must not immediately re-fire.
    pub(super) fn reset(&mut self) {
        self.accumulated_x_px = 0.0;
    }
}

/// Maps a horizontal scroll offset to a history direction. Under natural
/// scrolling a positive offset means fingers moved right, which is Back —
/// mirroring the backend's discrete swipe mapping (`deltaX > 0 → Back` in
/// `gpui-pre-macos` `events.rs`); with natural scrolling off the deltas flip,
/// so the mapping flips with them. Returns `None` for a zero offset.
fn swipe_direction_for_dx(dx_px: f32, natural_scroll: bool) -> Option<NavigationDirection> {
    let back = if natural_scroll { dx_px > 0.0 } else { dx_px < 0.0 };
    let forward = if natural_scroll { dx_px < 0.0 } else { dx_px > 0.0 };
    if back {
        Some(NavigationDirection::Back)
    } else if forward {
        Some(NavigationDirection::Forward)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NavigationLocation {
    pub(super) workspace: PathBuf,
    pub(super) session_id: Option<String>,
}

#[derive(Default)]
pub(super) struct NavigationHistory {
    entries: Vec<NavigationLocation>,
    current_index: Option<usize>,
}

impl NavigationHistory {
    pub(super) fn record(&mut self, location: NavigationLocation) {
        if self.current_index.and_then(|index| self.entries.get(index)) == Some(&location) {
            return;
        }

        if let Some(index) = self.current_index {
            self.entries.truncate(index + 1);
        }
        self.entries.push(location);
        self.current_index = Some(self.entries.len() - 1);

        if self.entries.len() > MAX_NAVIGATION_ENTRIES {
            let _evicted = self.entries.remove(0);
            self.current_index = self.current_index.map(|index| index.saturating_sub(1));
        }
    }

    pub(super) fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub(super) fn back_index(&self) -> Option<usize> {
        self.current_index?.checked_sub(1)
    }

    pub(super) fn forward_index(&self) -> Option<usize> {
        let next_index = self.current_index?.checked_add(1)?;
        (next_index < self.entries.len()).then_some(next_index)
    }

    pub(super) fn location(&self, index: usize) -> Option<&NavigationLocation> {
        self.entries.get(index)
    }

    pub(super) fn navigate_to(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.current_index = Some(index);
        true
    }

    pub(super) fn replace_and_navigate_to(&mut self, index: usize, location: NavigationLocation) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        *entry = location;
        self.current_index = Some(index);
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct PendingNavigation {
    pub(super) target_index: usize,
    pub(super) workspace_switch: bool,
}

#[cfg(test)]
mod tests {
    use super::{NavigationHistory, NavigationLocation, SwipeScrollAccumulator};
    use gpui_kit::{NavigationDirection, TouchPhase};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn location(workspace: &str, session_id: Option<&str>) -> NavigationLocation {
        NavigationLocation {
            workspace: PathBuf::from(workspace),
            session_id: session_id.map(str::to_owned),
        }
    }

    fn scroll(accumulator: &mut SwipeScrollAccumulator, dx_px: f32, now: Instant) -> Option<NavigationDirection> {
        accumulator.push(dx_px, 0.0, TouchPhase::Moved, now, true)
    }

    #[test]
    fn records_locations_and_truncates_forward_entries_after_new_navigation() {
        let mut history = NavigationHistory::default();
        history.record(location("/repo", Some("one")));
        history.record(location("/repo", Some("two")));
        history.record(location("/other", None));

        assert!(history.navigate_to(0));
        history.record(location("/repo", Some("three")));

        assert_eq!(history.back_index(), Some(0));
        assert_eq!(history.forward_index(), None);
        assert_eq!(history.location(1), Some(&location("/repo", Some("three"))));
    }

    #[test]
    fn bounds_history_and_rejects_invalid_cursor_targets() {
        let mut history = NavigationHistory::default();
        for index in 0..=100 {
            history.record(location("/repo", Some(&index.to_string())));
        }

        assert_eq!(history.location(0), Some(&location("/repo", Some("1"))));
        assert!(!history.navigate_to(100));
        assert_eq!(history.forward_index(), None);
    }

    #[test]
    fn sustained_horizontal_scroll_fires_once_per_direction() {
        let now = Instant::now();
        let mut back = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut back, 60.0, now), None);
        assert_eq!(scroll(&mut back, 60.0, now), Some(NavigationDirection::Back));

        let mut forward = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut forward, -60.0, now), None);
        assert_eq!(scroll(&mut forward, -60.0, now), Some(NavigationDirection::Forward));
    }

    #[test]
    fn non_natural_scrolling_flips_the_mapping() {
        let now = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(accumulator.push(60.0, 0.0, TouchPhase::Moved, now, false), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Forward));
        assert_eq!(accumulator.push(60.0, 0.0, TouchPhase::Moved, now, false), Some(NavigationDirection::Forward));
        assert_eq!(accumulator.armed(), None);
    }

    #[test]
    fn vertical_dominant_input_resets_instead_of_firing() {
        let now = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        assert_eq!(accumulator.push(10.0, 30.0, TouchPhase::Moved, now, true), None);
        // Without the reset this second push would cross the threshold and
        // fire; instead exactly 100px remain, which arms but does not fire.
        assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Back));
    }

    #[test]
    fn direction_reversal_restarts_the_gesture() {
        let now = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        assert_eq!(scroll(&mut accumulator, -30.0, now), None);
        // A net sum would sit at -130 and fire; a restart sits at -30 + -100.
        assert_eq!(scroll(&mut accumulator, -100.0, now), Some(NavigationDirection::Forward));
    }

    #[test]
    fn terminal_phases_reset_without_firing() {
        let now = Instant::now();
        for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
            let mut accumulator = SwipeScrollAccumulator::default();
            assert_eq!(scroll(&mut accumulator, 100.0, now), None);
            assert_eq!(accumulator.push(100.0, 0.0, phase, now, true), None);
            assert_eq!(accumulator.armed(), None);
            assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        }
    }

    #[test]
    fn started_begins_a_fresh_gesture() {
        let now = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        assert_eq!(accumulator.push(10.0, 0.0, TouchPhase::Started, now, true), None);
        assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        assert_eq!(scroll(&mut accumulator, 10.0, now), Some(NavigationDirection::Back));
    }

    #[test]
    fn cooldown_suppresses_momentum_refires_then_releases() {
        let start = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 120.0, start), Some(NavigationDirection::Back));
        let suppressed = start + Duration::from_millis(799);
        assert_eq!(scroll(&mut accumulator, 120.0, suppressed), None);
        let released = start + Duration::from_millis(800);
        assert_eq!(scroll(&mut accumulator, 120.0, released), Some(NavigationDirection::Back));
    }

    #[test]
    fn non_finite_deltas_reset_safely() {
        let now = Instant::now();
        for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut accumulator = SwipeScrollAccumulator::default();
            assert_eq!(scroll(&mut accumulator, 100.0, now), None);
            assert_eq!(accumulator.push(delta, 0.0, TouchPhase::Moved, now, true), None);
            assert_eq!(scroll(&mut accumulator, 100.0, now), None);
        }
    }

    #[test]
    fn armed_tracks_both_sides_of_the_cue_boundary() {
        let now = Instant::now();
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(accumulator.armed(), None);
        assert_eq!(scroll(&mut accumulator, 29.0, now), None);
        assert_eq!(accumulator.armed(), None);
        assert_eq!(scroll(&mut accumulator, 1.0, now), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Back));

        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, -30.0, now), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Forward));
    }
}
