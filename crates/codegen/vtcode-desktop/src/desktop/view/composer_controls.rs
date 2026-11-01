//! Native controls over the service's authoritative session options.
use super::*;
use gpui_kit::Hsla;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposerTriggerKind {
    FileMention,
    SlashCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ComposerTrigger {
    pub(super) kind: ComposerTriggerKind,
    pub(super) range: Range<usize>,
    pub(super) query: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DesktopSlashAction {
    NewSession,
    CloseSession,
    StopTurn,
    ModelPicker,
    History,
    Settings(Option<settings::SettingsTab>),
    Configuration(Option<String>),
    Permissions,
    Modes,
    Files(String),
    FastMode(Option<bool>),
    Theme(Option<String>),
    Splash,
    Help,
    Go(String),
}

/// Settings-tab aliases shared by the `/config` and `/settings` deep-links
/// and the [`deeplinks`](super::deeplinks) system.
pub(super) fn settings_tab_for_name(name: &str) -> Option<settings::SettingsTab> {
    match name {
        "appearance" | "theme" => Some(settings::SettingsTab::Appearance),
        "accessibility" | "a11y" => Some(settings::SettingsTab::Accessibility),
        "shortcuts" | "keybindings" => Some(settings::SettingsTab::Shortcuts),
        "vtcode" | "general" => Some(settings::SettingsTab::VtCode),
        "connections" | "mcp" | "webmcp" => Some(settings::SettingsTab::Connections),
        "notifications" | "notification" | "alerts" | "updates" => Some(settings::SettingsTab::Notifications),
        "storage" | "disk" | "cleanup" => Some(settings::SettingsTab::Storage),
        _ => None,
    }
}

/// Native routes for bare `model`/`permissions` keywords shared by the
/// `/config` and `/settings` deep-links. Each caller keeps its own fallback
/// for anything else.
fn keyword_slash_action(keyword: &str) -> Option<DesktopSlashAction> {
    match keyword {
        "model" | "models" => Some(DesktopSlashAction::ModelPicker),
        "permissions" | "permission" => Some(DesktopSlashAction::Permissions),
        _ => None,
    }
}

/// Commands with a native Desktop equivalent; all other slash input
/// remains owned by the shared CLI dispatcher. `/config` additionally
/// deep-links setting paths and groups into the configuration page and treats
/// anything outside the runtime-owned sections as a catalog search query.
pub(super) fn desktop_slash_action(input: &str) -> Option<DesktopSlashAction> {
    if input.chars().any(char::is_control) {
        return None;
    }
    let body = input.trim().strip_prefix('/')?;
    let (name, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    let args = args.trim();
    // The CLI dispatcher lowercases the command token (see
    // `src/agent/runloop/slash_commands/dispatch.rs`); mirror that so a
    // capitalized native command cannot fall through to the headless service.
    // `new`, `exit`, `stop`, and `model` also ignore arguments in the CLI
    // (`builtins.rs` takes no args for them), so they route natively
    // regardless of trailing text: `/NEW foo` would otherwise tear down the
    // service-side session while this client keeps showing it as selected,
    // `/exit foo` the same, `/stop foo` would be silently dropped by the idle
    // gate mid-run, and `/model foo` would park the text-driven picker and
    // swallow later turns. Every remaining fallthrough below is headless-safe
    // (pure text, usage errors, or text-driven selection).
    let command = name.to_ascii_lowercase();
    let keyword = args.to_ascii_lowercase();
    match command.as_str() {
        "new" => Some(DesktopSlashAction::NewSession),
        "exit" => Some(DesktopSlashAction::CloseSession),
        "stop" => Some(DesktopSlashAction::StopTurn),
        "model" | "models" => Some(DesktopSlashAction::ModelPicker),
        "history" if args.is_empty() => Some(DesktopSlashAction::History),
        "splash" if args.is_empty() => Some(DesktopSlashAction::Splash),
        "help" if args.is_empty() => Some(DesktopSlashAction::Help),
        "mode" if args.is_empty() => Some(DesktopSlashAction::Modes),
        "files" => Some(DesktopSlashAction::Files(args.to_string())),
        "fast" => match keyword.as_str() {
            "" => Some(DesktopSlashAction::FastMode(None)),
            "on" => Some(DesktopSlashAction::FastMode(Some(true))),
            "off" => Some(DesktopSlashAction::FastMode(Some(false))),
            _ => None,
        },
        "theme" => match args.split_whitespace().collect::<Vec<_>>().as_slice() {
            [] => Some(DesktopSlashAction::Theme(None)),
            [theme] => Some(DesktopSlashAction::Theme(Some((*theme).to_string()))),
            _ => None,
        },
        "config" if args.is_empty() => Some(DesktopSlashAction::Configuration(None)),
        "settings" if args.is_empty() => Some(DesktopSlashAction::Settings(None)),
        "go" => Some(DesktopSlashAction::Go(args.to_string())),
        "config" => {
            // Runtime-owned sections keep their CLI behavior: reset/clear show
            // the target layer and ask for confirmation, checkup/doctor run
            // diagnostics, and memory/tasks/jobs/log/subprocess/notify render
            // CLI-owned views. The CLI matches the first word
            // case-insensitively (see `handle_config_command` in
            // `src/agent/runloop/slash_commands/builtins.rs`); mirror that so
            // capitalized forms keep working. None of these is a setting path
            // or group.
            let first = args.split_whitespace().next().unwrap_or_default();
            if [
                "reset",
                "clear",
                "checkup",
                "doctor",
                "memory",
                "agent.persistent_memory",
                "tasks",
                "jobs",
                "log",
                "subprocess",
                "subprocesses",
                "notify",
            ]
            .iter()
            .any(|command| first.eq_ignore_ascii_case(command))
            {
                return None;
            }
            if let Some(tab) = settings_tab_for_name(&keyword) {
                return Some(DesktopSlashAction::Settings(Some(tab)));
            }
            if let Some(action) = keyword_slash_action(&keyword) {
                return Some(action);
            }
            Some(DesktopSlashAction::Configuration(Some(args.to_string())))
        }
        "settings" => {
            if let Some(tab) = settings_tab_for_name(&keyword) {
                return Some(DesktopSlashAction::Settings(Some(tab)));
            }
            keyword_slash_action(&keyword)
        }
        _ => None,
    }
}

/// Match the CLI's leading slash-command token or a whitespace-bounded @file token.
pub(super) fn composer_trigger(input: &str, cursor: usize) -> Option<ComposerTrigger> {
    if cursor > input.len() || !input.is_char_boundary(cursor) {
        return None;
    }

    if input.starts_with('/') && cursor > 0 {
        let query = input.get(1..cursor)?;
        if !query.chars().any(char::is_whitespace) {
            let end = input
                .get(1..)?
                .find(char::is_whitespace)
                .map_or(input.len(), |offset| offset + 1);
            if cursor <= end {
                return Some(ComposerTrigger {
                    kind: ComposerTriggerKind::SlashCommand,
                    range: 0..end,
                    query: query.to_string(),
                });
            }
        }
    }

    let before_cursor = input.get(..cursor)?;
    let after_cursor = input.get(cursor..)?;
    let start = before_cursor
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map_or(0, |(index, character)| index + character.len_utf8());
    let end = after_cursor
        .char_indices()
        .find(|(_, character)| character.is_whitespace())
        .map_or(input.len(), |(offset, _)| cursor + offset);
    let token = input.get(start..end)?;
    if !token.starts_with('@')
        || is_package_manager_context(input, start)
        || !looks_like_file_reference(token.get(1..)?)
    {
        return None;
    }

    let query_end = cursor.max(start + 1).min(end);
    Some(ComposerTrigger {
        kind: ComposerTriggerKind::FileMention,
        range: start..end,
        query: input.get(start + 1..query_end)?.to_string(),
    })
}

pub(super) fn file_mention_token(path: &str) -> Option<String> {
    if path.chars().any(char::is_control) {
        return None;
    }
    let needs_quotes = path.chars().any(|character| {
        character.is_whitespace()
            || matches!(character, '"' | '\'' | '[' | ']' | '(' | ')' | '{' | '}' | '<' | '>' | '|' | '\\' | '^' | '`')
    });
    if !needs_quotes {
        return Some(format!("@{path}"));
    }
    let quote = if !path.contains('"') {
        '"'
    } else if !path.contains('\'') {
        '\''
    } else {
        return None;
    };
    Some(format!("@{quote}{path}{quote}"))
}

fn is_package_manager_context(input: &str, at_pos: usize) -> bool {
    input.get(..at_pos).is_some_and(|prefix| {
        let command = prefix.rsplit(['\n', ';', '|', '&']).next().unwrap_or_default();
        let mut words = command.split_whitespace();
        let first = words.next();
        let package_manager = if matches!(first, Some("$" | ">" | "%")) {
            words.next()
        } else {
            first
        };
        matches!(package_manager, Some("npm" | "npx" | "yarn" | "pnpm" | "bun"))
    })
}

pub(super) fn capitalize_agent_label(label: &str) -> String {
    let mut characters = label.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

pub(super) fn runtime_option_label(label: &str) -> String {
    if label.eq_ignore_ascii_case("ultrafast") {
        "Ultrafast".into()
    } else if label.eq_ignore_ascii_case("fast") {
        "Fast".into()
    } else {
        capitalize_agent_label(label)
    }
}

pub(super) fn permission_indicator(
    mode: Option<protocol::PermissionMode>,
    colors: gpui_kit::component::theme::ThemeColor,
) -> (IconName, Hsla) {
    // A few upstream light palettes map warning and success to the same token.
    // Keep permission states visually distinct there, then adapt those seeds to
    // the active surface below.
    let semantic_colors_overlap = colors.success == colors.warning;
    let (ask_seed, auto_seed) = themes::distinct_permission_seeds();
    let (icon, color) = match mode {
        Some(protocol::PermissionMode::Ask) => (
            IconName::ShieldCheck,
            if semantic_colors_overlap {
                ask_seed
            } else {
                colors.success
            },
        ),
        Some(protocol::PermissionMode::AutoApproveCommands) => (
            IconName::ShieldAlert,
            if semantic_colors_overlap {
                auto_seed
            } else {
                colors.warning
            },
        ),
        Some(protocol::PermissionMode::FullAccess) => (IconName::ShieldAlert, colors.danger),
        None => (IconName::Shield, colors.muted_foreground),
    };
    if mode.is_some() {
        (icon, themes::readable_indicator_color(colors.popover, colors.foreground, color))
    } else {
        (icon, color)
    }
}

pub(super) fn processing_tier_badge(label: &str, cx: &App) -> AnyElement {
    let colors = cx.theme().colors;
    let ultrafast = label == "Ultrafast";
    div()
        .id("composer-model-speed")
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(rust_i18n::t!("app.composer.speed_aria", label = label).to_string())
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap_1()
        .px_2()
        .py(px(2.))
        .rounded_full()
        .bg(if ultrafast { colors.primary } else { colors.secondary })
        .text_color(if ultrafast {
            colors.primary_foreground
        } else {
            colors.foreground
        })
        .text_xs()
        .font_semibold()
        .child(Icon::new(IconName::Activity).size_3())
        .child(label.to_string())
        .into_any_element()
}

fn looks_like_file_reference(reference: &str) -> bool {
    if reference.is_empty() {
        return true;
    }
    if reference.contains('@') {
        return reference
            .rsplit_once('@')
            .is_some_and(|(_, suffix)| suffix.contains(|ch: char| ch.is_ascii_alphabetic()) && suffix.contains('.'));
    }
    true
}

pub(super) fn position_at(input: &str, byte_offset: usize) -> Position {
    let prefix = input.get(..byte_offset).unwrap_or(input);
    let line = prefix.matches('\n').count();
    let column = prefix.rsplit('\n').next().unwrap_or_default().encode_utf16().count();
    Position::new(u32::try_from(line).unwrap_or(u32::MAX), u32::try_from(column).unwrap_or(u32::MAX))
}

impl PaletteKind {
    pub(super) fn is_composer_popup(self) -> bool {
        matches!(
            self,
            Self::Models
                | Self::ModelOptions
                | Self::Permissions
                | Self::Modes
                | Self::AddContext
                | Self::FileMentions
                | Self::SlashCommands
                | Self::PromptHistory
        )
    }
}

pub(super) fn model_options_header(
    config: Option<&ConfigurationSnapshot>,
    effort_levels: &[String],
    slider: Option<&Entity<gpui_kit::component::slider::SliderState>>,
    focus: &FocusHandle,
    model_label: Option<&str>,
    speed_control: Option<AnyElement>,
    enabled: bool,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let options = config.and_then(|config| config.session_options.as_ref());
    let effort = options.map(|options| {
        slider
            .and_then(|slider| effort_at(effort_levels, slider.read(cx).value().start()))
            .unwrap_or(&options.reasoning_effort)
            .as_str()
    });
    let model_fallback = rust_i18n::t!(i18n::COMPOSER_CHOOSE_MODEL).to_string();
    let model = model_label
        .or_else(|| config.map(|config| config.model.as_str()))
        .unwrap_or(&model_fallback);
    let summary_label = format!("{model}{}{}", effort.map(|_| display::MIDDOT).unwrap_or(""), effort.unwrap_or(""));
    let mut summary = div()
        .id("model-options-summary")
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(summary_label)
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .flex_wrap()
        .gap_2()
        .child(
            div()
                .id("model-selected-name")
                .test_support()
                .role(gpui_kit::Role::Label)
                .aria_label(model)
                .min_w_0()
                .text_sm()
                .font_medium()
                .whitespace_normal()
                .child(model.to_string()),
        );
    if let Some(effort) = effort {
        summary = summary.child(
            div()
                .id("model-effort-value")
                .test_support()
                .role(gpui_kit::Role::Label)
                .aria_label(rust_i18n::t!("app.composer.effort_aria", effort = effort).to_string())
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(capitalize_agent_label(effort)),
        );
    }
    let mut panel = div().p_3().flex().flex_col().gap_2().min_w_0().child(summary);
    if let Some(slider) = slider {
        panel = panel.child(effort_knob(slider, focus, effort_levels.len(), effort, enabled, cx));
    }
    if let Some(speed_control) = speed_control {
        panel = panel.child(
            div()
                .id("model-fast-mode-row")
                .test_support()
                .flex()
                .justify_end()
                .child(speed_control),
        );
    }
    panel.into_any_element()
}

fn effort_knob(
    state: &Entity<gpui_kit::component::slider::SliderState>,
    focus: &FocusHandle,
    level_count: usize,
    effort: Option<&str>,
    enabled: bool,
    cx: &App,
) -> AnyElement {
    use gpui_kit::base::slider::{SliderIndicator, SliderThumb, SliderTrack};
    use gpui_kit::relative;

    let colors = cx.theme().colors;
    let slider = state.read(cx);
    let span = slider.max_value() - slider.min_value();
    let position = if span > 0. {
        (slider.value().start() - slider.min_value()) / span
    } else {
        0.
    };
    let position = position.clamp(0., 1.);
    let ticks = (0..level_count).map(|index| {
        let position = index as f32 / (level_count.saturating_sub(1).max(1) as f32);
        div()
            .id(("effort-stop", index))
            .test_support()
            .absolute()
            .left(relative(position))
            .top(px(13.))
            .ml(px(-3.))
            .size(px(6.))
            .rounded_full()
            .bg(colors.foreground.opacity(0.35))
    });

    let slider_id = state.entity_id();
    let increment = state.clone();
    let decrement = state.clone();
    let keyboard = state.clone();
    let release = state.clone();
    let release_outside = state.clone();
    let click_focus = focus.clone();
    div()
        .id(("slider", slider_id))
        .test_support()
        .role(gpui_kit::Role::Slider)
        .aria_label(
            effort
                .map(|effort| rust_i18n::t!(i18n::COMPOSER_REASONING_EFFORT, effort = effort).to_string())
                .unwrap_or_else(|| rust_i18n::t!("app.composer.reasoning_effort_generic").to_string()),
        )
        .aria_description(rust_i18n::t!("app.composer.reasoning_effort_hint").to_string())
        .aria_numeric_value(slider.value().start() as f64)
        .aria_min_numeric_value(slider.min_value() as f64)
        .aria_max_numeric_value(slider.max_value() as f64)
        .aria_numeric_value_step(slider.step_value() as f64)
        .aria_orientation(gpui_kit::Orientation::Horizontal)
        .track_focus(focus)
        .focus_visible(|style| style.border_1().border_color(colors.ring))
        .on_a11y_action(gpui_kit::AccessibleAction::Increment, move |_, window, cx| {
            if enabled {
                step_effort(&increment, 1., window, cx);
            }
        })
        .on_a11y_action(gpui_kit::AccessibleAction::Decrement, move |_, window, cx| {
            if enabled {
                step_effort(&decrement, -1., window, cx);
            }
        })
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers.control
                || event.keystroke.modifiers.alt
                || event.keystroke.modifiers.platform
                || event.keystroke.modifiers.function
            {
                return;
            }
            let direction = match event.keystroke.key.as_str() {
                "left" | "down" => -1.,
                "right" | "up" => 1.,
                _ => return,
            };
            cx.stop_propagation();
            if enabled {
                step_effort(&keyboard, direction, window, cx);
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
            release.update(cx, |state, cx| state.handle_release(cx));
        })
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
            release_outside.update(cx, |state, cx| state.handle_release(cx));
        })
        .when(enabled, |knob| knob.cursor_pointer())
        .when(!enabled, |knob| knob.opacity(0.5))
        .w_full()
        .min_w_0()
        .py_2()
        .child(
            SliderTrack::new(state)
                .w_full()
                .h(px(32.))
                .px(px(18.))
                .rounded_full()
                .bg(colors.foreground.opacity(0.12))
                .child(
                    SliderIndicator::new(state)
                        .relative()
                        .w_full()
                        .h_full()
                        .child(
                            div()
                                .absolute()
                                .left(px(-18.))
                                .right(relative(1. - position))
                                .h_full()
                                .rounded_l_full()
                                .bg(colors.primary),
                        )
                        .children(ticks)
                        .child(
                            SliderThumb::new(state)
                                .absolute()
                                .left(relative(position))
                                .top(px(-2.))
                                .ml(px(-18.))
                                .size(px(36.))
                                .rounded_full()
                                .bg(gpui_kit::white())
                                .shadow_md(),
                        ),
                ),
        )
        .into_any_element()
}

fn step_effort(
    slider: &Entity<gpui_kit::component::slider::SliderState>,
    direction: f32,
    window: &mut Window,
    cx: &mut App,
) {
    slider.update(cx, |state, cx| {
        let current = state.value().start();
        let value = (current + direction).clamp(state.min_value(), state.max_value());
        if (value - current).abs() < f32::EPSILON {
            return;
        }
        state.set_value(value, window, cx);
        cx.emit(gpui_kit::component::slider::SliderEvent::Release(state.value()));
    });
}

// Match the small capability list directly: no float-to-index cast, saturation, or panic.
pub(super) fn effort_at(levels: &[String], value: f32) -> Option<&String> {
    if !value.is_finite() || value < 0. {
        return None;
    }
    let selected = value.round();
    levels
        .iter()
        .enumerate()
        .find_map(|(index, level)| ((index as f32 - selected).abs() < f32::EPSILON).then_some(level))
}

#[cfg(test)]
mod tests {
    use gpui_kit::assets::IconName;

    use super::settings::SettingsTab;
    use super::{
        ComposerTriggerKind, DesktopSlashAction, capitalize_agent_label, composer_trigger, desktop_slash_action,
        effort_at, file_mention_token, permission_indicator, settings_tab_for_name,
    };

    #[test]
    fn composer_triggers_match_cli_file_and_slash_boundaries() {
        let input = "Inspect @src/main.rs";
        let mention = composer_trigger(input, input.len()).expect("file mention trigger");
        assert_eq!(mention.kind, ComposerTriggerKind::FileMention);
        assert_eq!(input.get(mention.range).expect("valid file mention range"), "@src/main.rs");
        assert_eq!(mention.query, "src/main.rs");

        let input = "/rev";
        let command = composer_trigger(input, input.len()).expect("slash command trigger");
        assert_eq!(command.kind, ComposerTriggerKind::SlashCommand);
        assert_eq!(input.get(command.range).expect("valid slash command range"), "/rev");
        assert_eq!(command.query, "rev");

        let input = "/review --last-diff";
        assert!(composer_trigger(input, input.len()).is_none(), "arguments end slash completion");
    }

    #[test]
    fn composer_triggers_ignore_package_scopes_and_inline_email_addresses() {
        for input in [
            "npm install @types/node",
            "npm install @vitejs/plugin-react",
            "pnpm add @fontsource/inter",
            "Send to dev@example.com",
            "prefix@src/main.rs",
        ] {
            assert!(composer_trigger(input, input.len()).is_none(), "unexpected trigger for {input:?}");
        }

        for input in [
            "npm install && inspect @src/main.rs",
            "Package npm is installed; inspect @src/main.rs",
            "Use pnpm for this workspace, then inspect @src/main.rs",
        ] {
            assert!(
                composer_trigger(input, input.len())
                    .is_some_and(|trigger| trigger.kind == ComposerTriggerKind::FileMention),
                "package-manager words outside the active command must not suppress {input:?}"
            );
        }
    }

    #[test]
    fn settings_deep_links_cover_every_settings_tab() {
        for (name, expected) in [
            ("appearance", SettingsTab::Appearance),
            ("theme", SettingsTab::Appearance),
            ("accessibility", SettingsTab::Accessibility),
            ("a11y", SettingsTab::Accessibility),
            ("shortcuts", SettingsTab::Shortcuts),
            ("keybindings", SettingsTab::Shortcuts),
            ("vtcode", SettingsTab::VtCode),
            ("general", SettingsTab::VtCode),
            ("connections", SettingsTab::Connections),
            ("mcp", SettingsTab::Connections),
            ("webmcp", SettingsTab::Connections),
            ("notifications", SettingsTab::Notifications),
            ("notification", SettingsTab::Notifications),
            ("alerts", SettingsTab::Notifications),
            ("updates", SettingsTab::Notifications),
        ] {
            assert_eq!(settings_tab_for_name(name), Some(expected), "alias {name:?}");
        }
        assert_eq!(settings_tab_for_name("unknown"), None);
    }

    #[test]
    fn agent_labels_capitalize_cli_names_without_changing_their_identity() {
        for (name, label) in [("auto", "Auto"), ("build", "Build"), ("plan", "Plan"), ("duck", "Duck")] {
            assert_eq!(capitalize_agent_label(name), label);
        }
        assert_eq!(capitalize_agent_label("custom reviewer"), "Custom reviewer");
        assert_eq!(capitalize_agent_label(""), "");
    }

    #[test]
    fn permission_indicators_distinguish_restrictive_and_auto_approval_states() {
        for palette in super::super::themes::all_themes() {
            let colors = gpui_kit::component::theme::ThemeColor {
                foreground: palette.foreground,
                popover: palette.background,
                success: palette.success,
                warning: palette.warning,
                ..Default::default()
            };
            let (ask_icon, ask_color) = permission_indicator(Some(super::protocol::PermissionMode::Ask), colors);
            let (auto_icon, auto_color) =
                permission_indicator(Some(super::protocol::PermissionMode::AutoApproveCommands), colors);

            assert_eq!(ask_icon, IconName::ShieldCheck, "{} uses a protected state icon", palette.id);
            assert_eq!(auto_icon, IconName::ShieldAlert, "{} marks auto approval as elevated", palette.id);
            assert!(
                super::super::themes::contrast_ratio(ask_color, palette.background) >= 4.5,
                "{} Ask indicator must meet AA contrast",
                palette.id
            );
            assert!(
                super::super::themes::contrast_ratio(auto_color, palette.background) >= 4.5,
                "{} Auto indicator must meet AA contrast",
                palette.id
            );
            assert_ne!(ask_color, auto_color, "{} permission states need distinct colors", palette.id);
        }

        let unavailable_colors = gpui_kit::component::theme::ThemeColor::default();
        let (unavailable_icon, unavailable_color) = permission_indicator(None, unavailable_colors);
        assert_eq!(unavailable_icon, IconName::Shield);
        assert_eq!(unavailable_color, unavailable_colors.muted_foreground);
    }

    #[test]
    fn composer_trigger_handles_unicode_text_and_rejects_non_boundary_cursors() {
        let input = "检查 @src/main.rs";
        let trigger = composer_trigger(input, input.len()).expect("unicode prefix does not break file trigger");
        assert_eq!(trigger.query, "src/main.rs");
        assert!(composer_trigger("é/review", 1).is_none(), "a cursor inside a UTF-8 code point is invalid");
    }

    #[test]
    fn desktop_slash_actions_route_native_equivalents_and_config_deep_links() {
        assert_eq!(desktop_slash_action(" /new "), Some(DesktopSlashAction::NewSession));
        assert_eq!(desktop_slash_action("/exit"), Some(DesktopSlashAction::CloseSession));
        assert_eq!(desktop_slash_action("/stop"), Some(DesktopSlashAction::StopTurn));
        assert_eq!(desktop_slash_action("/models"), Some(DesktopSlashAction::ModelPicker));
        assert_eq!(desktop_slash_action("/history"), Some(DesktopSlashAction::History));
        assert_eq!(desktop_slash_action("/splash"), Some(DesktopSlashAction::Splash));
        assert_eq!(desktop_slash_action("/config"), Some(DesktopSlashAction::Configuration(None)));
        assert_eq!(desktop_slash_action("/settings"), Some(DesktopSlashAction::Settings(None)));
        assert_eq!(desktop_slash_action("/config permissions"), Some(DesktopSlashAction::Permissions));
        assert_eq!(
            desktop_slash_action("/config appearance"),
            Some(DesktopSlashAction::Settings(Some(SettingsTab::Appearance,)))
        );
        assert_eq!(
            desktop_slash_action("/config agent.default_model"),
            Some(DesktopSlashAction::Configuration(Some("agent.default_model".into())))
        );
        assert_eq!(
            desktop_slash_action("/config   Tools  "),
            Some(DesktopSlashAction::Configuration(Some("Tools".into())))
        );
        assert_eq!(desktop_slash_action("/files src/ui"), Some(DesktopSlashAction::Files("src/ui".into())));
        assert_eq!(desktop_slash_action("/fast"), Some(DesktopSlashAction::FastMode(None)));
        assert_eq!(desktop_slash_action("/fast off"), Some(DesktopSlashAction::FastMode(Some(false))));
        assert_eq!(desktop_slash_action("/theme ciapre"), Some(DesktopSlashAction::Theme(Some("ciapre".into()))));
        assert_eq!(desktop_slash_action("/theme"), Some(DesktopSlashAction::Theme(None)));

        for input in [
            "/fast maybe",
            "/config reset",
            "/config reset --global",
            "/config Reset",
            "/config clear",
            "/config checkup",
            "/config checkup --quick",
            "/config CHECKUP",
            "/config doctor",
            "/config memory",
            "/config Memory",
            "/config agent.persistent_memory",
            "/config tasks",
            "/config jobs",
            "/config log",
            "/config notify",
            "/config subprocess",
            "/config subprocesses",
            "/settings reset",
            "/settings agent.default_model",
            "/splash now",
            "/files\nother",
        ] {
            assert_eq!(desktop_slash_action(input), None, "{input:?} stays with the CLI runtime");
        }
    }

    #[test]
    fn desktop_go_routes_deeplinks_and_storage_alias() {
        assert_eq!(
            desktop_slash_action("/go settings/shortcuts"),
            Some(DesktopSlashAction::Go("settings/shortcuts".into()))
        );
        assert_eq!(
            desktop_slash_action("/GO COMMAND/ZEN_MODE"),
            Some(DesktopSlashAction::Go("COMMAND/ZEN_MODE".into()))
        );
        assert_eq!(desktop_slash_action("/go"), Some(DesktopSlashAction::Go(String::new())));
        // /go always routes natively (even unknown links, which report a
        // desktop status error) so garbage never falls through headless.
        assert_eq!(desktop_slash_action("/go nope"), Some(DesktopSlashAction::Go("nope".into())));
        // Storage joins the known settings tabs for every deep-link surface.
        assert_eq!(
            desktop_slash_action("/settings storage"),
            Some(DesktopSlashAction::Settings(Some(SettingsTab::Storage)))
        );
        assert_eq!(
            desktop_slash_action("/config disk"),
            Some(DesktopSlashAction::Settings(Some(SettingsTab::Storage)))
        );
    }

    #[test]
    fn desktop_slash_actions_match_cli_case_insensitivity() {
        // The CLI lowercases the command token before dispatch; a capitalized
        // native command must route natively instead of falling through to the
        // headless service (where `/NEW` would end the service-side session
        // while this client keeps showing it as selected).
        for (input, expected) in [
            ("/NEW", DesktopSlashAction::NewSession),
            ("/New", DesktopSlashAction::NewSession),
            ("/EXIT", DesktopSlashAction::CloseSession),
            ("/Stop", DesktopSlashAction::StopTurn),
            ("/MODEL", DesktopSlashAction::ModelPicker),
            ("/Models", DesktopSlashAction::ModelPicker),
            ("/HISTORY", DesktopSlashAction::History),
            ("/Splash", DesktopSlashAction::Splash),
            ("/HELP", DesktopSlashAction::Help),
            ("/Mode", DesktopSlashAction::Modes),
        ] {
            assert_eq!(desktop_slash_action(input), Some(expected), "{input:?} routes natively");
        }
        // Keyword arguments follow the CLI's case-insensitive matching, while
        // free-form values keep their original case for downstream resolvers.
        assert_eq!(desktop_slash_action("/FAST ON"), Some(DesktopSlashAction::FastMode(Some(true))));
        assert_eq!(desktop_slash_action("/Fast Off"), Some(DesktopSlashAction::FastMode(Some(false))));
        assert_eq!(desktop_slash_action("/CONFIG PERMISSIONS"), Some(DesktopSlashAction::Permissions));
        assert_eq!(
            desktop_slash_action("/Config Appearance"),
            Some(DesktopSlashAction::Settings(Some(SettingsTab::Appearance,)))
        );
        assert_eq!(desktop_slash_action("/SETTINGS Model"), Some(DesktopSlashAction::ModelPicker));
        assert_eq!(desktop_slash_action("/THEME Ciapre"), Some(DesktopSlashAction::Theme(Some("Ciapre".into()))));
        assert_eq!(desktop_slash_action("/FILES SRC/UI"), Some(DesktopSlashAction::Files("SRC/UI".into())));
        // Commands whose CLI backend ignores arguments route natively regardless
        // of trailing text, in any case: falling through would hit
        // headless-unsafe handlers (session teardown, picker parking).
        for (input, expected) in [
            ("/new please", DesktopSlashAction::NewSession),
            ("/NEW please", DesktopSlashAction::NewSession),
            ("/exit now", DesktopSlashAction::CloseSession),
            ("/EXIT now", DesktopSlashAction::CloseSession),
            ("/stop now", DesktopSlashAction::StopTurn),
            ("/STOP now", DesktopSlashAction::StopTurn),
            ("/model foo", DesktopSlashAction::ModelPicker),
            ("/MODEL foo bar", DesktopSlashAction::ModelPicker),
        ] {
            assert_eq!(desktop_slash_action(input), Some(expected), "{input:?} routes natively");
        }
        // Guards still apply to capitalized input: unknown commands and
        // runtime-owned `/config` sections stay with the CLI.
        for input in [
            "/NEWS",
            "/FAST maybe",
            "/CONFIG RESET",
            "/Settings reset",
            "/history please",
        ] {
            assert_eq!(desktop_slash_action(input), None, "{input:?} stays with the CLI runtime");
        }
    }

    #[test]
    fn file_mentions_quote_paths_for_the_cli_reference_parser() {
        assert_eq!(file_mention_token("src/main.rs").as_deref(), Some("@src/main.rs"));
        assert_eq!(file_mention_token("docs/guide notes.md").as_deref(), Some("@\"docs/guide notes.md\""));
        assert_eq!(file_mention_token("docs/guide's notes.md").as_deref(), Some("@\"docs/guide's notes.md\""));
        assert_eq!(file_mention_token("docs/guide\"s notes.md").as_deref(), Some("@'docs/guide\"s notes.md'"));
        assert!(file_mention_token("docs/a'\"b notes.md").is_none());
        assert!(file_mention_token("docs/line\nbreak.md").is_none());

        for path in [
            "src/main.rs",
            "docs/guide notes.md",
            "docs/guide's notes.md",
            "docs/guide\"s notes.md",
        ] {
            let mention = file_mention_token(path).expect("the path has representable CLI mention syntax");
            let parsed = vtcode_commons::at_pattern::find_at_patterns(&mention);
            assert_eq!(parsed.first().map(|reference| reference.path), Some(path));
        }
    }

    #[test]
    fn effort_knob_respects_discrete_capability_boundaries() {
        let levels = ["low".into(), "medium".into(), "high".into()];
        assert_eq!(effort_at(&levels, 0.49).map(String::as_str), Some("low"));
        assert_eq!(effort_at(&levels, 0.51).map(String::as_str), Some("medium"));
        assert_eq!(effort_at(&levels, 2.).map(String::as_str), Some("high"));
        for value in [-1., 3., f32::NAN, f32::INFINITY] {
            assert!(effort_at(&levels, value).is_none());
        }
    }
}
