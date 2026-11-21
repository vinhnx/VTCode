//! Local day-period greeting shared by normal and Zen mode.

use chrono::Timelike;

use super::{Context, DesktopView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WelcomePeriod {
    Morning,
    Afternoon,
    Evening,
}

impl WelcomePeriod {
    pub(super) fn current() -> Self {
        Self::for_hour(chrono::Local::now().hour())
    }

    fn for_hour(hour: u32) -> Self {
        match hour {
            5..=11 => Self::Morning,
            12..=16 => Self::Afternoon,
            _ => Self::Evening,
        }
    }

    pub(super) fn label(self) -> String {
        match self {
            Self::Morning => rust_i18n::t!("app.welcome.greeting.morning"),
            Self::Afternoon => rust_i18n::t!("app.welcome.greeting.afternoon"),
            Self::Evening => rust_i18n::t!("app.welcome.greeting.evening"),
        }
        .into_owned()
    }
}

impl DesktopView {
    pub(super) fn refresh_welcome_period(&mut self, cx: &mut Context<Self>) {
        let period = WelcomePeriod::current();
        if period != self.welcome_period {
            self.welcome_period = period;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WelcomePeriod::{self, Afternoon, Evening, Morning};

    #[test]
    fn welcome_greeting_changes_at_local_day_period_boundaries() {
        for (hour, expected) in [
            (0, Evening),
            (4, Evening),
            (5, Morning),
            (11, Morning),
            (12, Afternoon),
            (16, Afternoon),
            (17, Evening),
            (23, Evening),
        ] {
            assert_eq!(WelcomePeriod::for_hour(hour), expected, "hour {hour}");
        }
    }
}
