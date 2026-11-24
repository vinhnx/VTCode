//! Unified motion policy for Desktop presentation fades.
//!
//! Every opacity reveal resolves its duration and easing from the theme's
//! motion tokens through [`fade_timing`] and builds its GPUI animation with
//! [`fade_animation`]. Navigation reveals sample with absolute time, resume
//! from the last sampled value when retargeted, and request frames only while
//! the value is still moving.
//!
//! Choice of mechanism follows what owns the changing value (adapted from the
//! GPUI Kit [animation guide](https://gpui-kit.com/docs/animation/),
//! [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); checked against
//! pinned `0.10.0`): a mounted one-shot appearance uses GPUI's
//! [`Animation`] via [`opacity_fade`], while a destination that can change
//! before settling uses [`NavigationMotion`]'s sampled clock. The wrapper's
//! `id` is the playback identity — GPUI continues a mounted wrapper with the
//! same id and starts a fresh playback only when the id changes or the wrapper
//! remounts. Callers must therefore give each surface a stable id and include
//! an application-owned generation only when a fresh appearance should replay.
use std::cell::Cell;

use super::*;

/// A resumed reveal never compresses below this fraction of its base duration.
const MIN_REVEAL_SCALE: f32 = 0.15;

pub(super) struct NavigationMotion<T> {
    destination: Option<T>,
    pub(super) started_at: Option<Instant>,
    pub(super) returning: bool,
    /// Base duration from the last sample; `update` pairs it with `sampled`
    /// to tell a running fade from a settled one without consulting the theme.
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
        // `sampled` is the authority on what is on screen: it is < 1.0 only
        // while a reveal is painting. A settled fade's clock can still look
        // running (shortened reveal, or no fade rendered for the Workspace
        // route), so resume only when opacity is actually mid-reveal.
        let running = self.sampled.get() < 1.0
            && self
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

/// The one GPUI animation builder used by mount and navigation fades. GPUI's
/// easing output may overshoot `0..=1`; opacity is narrower, so clamp before
/// the value reaches a style property (pinned `Easing::sample` requires it).
pub(super) fn fade_animation(theme: &Theme, fast: bool, returning: bool) -> Animation {
    let (duration, curve) = fade_timing(theme, fast, returning);
    let curve = curve.clone();
    Animation::new(duration).with_easing(move |delta| curve.sample(delta).clamp(0.0, 1.0))
}

/// A reveal that resumes partway covers less distance, so it finishes sooner
/// instead of replaying a full-duration fade.
fn reveal_duration(base: Duration, from: f32) -> Duration {
    base.mul_f64(f64::from((1.0 - from).clamp(MIN_REVEAL_SCALE, 1.0)))
}

/// Interpolates from the sampled value to fully opaque. A reveal never moves
/// backwards or past full opacity, even when a theme's curve anticipates or
/// overshoots `0..=1`.
fn reveal_value(from: f32, eased: f32) -> f32 {
    (from + (1.0 - from) * eased).clamp(from, 1.0)
}

/// The one opacity-reveal wrapper for every one-shot appearance. Both the
/// mount fade and the navigation fade delegate here so the animation builder
/// and the animator cannot drift. `id` is the playback identity, not a label
/// for the content: reusing it keeps a mounted reveal at its current value,
/// while a new id (or a remount) replays it from transparent.
pub(super) fn opacity_fade<E>(
    element: E,
    id: impl Into<gpui_kit::ElementId>,
    theme: &Theme,
    fast: bool,
    returning: bool,
) -> gpui_kit::AnimationElement<E>
where
    E: AnimationExt + Styled + Sized,
{
    element.with_animation(id, fade_animation(theme, fast, returning), |el, delta| el.opacity(delta))
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
        opacity_fade(self, id, theme, fast, returning)
    }
}

