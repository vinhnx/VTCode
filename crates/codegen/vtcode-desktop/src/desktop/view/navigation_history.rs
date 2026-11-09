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

/// Settle delay for an armed swipe navigation: a gesture stream with no
/// terminal phase (notched wheel, discrete system swipe) commits this long
/// after arming. Trackpad gestures usually commit earlier via `Ended` (finger
/// lift); the timer is the backstop, not the hot path.
pub(super) const SWIPE_COMMIT_DELAY: Duration = Duration::from_millis(200);

/// Quiet period after a swipe *commit* during which new arms are refused.
/// This is the only timing guard in swipe navigation: the scroll accumulator
/// is a pure geometric threshold detector, so without this a momentum tail or
/// a rapid second discrete swipe could arm (and commit) a second navigation
/// from one gesture. Measured from commit time, not arm time.
pub(super) const SWIPE_ARM_COOLDOWN: Duration = Duration::from_millis(1000);

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

/// Gesture state machine turning horizontal scrolls into arm candidates: a
/// pure geometric threshold detector with no timing of its own. Repeat-fire
/// protection lives in the arm layer ([`SwipeArming`]), which ignores
/// same-direction crossings while armed and refuses arms inside
/// [`SWIPE_ARM_COOLDOWN`] after a commit.
///
/// Contract:
/// - `push` is total: non-finite deltas reset and return `None`; it never
///   panics.
/// - Accumulation is single-signed: a direction reversal starts a fresh
///   gesture, so wiggling back and forth cannot ratchet to the threshold.
/// - Vertical-dominant input (`|dy| > 2|dx|`) resets without firing; it
///   belongs to the scrollable under the cursor.
/// - `Started` begins a fresh gesture; `Ended`/`Cancelled` reset without
///   firing, unwinding any in-progress interaction.
/// - Firing zeroes the accumulation, so a sustained scroll fires once per
///   full threshold traveled.
#[derive(Debug, Default)]
pub(super) struct SwipeScrollAccumulator {
    accumulated_x_px: f32,
    natural_scroll: bool,
}

impl SwipeScrollAccumulator {
    pub(super) fn push(
        &mut self,
        dx_px: f32,
        dy_px: f32,
        phase: TouchPhase,
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
        if is_vertical_dominant_scroll(dx_px, dy_px) {
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
        let direction = swipe_direction_for_dx(self.accumulated_x_px, self.natural_scroll);
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

    /// Drops the in-progress gesture.
    pub(super) fn reset(&mut self) {
        self.accumulated_x_px = 0.0;
    }
}

/// Whether a scroll delta belongs to the scrollable under the cursor rather
/// than swipe navigation. Shared by the accumulator and the armed-swipe cancel
/// check so the two cannot drift. Non-finite deltas report false here; callers
/// must reject those first (see [`SwipeScrollAccumulator::push`]).
pub(super) fn is_vertical_dominant_scroll(dx_px: f32, dy_px: f32) -> bool {
    dy_px.abs() > dx_px.abs() * 2.0
}

/// An armed-but-uncommitted swipe navigation: the overlay is visible and a
/// commit (finger lift or settle timer) will navigate unless canceled first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingSwipe {
    pub(super) direction: NavigationDirection,
    pub(super) generation: u64,
}

/// Outcome of an [`SwipeArming::arm`] attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArmOutcome {
    /// Fresh arm: show the overlay and start the commit timer.
    Armed,
    /// The opposite direction replaced the pending arm: restart the timer.
    Superseded,
    /// No state change: cooldown refusal or same-direction re-arm.
    Ignored,
}

