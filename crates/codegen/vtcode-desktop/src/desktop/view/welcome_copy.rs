//! Welcome-column greeting copy: day-period greeting plus rotating prompts.

pub(super) const WELCOME_COPY_COUNT: usize = 3;

pub(super) struct WelcomeCopy {
    pub(super) action: String,
    pub(super) workspace_action: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WelcomePeriod {
    Morning,
    Afternoon,
    Evening,
}

pub(super) fn welcome_period_for_hour(hour: u32) -> WelcomePeriod {
    match hour % 24 {
        5..=11 => WelcomePeriod::Morning,
        12..=16 => WelcomePeriod::Afternoon,
        _ => WelcomePeriod::Evening,
    }
}

pub(super) fn welcome_greeting(period: WelcomePeriod) -> String {
    match period {
        WelcomePeriod::Morning => rust_i18n::t!("app.welcome.greeting.morning"),
        WelcomePeriod::Afternoon => rust_i18n::t!("app.welcome.greeting.afternoon"),
        WelcomePeriod::Evening => rust_i18n::t!("app.welcome.greeting.evening"),
    }
    .to_string()
}

pub(super) fn welcome_copy(index: usize) -> WelcomeCopy {
    match index {
        1 => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.improve").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.improve_in_workspace").to_string(),
        },
        2 => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.explore").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.explore_in_workspace").to_string(),
        },
        _ => WelcomeCopy {
            action: rust_i18n::t!("app.welcome.action.build").to_string(),
            workspace_action: rust_i18n::t!("app.welcome.action.build_in_workspace").to_string(),
        },
    }
}

pub(super) fn welcome_prompt(
    greeting: &str,
    action: &str,
    workspace_action: &str,
    workspace_name: Option<&str>,
) -> String {
    match workspace_name {
        Some(name) => format!("{greeting}. {workspace_action} {name}?"),
        None => format!("{greeting}. {action}?"),
    }
}
