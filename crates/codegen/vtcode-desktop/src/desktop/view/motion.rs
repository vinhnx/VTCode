//! Unified motion policy for Desktop presentation fades.
//!
//! Every opacity reveal resolves its duration and easing from the theme's
//! motion tokens through [`fade_timing`] and builds its GPUI animation with
//! [`fade_animation`]. Navigation reveals sample with absolute time, resume
//! from the last sampled value when retargeted, and request frames only while
//! the value is still moving.
use std::cell::Cell;

use super::*;

/// A resumed reveal never compresses below this fraction of its base duration.
const MIN_REVEAL_SCALE: f32 = 0.15;

pub(super) struct NavigationMotion<T> {
    destination: Option<T>,
    pub(super) started_at: Option<Instant>,
    pub(super) returning: bool,
    /// Duration used by the last sample, so `update` can tell a running fade
    /// from a settled one without consulting the theme.
    duration: Cell<Duration>,
    /// Last sampled opacity (1.0 when nothing is fading).
    sampled: Cell<f32>,
    /// Opacity this reveal started from; retargeting resumes from the sample.
    from: f32,
}

impl<T> Default for NavigationMotion<T> {
    fn default() -> Self {
        Self {
            destination: None,
            started_at: None,
            returning: false,
            duration: Cell::new(Duration::ZERO),
            sampled: Cell::new(1.0),
            from: 0.0,
        }
    }
}

impl<T: PartialEq> NavigationMotion<T> {
    pub(super) fn is_current(&self, destination: &T) -> bool {
        self.destination.as_ref() == Some(destination)
    }

    pub(super) fn update(&mut self, destination: T, returning: bool, revisit: bool, now: Instant) -> bool {
        if !revisit && self.is_current(&destination) {
            return false;
        }
        let running = self
            .started_at
            .is_some_and(|start| now.saturating_duration_since(start) < self.duration.get());
        // Interruption contract: a running reveal continues from what is on
        // screen instead of flashing back to transparent; a settled surface
        // starts a fresh reveal. The remaining distance also shortens the
        // duration, so retargeting early does not replay a full-length fade.
        self.from = if running { self.sampled.get() } else { 0.0 };
        if self.destination.replace(destination).is_some() || revisit {
            self.started_at = Some(now);
            self.returning = returning;
            return true;
        }
        false
    }

    /// Apply opacity directly so a reveal keeps its clock and its descendants'
    /// element IDs across layout changes. Settled surfaces request no frames.
    pub(super) fn fade(&self, content: gpui_kit::Div, fast: bool, window: &Window, cx: &App) -> gpui_kit::Div {
        let (base_duration, curve) = fade_timing(cx.theme(), fast, self.returning);
        self.duration.set(base_duration);
        if cx.reduce_motion() {
            self.sampled.set(1.0);
            return content;
        }
        let Some(start) = self.started_at else {
            self.sampled.set(1.0);
            return content;
        };
        let duration = reveal_duration(base_duration, self.from);
        let elapsed = cx.background_executor().now().saturating_duration_since(start);
        if elapsed >= duration {
            self.sampled.set(1.0);
            return content;
        }
        window.request_animation_frame();
        let progress = elapsed.as_secs_f32() / duration.as_secs_f32();
        let value = reveal_value(self.from, curve.sample(progress));
        self.sampled.set(value);
        content.opacity(value)
    }
}

/// The single timing source for Desktop fades. `returning` selects the
/// decelerating enter curve; forward and fast reveals use the accelerating
/// exit curve, which gives a reveal its ease-in shape.
pub(super) fn fade_timing(theme: &Theme, fast: bool, returning: bool) -> (Duration, &gpui_kit::base::Easing) {
    let motion = theme.motion_tokens();
    let duration = if fast {
        motion.duration_fast
    } else {
        motion.duration_normal
    };
    let curve = if returning {
        &motion.easing_enter
    } else {
        &motion.easing_exit
    };
    (duration, curve)
}

/// The one GPUI animation builder used by mount and navigation fades.
pub(super) fn fade_animation(theme: &Theme, fast: bool, returning: bool) -> Animation {
    let (duration, curve) = fade_timing(theme, fast, returning);
    let curve = curve.clone();
    Animation::new(duration).with_easing(move |delta| curve.sample(delta))
}

/// A reveal that resumes partway covers less distance, so it finishes sooner
/// instead of replaying a full-duration fade.
fn reveal_duration(base: Duration, from: f32) -> Duration {
    base.mul_f64(f64::from((1.0 - from).clamp(MIN_REVEAL_SCALE, 1.0)))
}

/// Interpolates from the sampled value to fully opaque.
fn reveal_value(from: f32, eased: f32) -> f32 {
    from + (1.0 - from) * eased
}

pub(super) trait NavigationFadeExt: AnimationExt + Sized {
    fn navigation_fade(
        self,
        id: impl Into<gpui_kit::ElementId>,
        fast: bool,
        returning: bool,
        theme: &Theme,
    ) -> gpui_kit::AnimationElement<Self>
    where
        Self: Styled,
    {
        self.with_animation(id, fade_animation(theme, fast, returning), |el, delta| el.opacity(delta))
    }
}

impl<T: AnimationExt + Sized> NavigationFadeExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_sampling_is_bounded_and_asymmetric() {
        let base = Duration::from_millis(180);
        assert_eq!(reveal_duration(base, 0.0), base, "a fresh reveal keeps the full duration");
        let shortened = reveal_duration(base, 0.6);
        assert!(shortened >= Duration::from_millis(71) && shortened <= Duration::from_millis(73));
        let clamped = reveal_duration(base, 1.0);
        assert!(clamped >= Duration::from_millis(26) && clamped <= Duration::from_millis(28));

        assert_eq!(reveal_value(0.0, 0.25), 0.25);
        assert!((reveal_value(0.6, 0.25) - 0.7).abs() < 1e-6);
        assert!((reveal_value(0.6, 1.0) - 1.0).abs() < 1e-6);
        assert_eq!(reveal_value(1.0, 0.0), 1.0, "a settled value cannot move backwards");
    }

    #[test]
    fn retarget_resumes_from_sampled_value_and_shortens_the_reveal() {
        let mut motion = NavigationMotion::<u8>::default();
        let now = Instant::now();
        motion.update(1, false, false, now);
        assert!(motion.update(2, false, false, now));
        motion.sampled.set(0.6);
        motion.duration.set(Duration::from_millis(180));
        let later = now + Duration::from_millis(10);
        assert!(motion.update(3, true, false, later));
        assert_eq!(motion.from, 0.6, "a running reveal resumes at the sampled opacity");
        assert_eq!(motion.started_at, Some(later));
        assert!(motion.returning);
        let shortened = reveal_duration(motion.duration.get(), motion.from);
        assert!(
            shortened >= Duration::from_millis(71) && shortened <= Duration::from_millis(73),
            "a return from 0.6 must cover only the remaining 0.4 of the 180ms duration"
        );

        motion.sampled.set(1.0);
        let settled = later + Duration::from_millis(180);
        assert!(motion.update(4, false, false, settled));
        assert_eq!(motion.from, 0.0, "a settled destination starts a fresh reveal");
    }

    #[test]
    fn redraw_of_the_same_destination_keeps_the_reveal_clock() {
        let mut motion = NavigationMotion::<u8>::default();
        let now = Instant::now();
        motion.update(1, false, false, now);
        assert!(motion.update(2, false, false, now));
        assert!(!motion.update(2, false, false, now + Duration::from_millis(20)));
        assert_eq!(motion.started_at, Some(now));
        assert_eq!(motion.from, 0.0);
    }
}