/// Second-stage gate turning threshold crossings and discrete swipes into
/// debounced history navigation: at most one pending arm, at most one commit
/// per [`SWIPE_ARM_COOLDOWN`] window, across both input paths.
///
/// Contract:
/// - `arm`/`cancel`/`take_for_commit` are total: they never panic (cooldown
///   uses a saturating duration, generation wraps deliberately — it is an
///   opaque token compared only against the live pending arm).
/// - Same-direction input while armed is ignored, never extending the commit
///   window; opposite input supersedes.
/// - `take_for_commit` consumes the arm only on generation match, so a stale
///   timer (`Ended` already committed) or a superseded timer no-ops.
/// - Consuming an arm does NOT navigate: the caller must re-validate the
///   overlay and history guards before navigating, then call
///   `record_commit` only when a navigation actually issues.
#[derive(Debug, Default)]
pub(super) struct SwipeArming {
    pending: Option<PendingSwipe>,
    last_commit: Option<Instant>,
    generation: u64,
}

impl SwipeArming {
    pub(super) fn arm(&mut self, direction: NavigationDirection, now: Instant) -> (ArmOutcome, u64) {
        if self
            .last_commit
            .is_some_and(|committed| now.saturating_duration_since(committed) < SWIPE_ARM_COOLDOWN)
        {
            return (ArmOutcome::Ignored, self.generation);
        }
        if self.pending.is_some_and(|pending| pending.direction == direction) {
            return (ArmOutcome::Ignored, self.generation);
        }
        self.generation = self.generation.wrapping_add(1);
        let outcome = if self.pending.is_some() {
            ArmOutcome::Superseded
        } else {
            ArmOutcome::Armed
        };
        self.pending = Some(PendingSwipe { direction, generation: self.generation });
        (outcome, self.generation)
    }

    /// Drops the pending arm, invalidating its commit timer. Reports whether
    /// an arm was actually pending so the caller can skip redundant redraws.
    pub(super) fn cancel(&mut self) -> bool {
        if self.pending.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
            true
        } else {
            false
        }
    }

    /// Consumes the pending arm for a commit, but only when `generation`
    /// matches the live arm. Returns the direction to navigate toward; the
    /// caller still owns the guard re-check and the navigation itself.
    pub(super) fn take_for_commit(&mut self, generation: u64) -> Option<NavigationDirection> {
        let pending = self.pending.filter(|pending| pending.generation == generation)?;
        self.pending = None;
        Some(pending.direction)
    }

    /// Starts the arm cooldown. Call only when a navigation actually issues;
    /// a guard-blocked commit consumes the arm without arming the cooldown.
    pub(super) fn record_commit(&mut self, now: Instant) {
        self.last_commit = Some(now);
    }

    pub(super) fn pending_direction(&self) -> Option<NavigationDirection> {
        self.pending.map(|pending| pending.direction)
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
}