impl<T: AnimationExt + Sized> NavigationFadeExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_opacity_fade_is_a_finite_oneshot() {
        // Both fade wrappers delegate to `opacity_fade`, whose animation comes
        // from this one builder. GPUI requires finite easing output; opacity
        // also needs the value clamped to 0..=1 because a valid curve may
        // overshoot, and reduced motion renders the one-shot's end state.
        let theme = Theme::default();
        for (fast, returning) in [(false, true), (true, false), (false, false)] {
            let animation = fade_animation(&theme, fast, returning);
            assert!(animation.oneshot, "mount and navigation fades are one-shots");
            assert_eq!(animation.duration, fade_timing(&theme, fast, returning).0);
            for step in 0..=20 {
                let eased = (animation.easing)(step as f32 / 20.0);
                assert!(eased.is_finite() && (0.0..=1.0).contains(&eased), "eased opacity {eased} left 0..=1");
            }
        }

        // A curve whose y control point is outside 0..=1 is valid for GPUI but
        // not for opacity. The builder must clamp it instead of painting an
        // out-of-range alpha when a theme opts into an anticipating curve.
        let overshoot = gpui_kit::base::Easing::cubic_bezier(0.16, 1.2, 0.3, 1.0).expect("valid curve");
        assert!(
            (0..=20).any(|step| overshoot.sample(step as f32 / 20.0) > 1.0),
            "precondition: the raw curve overshoots"
        );
        let mut theme = Theme::default();
        theme.motion.easing_enter = overshoot;
        let animation = fade_animation(&theme, false, true);
        for step in 0..=20 {
            let eased = (animation.easing)(step as f32 / 20.0);
            assert!((0.0..=1.0).contains(&eased), "clamped opacity {eased} left 0..=1");
        }
    }

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
        assert_eq!(reveal_value(0.6, 1.2), 1.0, "an overshooting curve stops at full opacity");
        assert_eq!(reveal_value(0.6, -0.1), 0.6, "an anticipating curve cannot move backwards");
    }

    #[test]
    fn retarget_resumes_from_sampled_value_and_shortens_the_reveal() {
        let mut motion = NavigationMotion::<u8>::default();
        let now = Instant::now();
        let _ = motion.update(1, false, false, now);
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
    fn settled_reveal_starts_fresh_while_the_base_clock_is_still_running() {
        let mut motion = NavigationMotion::<u8>::default();
        let now = Instant::now();
        let _ = motion.update(1, false, false, now);
        assert!(motion.update(2, false, false, now));

        // A resumed reveal settles after 72ms, while the stored base duration
        // remains 180ms — the clock alone still says "running".
        motion.sampled.set(0.6);
        motion.duration.set(Duration::from_millis(180));
        let resumed = now + Duration::from_millis(10);
        assert!(motion.update(3, false, false, resumed));
        assert_eq!(motion.from, 0.6);

        motion.sampled.set(1.0);
        let settled = resumed + Duration::from_millis(90);
        assert!(
            settled < resumed + Duration::from_millis(180),
            "precondition: the base-duration clock is still inside its window"
        );
        assert!(motion.update(4, false, false, settled));
        assert_eq!(
            motion.from, 0.0,
            "a fully opaque surface must start a fresh reveal even when the clock looks active"
        );

        // A route that resets its start without painting a fade has the same
        // opaque sample and must not skip the next reveal either.
        assert!(motion.update(5, true, true, settled + Duration::from_millis(1)));
        assert_eq!(motion.from, 0.0);
    }

    #[test]
    fn redraw_of_the_same_destination_keeps_the_reveal_clock() {
        let mut motion = NavigationMotion::<u8>::default();
        let now = Instant::now();
        let _ = motion.update(1, false, false, now);
        assert!(motion.update(2, false, false, now));
        assert!(!motion.update(2, false, false, now + Duration::from_millis(20)));
        assert_eq!(motion.started_at, Some(now));
        assert_eq!(motion.from, 0.0);
    }
}
