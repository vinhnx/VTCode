//! Presentation-only motion for committed Desktop destinations.
use super::*;

pub(super) struct NavigationMotion<T> {
    destination: Option<T>,
    pub(super) started_at: Option<Instant>,
    pub(super) returning: bool,
}

impl<T> Default for NavigationMotion<T> {
    fn default() -> Self {
        Self {
            destination: None,
            started_at: None,
            returning: false,
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
        if self.destination.replace(destination).is_some() || revisit {
            self.started_at = Some(now);
            self.returning = returning;
            return true;
        }
        false
    }

    // Apply opacity directly so a transition keeps its clock and descendants'
    // element IDs across layout changes. Settled surfaces request no frames.
    pub(super) fn fade(&self, content: gpui_kit::Div, fast: bool, window: &Window, cx: &App) -> gpui_kit::Div {
        if cx.reduce_motion() {
            return content;
        }
        let Some(start) = self.started_at else {
            return content;
        };
        let (duration, curve) = navigation_timing(cx.theme(), fast, self.returning);
        let elapsed = cx.background_executor().now().saturating_duration_since(start);
        if elapsed >= duration {
            return content;
        }
        window.request_animation_frame();
        content.opacity(curve.sample(elapsed.as_secs_f32() / duration.as_secs_f32()))
    }
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
        let (duration, curve) = navigation_timing(theme, fast, returning);
        let curve = curve.clone();
        self.with_animation(id, Animation::new(duration).with_easing(move |delta| curve.sample(delta)), |el, delta| {
            el.opacity(delta)
        })
    }
}

impl<T: AnimationExt + Sized> NavigationFadeExt for T {}

// The accelerating exit curve also gives a forward reveal its ease-in shape.
fn navigation_timing(theme: &Theme, fast: bool, returning: bool) -> (Duration, &gpui_kit::base::Easing) {
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