/// Maps a horizontal scroll offset to a history direction. Under natural
/// scrolling a positive offset means fingers moved right, which is Back —
/// mirroring the backend's discrete swipe mapping (`deltaX > 0 → Back` in
/// `gpui-pre-macos` `events.rs`); with natural scrolling off the deltas flip,
/// so the mapping flips with them. Returns `None` for a zero offset.
pub(super) fn swipe_direction_for_dx(dx_px: f32, natural_scroll: bool) -> Option<NavigationDirection> {
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
    use super::{
        ArmOutcome, NavigationHistory, NavigationLocation, SWIPE_ARM_COOLDOWN, SwipeArming, SwipeScrollAccumulator,
        is_vertical_dominant_scroll,
    };
    use gpui_kit::{NavigationDirection, TouchPhase};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn location(workspace: &str, session_id: Option<&str>) -> NavigationLocation {
        NavigationLocation {
            workspace: PathBuf::from(workspace),
            session_id: session_id.map(str::to_owned),
        }
    }

    fn scroll(accumulator: &mut SwipeScrollAccumulator, dx_px: f32) -> Option<NavigationDirection> {
        accumulator.push(dx_px, 0.0, TouchPhase::Moved, true)
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
    fn each_full_threshold_travel_fires_without_cooldown() {
        let mut back = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut back, 60.0), None);
        assert_eq!(scroll(&mut back, 60.0), Some(NavigationDirection::Back));
        // No timing guard lives here: a second full travel fires again
        // immediately, and the arm layer decides whether it may navigate.
        assert_eq!(scroll(&mut back, 120.0), Some(NavigationDirection::Back));

        let mut forward = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut forward, -60.0), None);
        assert_eq!(scroll(&mut forward, -60.0), Some(NavigationDirection::Forward));
        assert_eq!(scroll(&mut forward, -120.0), Some(NavigationDirection::Forward));
    }

    #[test]
    fn non_natural_scrolling_flips_the_mapping() {
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(accumulator.push(60.0, 0.0, TouchPhase::Moved, false), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Forward));
        assert_eq!(accumulator.push(60.0, 0.0, TouchPhase::Moved, false), Some(NavigationDirection::Forward));
        assert_eq!(accumulator.armed(), None);
    }

    #[test]
    fn vertical_dominant_input_resets_instead_of_firing() {
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0), None);
        assert_eq!(accumulator.push(10.0, 30.0, TouchPhase::Moved, true), None);
        // Without the reset this second push would cross the threshold and
        // fire; instead exactly 100px remain, which arms but does not fire.
        assert_eq!(scroll(&mut accumulator, 100.0), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Back));
    }

    #[test]
    fn direction_reversal_restarts_the_gesture() {
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0), None);
        assert_eq!(scroll(&mut accumulator, -30.0), None);
        // A net sum would sit at -130 and fire; a restart sits at -30 + -100.
        assert_eq!(scroll(&mut accumulator, -100.0), Some(NavigationDirection::Forward));
    }

    #[test]
    fn terminal_phases_reset_without_firing() {
        for phase in [TouchPhase::Ended, TouchPhase::Cancelled] {
            let mut accumulator = SwipeScrollAccumulator::default();
            assert_eq!(scroll(&mut accumulator, 100.0), None);
            assert_eq!(accumulator.push(100.0, 0.0, phase, true), None);
            assert_eq!(accumulator.armed(), None);
            assert_eq!(scroll(&mut accumulator, 100.0), None);
        }
    }

    #[test]
    fn started_begins_a_fresh_gesture() {
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, 100.0), None);
        assert_eq!(accumulator.push(10.0, 0.0, TouchPhase::Started, true), None);
        assert_eq!(scroll(&mut accumulator, 100.0), None);
        assert_eq!(scroll(&mut accumulator, 10.0), Some(NavigationDirection::Back));
    }

    #[test]
    fn non_finite_deltas_reset_safely() {
        for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut accumulator = SwipeScrollAccumulator::default();
            assert_eq!(scroll(&mut accumulator, 100.0), None);
            assert_eq!(accumulator.push(delta, 0.0, TouchPhase::Moved, true), None);
            assert_eq!(scroll(&mut accumulator, 100.0), None);
        }
    }

    #[test]
    fn armed_tracks_both_sides_of_the_cue_boundary() {
        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(accumulator.armed(), None);
        assert_eq!(scroll(&mut accumulator, 29.0), None);
        assert_eq!(accumulator.armed(), None);
        assert_eq!(scroll(&mut accumulator, 1.0), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Back));

        let mut accumulator = SwipeScrollAccumulator::default();
        assert_eq!(scroll(&mut accumulator, -30.0), None);
        assert_eq!(accumulator.armed(), Some(NavigationDirection::Forward));
    }

    #[test]
    fn same_direction_rearm_is_ignored_without_bumping_generation() {
        let now = Instant::now();
        let mut arming = SwipeArming::default();
        let (outcome, generation) = arming.arm(NavigationDirection::Back, now);
        assert_eq!(outcome, ArmOutcome::Armed);
        assert_eq!(arming.pending_direction(), Some(NavigationDirection::Back));

        let (outcome, same_generation) = arming.arm(NavigationDirection::Back, now);
        assert_eq!(outcome, ArmOutcome::Ignored);
        assert_eq!(same_generation, generation);
        assert_eq!(arming.generation(), generation);
    }

    #[test]
    fn opposite_direction_supersedes_in_both_orders() {
        let now = Instant::now();
        for (first, second) in [
            (NavigationDirection::Back, NavigationDirection::Forward),
            (NavigationDirection::Forward, NavigationDirection::Back),
        ] {
            let mut arming = SwipeArming::default();
            let (_, first_generation) = arming.arm(first, now);
            let (outcome, second_generation) = arming.arm(second, now);
            assert_eq!(outcome, ArmOutcome::Superseded);
            assert_ne!(second_generation, first_generation);
            assert_eq!(arming.pending_direction(), Some(second));
            // The superseded timer no-ops; only the current generation commits.
            assert_eq!(arming.take_for_commit(first_generation), None);
            assert_eq!(arming.take_for_commit(second_generation), Some(second));
        }
    }

    #[test]
    fn cooldown_refuses_arms_until_it_releases_on_both_sides() {
        let committed = Instant::now();
        let mut arming = SwipeArming::default();
        let (_, generation) = arming.arm(NavigationDirection::Back, committed);
        assert_eq!(arming.take_for_commit(generation), Some(NavigationDirection::Back));
        arming.record_commit(committed);

        // At the commit instant and just inside the window: refused.
        for now in [committed, committed + Duration::from_millis(999)] {
            let (outcome, _) = arming.arm(NavigationDirection::Forward, now);
            assert_eq!(outcome, ArmOutcome::Ignored);
            assert_eq!(arming.pending_direction(), None);
        }
        // Exactly at the window edge: released.
        let (outcome, _) = arming.arm(NavigationDirection::Forward, committed + SWIPE_ARM_COOLDOWN);
        assert_eq!(outcome, ArmOutcome::Armed);
        assert_eq!(arming.pending_direction(), Some(NavigationDirection::Forward));
    }

    #[test]
    fn canceled_arm_starts_no_cooldown() {
        let now = Instant::now();
        let mut arming = SwipeArming::default();
        assert!(!arming.cancel(), "canceling with no arm reports false");
        let _ = arming.arm(NavigationDirection::Back, now);
        assert!(arming.cancel());
        assert_eq!(arming.pending_direction(), None);
        // No commit issued, so an immediate re-arm succeeds.
        let (outcome, _) = arming.arm(NavigationDirection::Forward, now);
        assert_eq!(outcome, ArmOutcome::Armed);
    }

    #[test]
    fn commit_consumes_the_arm_exactly_once() {
        let now = Instant::now();
        let mut arming = SwipeArming::default();
        let (_, generation) = arming.arm(NavigationDirection::Forward, now);
        assert_eq!(arming.take_for_commit(generation), Some(NavigationDirection::Forward));
        assert_eq!(arming.pending_direction(), None);
        assert_eq!(arming.take_for_commit(generation), None, "a second commit finds no arm");
        assert_eq!(arming.take_for_commit(generation.wrapping_add(1)), None);
    }

    #[test]
    fn cancel_invalidates_the_outstanding_commit() {
        let now = Instant::now();
        let mut arming = SwipeArming::default();
        let (_, generation) = arming.arm(NavigationDirection::Back, now);
        assert!(arming.cancel());
        assert_eq!(arming.take_for_commit(generation), None);
    }

    #[test]
    fn vertical_dominance_holds_on_both_sides_and_signs() {
        // Boundary: |dy| == 2|dx| is not dominant; one step past is.
        assert!(!is_vertical_dominant_scroll(10.0, 20.0));
        assert!(!is_vertical_dominant_scroll(-10.0, -20.0));
        assert!(is_vertical_dominant_scroll(10.0, 21.0));
        assert!(is_vertical_dominant_scroll(-10.0, -21.0));
        assert!(is_vertical_dominant_scroll(0.0, 1.0));
        assert!(!is_vertical_dominant_scroll(0.0, 0.0));
        assert!(!is_vertical_dominant_scroll(120.0, 0.0));
    }
}
