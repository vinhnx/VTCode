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
    Cli,
    Desktop,
}

/// Settings-tab aliases shared by the `/config` and `/settings` deep-links
/// and the [`deeplinks`] system.
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
    // swallow later turns. `cli` and `desktop` are likewise unconditional:
    // the service has no `/cli` (fallthrough would submit it as a prompt)
    // and its `/desktop` would spawn a second desktop instance.
    // Every remaining fallthrough below is headless-safe
    // (pure text, usage errors, or text-driven selection).
    let command = name.to_ascii_lowercase();
    let keyword = args.to_ascii_lowercase();
    match command.as_str() {
        "new" => Some(DesktopSlashAction::NewSession),
        "exit" => Some(DesktopSlashAction::CloseSession),
        "stop" => Some(DesktopSlashAction::StopTurn),
        "model" | "models" => Some(DesktopSlashAction::ModelPicker),
        "cli" => Some(DesktopSlashAction::Cli),
        "desktop" => Some(DesktopSlashAction::Desktop),
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
        // Zero or one theme name: no argument opens the picker, exactly one
        // routes natively, and anything longer falls through to the service
        // (headless-safe, same as before). Reading two tokens off the
        // iterator keeps that rule without materializing a Vec just to match
        // on its slice.
        "theme" => {
            let mut words = args.split_whitespace();
            match (words.next(), words.next()) {
                (None, _) => Some(DesktopSlashAction::Theme(None)),
                (Some(theme), None) => Some(DesktopSlashAction::Theme(Some(theme.to_string()))),
                (Some(_), Some(_)) => None,
            }
        }
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

/// Quiet `· value` suffix for the model button: the reasoning effort always,
/// and the processing tier in Zen mode (where the filled badge would compete
/// with the send action). Same identity/role contract as the badge; muted
/// text instead of a filled pill.
pub(super) fn quiet_option_suffix(id: &'static str, accessible_label: String, value: &str, muted: Hsla) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(accessible_label)
        .flex_shrink_0()
        .text_color(muted)
        .child(format!("· {value}"))
        .into_any_element()
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
        panel = panel.child(effort_options(slider, focus, effort_levels, effort, enabled, cx));
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

/// One radio row per reasoning-effort level: discrete levels read as a
/// small visible set, not a continuous drag, so one click commits.
/// Selection pairs the primary indicator with the choice-card tint, hover
/// pairs the accent fill with foreground text like Kit menus, and rows
/// commit through the same SliderState channel the old rail used, so
/// release-to-commit, revert-on-blocked, and the palette lifecycle stay
/// in one place.
fn effort_options(
    state: &Entity<gpui_kit::component::slider::SliderState>,
    focus: &FocusHandle,
    levels: &[String],
    effort: Option<&str>,
    enabled: bool,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    let selected_index = effort.and_then(|effort| levels.iter().position(|level| level.as_str() == effort));
    let level_count = levels.len();
    // Highest effort first, matching the old bottom-to-top rail order.
    // Element ids stay on the levels index (not the visual row) so
    // existing queries keep working; only the announced set position
    // follows the visual order.
    let rows = levels.iter().enumerate().rev().map(|(index, level)| {
        let label = capitalize_agent_label(level);
        let target = state.clone();
        let selected = selected_index == Some(index);
        Radio::new(("effort-label", index))
            .label(label.clone())
            .accessibility_label(label)
            .checked(selected)
            .disabled(!enabled)
            .small()
            .w_full()
            .py_1()
            .px_2()
            .rounded_md()
            .when(selected, |row| {
                row.bg(colors.primary.opacity(0.08))
                    .font_semibold()
                    .text_color(colors.foreground)
            })
            .when(!selected, |row| row.text_color(colors.muted_foreground))
            .when(enabled && !selected, |row| row.hover(|hover| hover.bg(colors.accent).text_color(colors.foreground)))
            .when(enabled, |row| row.cursor_pointer())
            .aria_position_in_set(level_count - index)
            .aria_size_of_set(level_count)
            .on_change(move |checked: &bool, window, cx| {
                if *checked {
                    settle_effort(&target, index as f32, window, cx);
                }
            })
    });
    let keyboard = state.clone();
    let click_focus = focus.clone();
    div()
        .id("effort-options")
        .test_support()
        .role(gpui_kit::Role::RadioGroup)
        .aria_label(
            effort
                .map(|effort| rust_i18n::t!(i18n::COMPOSER_REASONING_EFFORT, effort = effort).to_string())
                .unwrap_or_else(|| rust_i18n::t!("app.composer.reasoning_effort_generic").to_string()),
        )
        .aria_description(rust_i18n::t!("app.composer.reasoning_effort_hint").to_string())
        .track_focus(focus)
        .focus_visible(|style| style.border_1().border_color(colors.ring))
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers.control
                || event.keystroke.modifiers.alt
                || event.keystroke.modifiers.platform
                || event.keystroke.modifiers.function
            {
                return;
            }
            let key = event.keystroke.key.as_str();
            if !matches!(key, "left" | "down" | "right" | "up" | "home" | "end") {
                return;
            }
            cx.stop_propagation();
            if !enabled {
                return;
            }
            match key {
                "left" | "down" => step_effort(&keyboard, -1., window, cx),
                "right" | "up" => step_effort(&keyboard, 1., window, cx),
                "home" => jump_effort(&keyboard, false, window, cx),
                "end" => jump_effort(&keyboard, true, window, cx),
                _ => {}
            }
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
        .when(!enabled, |options| options.opacity(0.5))
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .children(rows)
        .into_any_element()
}

fn settle_effort(
    slider: &Entity<gpui_kit::component::slider::SliderState>,
    value: f32,
    window: &mut Window,
    cx: &mut App,
) {
    slider.update(cx, |state, cx| {
        let value = value.clamp(state.min_value(), state.max_value());
        if (value - state.value().start()).abs() < f32::EPSILON {
            return;
        }
        state.set_value(value, window, cx);
        cx.emit(gpui_kit::component::slider::SliderEvent::Release(state.value()));
    });
}

fn step_effort(
    slider: &Entity<gpui_kit::component::slider::SliderState>,
    direction: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let current = slider.read(cx).value().start();
    settle_effort(slider, current + direction, window, cx);
}

fn jump_effort(
    slider: &Entity<gpui_kit::component::slider::SliderState>,
    to_max: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let limit = slider.read(cx);
    let limit = if to_max { limit.max_value() } else { limit.min_value() };
    settle_effort(slider, limit, window, cx);
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
mod tests;
