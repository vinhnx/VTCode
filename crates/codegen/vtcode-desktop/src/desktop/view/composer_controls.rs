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
    Configuration,
    Permissions,
    Modes,
    Files(String),
    FastMode(Option<bool>),
    Theme(Option<String>),
    Splash,
    Help,
}

/// Commands with an exact native Desktop equivalent; all other slash input
/// remains owned by the shared CLI dispatcher.
pub(super) fn desktop_slash_action(input: &str) -> Option<DesktopSlashAction> {
    if input.chars().any(char::is_control) {
        return None;
    }
    let body = input.trim().strip_prefix('/')?;
    let (name, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    let args = args.trim();
    match name {
        "new" if args.is_empty() => Some(DesktopSlashAction::NewSession),
        "exit" if args.is_empty() => Some(DesktopSlashAction::CloseSession),
        "stop" if args.is_empty() => Some(DesktopSlashAction::StopTurn),
        "model" | "models" if args.is_empty() => Some(DesktopSlashAction::ModelPicker),
        "history" if args.is_empty() => Some(DesktopSlashAction::History),
        "splash" if args.is_empty() => Some(DesktopSlashAction::Splash),
        "help" if args.is_empty() => Some(DesktopSlashAction::Help),
        "mode" if args.is_empty() => Some(DesktopSlashAction::Modes),
        "files" => Some(DesktopSlashAction::Files(args.to_string())),
        "fast" => match args {
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
        "config" if args.is_empty() => Some(DesktopSlashAction::Configuration),
        "settings" if args.is_empty() => Some(DesktopSlashAction::Settings(None)),
        "config" | "settings" => {
            let settings_tab = match args {
                "appearance" | "theme" => Some(Some(settings::SettingsTab::Appearance)),
                "accessibility" | "a11y" => Some(Some(settings::SettingsTab::Accessibility)),
                "shortcuts" | "keybindings" => Some(Some(settings::SettingsTab::Shortcuts)),
                "vtcode" | "general" => Some(Some(settings::SettingsTab::VtCode)),
                "connections" | "mcp" | "webmcp" => Some(Some(settings::SettingsTab::Connections)),
                "model" | "models" => return Some(DesktopSlashAction::ModelPicker),
                "permissions" | "permission" => return Some(DesktopSlashAction::Permissions),
                _ => None,
            }?;
            Some(DesktopSlashAction::Settings(settings_tab))
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

fn capitalize_agent_label(label: &str) -> String {
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
    let (icon, color) = match mode {
        Some(protocol::PermissionMode::Ask) => (
            IconName::ShieldCheck,
            if semantic_colors_overlap {
                gpui_kit::rgb(0x237A45).into()
            } else {
                colors.success
            },
        ),
        Some(protocol::PermissionMode::AutoApproveCommands) => (
            IconName::ShieldAlert,
            if semantic_colors_overlap {
                gpui_kit::rgb(0xB45309).into()
            } else {
                colors.warning
            },
        ),
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
        .aria_label(format!("Processing speed: {label}"))
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

fn position_at(input: &str, byte_offset: usize) -> Position {
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

impl DesktopView {
    fn clear_submitted_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |input, cx| input.set_value("", window, cx));
    }

    pub(super) fn handle_desktop_slash_action(
        &mut self,
        action: DesktopSlashAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            DesktopSlashAction::StopTurn => {
                self.clear_submitted_command(window, cx);
                if self.running {
                    self.cancel_turn(cx);
                } else {
                    self.status = "No active turn to stop.".into();
                    cx.notify();
                }
                true
            }
            DesktopSlashAction::NewSession => {
                if self.begin_new_thread(cx) {
                    self.clear_submitted_command(window, cx);
                }
                true
            }
            DesktopSlashAction::CloseSession => {
                if let Some(session_id) = self.selected_session_id.clone() {
                    if self.request_session_change(
                        protocol::method::SESSION_CLOSE,
                        json!({ "sessionId": session_id }),
                        cx,
                    ) {
                        self.clear_submitted_command(window, cx);
                    }
                } else {
                    self.status = "No active thread to close.".into();
                    self.clear_submitted_command(window, cx);
                    cx.notify();
                }
                true
            }
            DesktopSlashAction::ModelPicker => {
                if !self.session_change_allowed(None) || self.attachments_picker_open {
                    return false;
                }
                self.clear_submitted_command(window, cx);
                self.open_model_palette(window, cx);
                true
            }
            DesktopSlashAction::History => {
                self.clear_submitted_command(window, cx);
                self.open_prompt_history(window, cx);
                true
            }
            DesktopSlashAction::Configuration => {
                self.clear_submitted_command(window, cx);
                if self.settings_open {
                    self.close_settings(window, cx);
                }
                self.config_page_open = true;
                self.open_settings(window, cx);
                true
            }
            DesktopSlashAction::Settings(tab) => {
                if self.palette.is_some() {
                    self.close_palette(window, cx);
                }
                self.settings_tab = tab.unwrap_or(settings::SettingsTab::Appearance);
                self.clear_submitted_command(window, cx);
                self.open_settings(window, cx);
                true
            }
            DesktopSlashAction::Permissions => {
                if !self.can_select_session_options() {
                    return false;
                }
                self.clear_submitted_command(window, cx);
                self.open_permissions(window, cx);
                true
            }
            DesktopSlashAction::Modes => {
                if !self.can_select_session_options() {
                    return false;
                }
                self.clear_submitted_command(window, cx);
                self.open_modes(window, cx);
                true
            }
            DesktopSlashAction::Files(filter) => {
                if self.workspace.is_none() {
                    self.error = Some("Choose a workspace before browsing files.".into());
                    cx.notify();
                    return true;
                }
                self.composer
                    .update(cx, |input, cx| input.set_value(format!("@{filter}"), window, cx));
                if self.files.is_empty() {
                    self.refresh_files(cx);
                }
                self.update_composer_suggestions(window, cx);
                true
            }
            DesktopSlashAction::FastMode(enable) => {
                let Some(options) = self
                    .configuration
                    .as_ref()
                    .and_then(|configuration| configuration.session_options.as_ref())
                    .filter(|options| options.fast_mode_supported)
                else {
                    return false;
                };
                let enable = enable.unwrap_or(!options.fast_mode_enabled);
                if self.select_session_options(
                    protocol::SessionOptionsSelectParams { fast_mode: Some(enable), ..Default::default() },
                    cx,
                ) {
                    self.clear_submitted_command(window, cx);
                }
                true
            }
            DesktopSlashAction::Theme(theme) => {
                if let Some(theme) = theme {
                    if themes::find(&theme).is_none() {
                        self.clear_submitted_command(window, cx);
                        self.error =
                            Some(format!("Theme '{theme}' is not available. Choose a theme from Appearance settings."));
                        cx.notify();
                        return true;
                    }
                    self.clear_submitted_command(window, cx);
                    self.apply_theme(theme, window, cx);
                    self.sync_theme_to_cli(cx);
                } else {
                    self.settings_tab = settings::SettingsTab::Appearance;
                    self.clear_submitted_command(window, cx);
                    self.open_settings(window, cx);
                }
                true
            }
            DesktopSlashAction::Help => {
                self.clear_submitted_command(window, cx);
                self.open_command_palette(window, cx);
                true
            }
            DesktopSlashAction::Splash => {
                self.clear_submitted_command(window, cx);
                self.open_webmcp_settings(window, cx);
                true
            }
        }
    }

    pub(super) fn render_workspace_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.workspace.as_ref().map(|path| path.display().to_string());
        let current_label = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| rust_i18n::t!("app.workspace.choose").to_string());
        let recent = self.recent_workspaces();
        let disabled =
            !self.workspace_switch_idle() || self.retained_submission.is_some() || self.workspace_picker_open;
        let view = cx.entity().downgrade();
        let trigger = desktop_button("composer-workspace")
            .icon(IconName::Folder)
            .label(current_label)
            .ghost()
            .small()
            .max_w(px(240.))
            .overflow_hidden()
            .accessibility_label(rust_i18n::t!("app.workspace.choose").to_string())
            .tooltip(rust_i18n::t!("app.workspace.choose_or_recent").to_string())
            .desktop_disabled(disabled);

        Popover::new("composer-workspace-popover")
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .w(px(300.))
            .max_h(px(360.))
            .trigger(trigger)
            .content(move |_, _, cx| {
                let colors = cx.theme().colors;
                let mut content = div()
                    .id("workspace-picker-list")
                    .w_full()
                    .min_h_0()
                    .max_h(px(340.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(colors.muted_foreground)
                            .child(rust_i18n::t!("app.workspace.section").to_string().to_uppercase()),
                    );
                let open_view = view.clone();
                content = content.child(
                    desktop_button("open-workspace-folder")
                        .icon(IconName::Folder)
                        .label(rust_i18n::t!("app.workspace.choose_folder").to_string())
                        .ghost()
                        .w_full()
                        .desktop_disabled(disabled)
                        .on_click(cx.listener(move |popover, _, window, cx| {
                            popover.dismiss(window, cx);
                            let _updated = open_view.update(cx, |view, cx| view.choose_workspace(window, cx));
                        })),
                );
                if recent.is_empty() {
                    content = content.child(
                        div()
                            .px_2()
                            .py_2()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(rust_i18n::t!("app.workspace.no_recent_workspaces").to_string()),
                    );
                } else {
                    for path in &recent {
                        let label = Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(path)
                            .to_string();
                        let selected = current.as_deref() == Some(path.as_str());
                        let entry_view = view.clone();
                        let entry_path = path.clone();
                        content = content.child(
                            desktop_button(opaque_path_element_id("recent-workspace-popover", path))
                                .accessibility_label(if selected {
                                    rust_i18n::t!("app.workspace.current_label", workspace = label).to_string()
                                } else {
                                    label.clone()
                                })
                                .child(navigation_label(label, IconName::Folder, selected, colors.primary))
                                .ghost()
                                .w_full()
                                .desktop_disabled(disabled)
                                .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                                .on_click(cx.listener(move |popover, _, window, cx| {
                                    popover.dismiss(window, cx);
                                    let path = PathBuf::from(entry_path.clone());
                                    let _updated = entry_view.update(cx, |view, cx| {
                                        view.select_workspace_from_ui(path, window, cx);
                                    });
                                })),
                        );
                    }
                }
                content.into_any_element()
            })
            .into_any_element()
    }

    pub(super) fn open_add_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() || self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        self.show_palette(PaletteKind::AddContext, Vec::new(), window, cx);
        if !self.resources_loaded && !self.resources_loading && self.session_change_idle() {
            self.resources_loading = self.request(protocol::method::COMPOSER_RESOURCES, json!({}));
            if !self.resources_loading {
                self.error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn open_prompt_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = HashSet::new();
        let entries = self
            .transcript
            .items_newest_first()
            .filter(|item| item.role == "user")
            .filter_map(|item| {
                let prompt = item.text.to_string();
                if prompt.trim().is_empty() || !seen.insert(prompt.clone()) {
                    return None;
                }
                Some(command(session_title_from_prompt(&prompt), PaletteAction::InsertHistoryPrompt(prompt)))
            })
            .collect();
        self.show_palette(
            PaletteKind::PromptHistory,
            vec![(rust_i18n::t!("app.palette.sections.this_thread").to_string(), entries)],
            window,
            cx,
        );
    }

    pub(super) fn composer_resource_groups(&self) -> Vec<(String, Vec<PaletteCommand>)> {
        let mut groups = vec![(
            rust_i18n::t!("app.palette.sections.add").to_string(),
            vec![command(
                rust_i18n::t!("app.palette.command.add_files").to_string(),
                PaletteAction::AttachFiles,
            )],
        )];
        let files = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| command(entry.rel_path.clone(), PaletteAction::AttachFile(entry.rel_path.clone())))
            .collect::<Vec<_>>();
        // Preserve section identities while asynchronous discovery fills empty groups.
        groups.push((rust_i18n::t!("app.palette.sections.workspace_files").to_string(), files));
        for (kind, title) in [
            (protocol::ComposerResourceKind::Skill, rust_i18n::t!("app.palette.sections.skills").to_string()),
            (protocol::ComposerResourceKind::Plugin, rust_i18n::t!("app.palette.sections.plugins").to_string()),
        ] {
            let entries = self
                .composer_resources
                .iter()
                .filter(|resource| resource.kind == kind)
                .map(|resource| {
                    let text = match kind {
                        protocol::ComposerResourceKind::Skill => format!("${}", resource.name),
                        protocol::ComposerResourceKind::Plugin => {
                            rust_i18n::t!("app.palette.command.use_plugin", plugin = resource.name.as_str()).to_string()
                        }
                    };
                    let mut entry = command(resource.name.clone(), PaletteAction::InsertDraft(text));
                    entry.description = Some(resource.description.clone());
                    entry
                })
                .collect::<Vec<_>>();
            groups.push((title, entries));
        }
        groups.push((
            rust_i18n::t!("app.palette.sections.commands").to_string(),
            vec![
                command(rust_i18n::t!("app.palette.command.choose_model").to_string(), PaletteAction::ChooseModel),
                command(
                    rust_i18n::t!("app.palette.command.switch_workspace").to_string(),
                    PaletteAction::ChooseWorkspace,
                ),
                command(
                    rust_i18n::t!("app.palette.command.toggle_inspector").to_string(),
                    PaletteAction::ToggleInspector,
                ),
                command(rust_i18n::t!("app.files.refresh_tooltip").to_string(), PaletteAction::RefreshFiles),
            ],
        ));
        let slash_commands = self
            .slash_commands
            .iter()
            .map(|entry| {
                let mut command =
                    command(format!("/{}", entry.name), PaletteAction::CompleteSlashCommand(entry.name.clone()));
                command.description = Some(entry.description.clone());
                command.keywords = vec![entry.name.clone(), format!("/{}", entry.name)];
                command
            })
            .collect();
        groups.push((rust_i18n::t!("app.palette.sections.vtcode_commands").to_string(), slash_commands));
        groups
    }

    pub(super) fn update_composer_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.composer.read(cx).value();
        let cursor = self.composer.read(cx).cursor();
        let Some(trigger) = composer_trigger(&value, cursor) else {
            if matches!(self.palette_kind, PaletteKind::FileMentions | PaletteKind::SlashCommands) {
                self.close_palette(window, cx);
            }
            return;
        };

        let query_changed = self.palette_query.as_ref() != trigger.query.as_str();
        let query = trigger.query;
        let (kind, groups) = match trigger.kind {
            ComposerTriggerKind::FileMention => {
                let needle = query.to_lowercase();
                let entries = self
                    .files
                    .iter()
                    .filter_map(|entry| {
                        if entry.is_dir
                            || entry.rel_path.chars().any(char::is_control)
                            || !entry.rel_path.to_lowercase().contains(&needle)
                        {
                            return None;
                        }
                        let mention = file_mention_token(&entry.rel_path)?;
                        let mut command = command(mention, PaletteAction::InsertFileMention(entry.rel_path.clone()));
                        command.description = Some(rust_i18n::t!("app.palette.command.workspace_file").to_string());
                        Some(command)
                    })
                    .take(48)
                    .collect();
                (PaletteKind::FileMentions, vec![("Files".into(), entries)])
            }
            ComposerTriggerKind::SlashCommand => {
                let needle = query.to_lowercase();
                let matching = self
                    .slash_commands
                    .iter()
                    .filter(|entry| {
                        needle.is_empty()
                            || entry.name.to_lowercase().starts_with(&needle)
                            || entry.name.to_lowercase().contains(&needle)
                            || entry.description.to_lowercase().contains(&needle)
                    })
                    .map(|entry| {
                        let mut command = command(
                            format!("/{}", entry.name),
                            PaletteAction::CompleteSlashCommand(entry.name.clone()),
                        );
                        command.description = Some(entry.description.clone());
                        command.keywords = vec![entry.name.clone(), format!("/{}", entry.name)];
                        command
                    })
                    .collect();
                (PaletteKind::SlashCommands, vec![("Commands".into(), matching)])
            }
        };

        self.palette_query = query.clone().into();
        if self.palette_kind == kind && self.palette.is_some() {
            self.palette_groups = share_palette_commands(groups);
            if query_changed {
                let first = self
                    .palette_groups
                    .first()
                    .is_some_and(|(_, entries)| !entries.is_empty())
                    .then(|| gpui_kit::component::IndexPath::new(0));
                if let Some(state) = self.palette.clone() {
                    state.update(cx, |state, cx| state.set_selected_index(first, window, cx));
                }
            }
            cx.notify();
        } else {
            self.show_palette(kind, groups, window, cx);
            self.palette_query = query.into();
        }
    }

    pub(super) fn move_composer_suggestion(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(
            self.palette_kind,
            PaletteKind::FileMentions | PaletteKind::SlashCommands | PaletteKind::PromptHistory
        ) {
            return false;
        }
        let Some(state) = self.palette.clone() else {
            return false;
        };
        let item_count = self.palette_groups.first().map_or(0, |(_, entries)| entries.len());
        if item_count == 0 {
            return false;
        }
        let current = state.read(cx).selected_index().map(|index| index.row).unwrap_or(0);
        let selected = if down {
            (current + 1) % item_count
        } else if current == 0 {
            item_count - 1
        } else {
            current - 1
        };
        state.update(cx, |state, cx| {
            state.set_selected_index(Some(gpui_kit::component::IndexPath::new(selected)), window, cx);
        });
        true
    }

    fn selected_composer_suggestion(&self, cx: &App) -> Option<PaletteAction> {
        let state = self.palette.as_ref()?;
        let selected = state.read(cx).selected_index()?;
        self.palette_groups
            .get(selected.section)?
            .1
            .get(selected.row)
            .map(|entry| entry.action.clone())
    }

    pub(super) fn accept_composer_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(
            self.palette_kind,
            PaletteKind::FileMentions | PaletteKind::SlashCommands | PaletteKind::PromptHistory
        ) {
            return false;
        }
        let Some(action) = self.selected_composer_suggestion(cx) else {
            return false;
        };
        self.run_palette_action(action, window, cx);
        true
    }

    pub(super) fn handle_composer_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.palette_kind {
            PaletteKind::FileMentions => {
                if !self.accept_composer_suggestion(window, cx) {
                    self.submit_prompt(window, cx);
                }
            }
            PaletteKind::PromptHistory => {
                let _accepted = self.accept_composer_suggestion(window, cx);
            }
            PaletteKind::SlashCommands => {
                let _accepted = self.accept_composer_suggestion(window, cx);
                self.submit_prompt(window, cx);
            }
            _ => self.submit_prompt(window, cx),
        }
    }

    pub(super) fn complete_composer_trigger(
        &mut self,
        kind: ComposerTriggerKind,
        token: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.composer.read(cx).value().to_string();
        let cursor = self.composer.read(cx).cursor();
        let active = composer_trigger(&current, cursor).filter(|trigger| trigger.kind == kind);
        if let Some(trigger) = active {
            let Some(suffix) = current.get(trigger.range.end..) else {
                return;
            };
            let needs_space = !suffix.is_empty() && !suffix.chars().next().is_some_and(char::is_whitespace);
            let insertion = if suffix.is_empty() || needs_space {
                format!("{token} ")
            } else {
                token
            };
            let mut updated = String::with_capacity(current.len() + insertion.len());
            let Some(prefix) = current.get(..trigger.range.start) else {
                return;
            };
            updated.push_str(prefix);
            updated.push_str(&insertion);
            updated.push_str(suffix);
            let cursor = trigger.range.start + insertion.len();
            let position = position_at(&updated, cursor);
            self.composer.update(cx, |input, cx| {
                input.replace_all(updated, window, cx);
                input.set_cursor_position(position, window, cx);
            });
        } else {
            let prefix = current.get(..cursor).unwrap_or(current.as_str());
            let suffix = current.get(cursor..).unwrap_or_default();
            let needs_prefix_space = !prefix.is_empty() && !prefix.chars().next_back().is_some_and(char::is_whitespace);
            let needs_suffix_space = !suffix.is_empty() && !suffix.chars().next().is_some_and(char::is_whitespace);
            let insertion = format!(
                "{}{token}{}",
                if needs_prefix_space { " " } else { "" },
                if suffix.is_empty() || needs_suffix_space {
                    " "
                } else {
                    ""
                }
            );
            self.composer.update(cx, |input, cx| input.replace(insertion, window, cx));
        }
        self.focus_composer(window, cx);
    }

    pub(super) fn refresh_composer_suggestions_after_input_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composer.read(cx).focus_handle(cx).is_focused(window) {
            cx.on_next_frame(window, |view, window, cx| view.update_composer_suggestions(window, cx));
        }
    }

    pub(super) fn attach_context_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        if self.attachments.len() >= 5 {
            self.error = Some("At most 5 attachments per prompt.".into());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        let session = self.selected_session_id.clone();
        self.attachments_picker_open = true;
        self.attachments_task = Some(cx.spawn(async move |view, cx| {
            let root = workspace.clone();
            let attachment = cx
                .background_spawn(async move { files::read_attachment(&root, &root.join(path)) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                view.attachments_picker_open = false;
                if view.workspace.as_ref() != Some(&workspace) || view.selected_session_id != session {
                    return;
                }
                match attachment {
                    Ok(attachment) if !view.attachments.iter().any(|existing| existing.path == attachment.path) => {
                        view.attachments.push(attachment)
                    }
                    Ok(_) => {}
                    Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn open_model_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        self.show_palette(PaletteKind::ModelOptions, Vec::new(), window, cx);
        let Some(config) = self.configuration.as_ref() else {
            return;
        };
        let Some(options) = config.session_options.as_ref() else {
            return;
        };
        let levels = options.supported_efforts.clone();
        if !(1..=8).contains(&levels.len()) {
            return;
        }
        let Some(selected) = levels.iter().position(|effort| effort == &options.reasoning_effort) else {
            // An inherited CLI value can be unsupported after a model switch. Offer
            // explicit valid choices without pretending that the knob is already set.
            let entries = levels
                .iter()
                .map(|effort| {
                    command(
                        format!("Use {effort} effort"),
                        PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                            reasoning_effort: Some(effort.clone()),
                            ..Default::default()
                        }),
                    )
                })
                .collect();
            self.palette_groups = share_palette_commands(vec![("Supported reasoning effort".into(), entries)]);
            cx.notify();
            return;
        };
        if levels.len() < 2 {
            return;
        }
        let provider = config.provider.clone();
        let model = config.model.clone();
        let slider = cx.new(|_| {
            gpui_kit::component::slider::SliderState::new()
                .min(0.)
                .max((levels.len() - 1) as f32)
                .step(1.)
                .default_value(selected as f32)
        });
        let palette_id = self.palette.as_ref().map(|state| state.entity_id());
        let slider_observation = cx.observe(&slider, |_, _, cx| cx.notify());
        let slider_events = cx.subscribe_in(&slider, window, move |_view, _, event, window, cx| {
            // The observer above invalidates the view when slider state changes;
            // this typed event only commits a released value to the CLI.
            if let gpui_kit::component::slider::SliderEvent::Release(value) = event {
                let Some(effort) = effort_at(&levels, value.start()).cloned() else {
                    return;
                };
                let provider = provider.clone();
                let model = model.clone();
                let view = cx.entity().downgrade();
                window.defer(cx, move |window, cx| {
                    let _updated = view.update(cx, |view, cx| {
                        if view.palette_kind != PaletteKind::ModelOptions
                            || view.palette.as_ref().map(|state| state.entity_id()) != palette_id
                            || !view.model_is_current(&provider, &model)
                        {
                            return;
                        }
                        // Keep the model options panel open so effort
                        // adjustments do not dismiss the knob.
                        let params = protocol::SessionOptionsSelectParams {
                            reasoning_effort: Some(effort),
                            ..Default::default()
                        };
                        let selected = view.select_session_options(params.clone(), cx);
                        if !selected && !view.session_option_is_current(&params) {
                            // Blocked by a pending request. Revert the knob to
                            // the CLI value so the panel never shows an
                            // uncommitted level as selected.
                            let current = view
                                .configuration
                                .as_ref()
                                .and_then(|config| config.session_options.as_ref())
                                .and_then(|options| {
                                    options
                                        .supported_efforts
                                        .iter()
                                        .position(|level| level == &options.reasoning_effort)
                                });
                            if let Some(index) = current
                                && let Some(slider) = view.effort_slider.clone()
                            {
                                slider.update(cx, |state, cx| state.set_value(index as f32, window, cx));
                            }
                        }
                    });
                });
            }
        });
        self.effort_slider = Some(slider);
        self._effort_slider_subscriptions = vec![slider_observation, slider_events];
        cx.notify();
    }

    pub(super) fn can_select_session_options(&self) -> bool {
        self.session_change_allowed(None)
            && !self.attachments_picker_open
            && self
                .configuration
                .as_ref()
                .is_some_and(|config| config.session_options.is_some())
    }

    pub(super) fn select_session_options(
        &mut self,
        options: protocol::SessionOptionsSelectParams,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_select_session_options() {
            return false;
        }
        if self.session_option_is_current(&options) {
            if let Some(choice) = options.permission_mode {
                self.save_workspace_permission(choice);
            }
            return true;
        }
        if self.request_session_change(protocol::method::CONFIG_OPTIONS, json!(options), cx) {
            self.pending_permission_choice = options.permission_mode;
            self.status = "Updating runtime options…".into();
            true
        } else {
            false
        }
    }

    fn permission_workspace_key(&self) -> Option<String> {
        self.service_info
            .as_ref()
            .map(|info| info.workspace.clone())
            .or_else(|| self.workspace.as_ref().map(|path| path.to_string_lossy().into_owned()))
    }

    pub(super) fn save_workspace_permission(&mut self, choice: protocol::PermissionMode) {
        if let Some(workspace) = self.permission_workspace_key() {
            self.preferences.touch_workspace(&workspace);
            let _previous = self.preferences.workspace_permissions.insert(workspace, choice);
            if let Err(error) = self.preferences.try_save() {
                self.error =
                    Some(format!("Permission choice is active for this session, but could not be saved: {error}"));
            }
        }
    }

    pub(super) fn restore_workspace_permission(&mut self, cx: &mut Context<Self>) {
        let Some(choice) = self
            .permission_workspace_key()
            .and_then(|workspace| self.preferences.workspace_permissions.get(&workspace).copied())
        else {
            return;
        };
        let options = protocol::SessionOptionsSelectParams {
            permission_mode: Some(choice),
            ..Default::default()
        };
        if !self.session_option_is_current(&options) && !self.select_session_options(options, cx) {
            self.error = Some(
                "Could not restore this workspace's saved permission choice. Select it again before continuing.".into(),
            );
        }
    }

    pub(super) fn workspace_permission_ready(&self) -> bool {
        self.permission_workspace_key()
            .and_then(|workspace| self.preferences.workspace_permissions.get(&workspace))
            .is_none_or(|saved| {
                self.configuration
                    .as_ref()
                    .and_then(|config| config.session_options.as_ref())
                    .is_some_and(|options| options.permission_mode == *saved)
            })
    }

    pub(super) fn session_option_is_current(&self, options: &protocol::SessionOptionsSelectParams) -> bool {
        self.configuration
            .as_ref()
            .and_then(|config| config.session_options.as_ref())
            .is_some_and(|current| {
                options.permission_mode.is_none_or(|mode| current.permission_mode == mode)
                    && options.mode.as_ref().is_none_or(|mode| current.mode == *mode)
                    && options
                        .reasoning_effort
                        .as_ref()
                        .is_none_or(|effort| current.reasoning_effort == *effort)
                    && options.fast_mode.is_none_or(|enabled| current.fast_mode_enabled == enabled)
            })
    }

    pub(super) fn open_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_select_session_options() {
            return;
        }
        let entries = [
            (
                protocol::PermissionMode::Ask,
                "Ask before protected actions",
                "Confirm protected commands using the CLI's approval flow.",
            ),
            (
                protocol::PermissionMode::AutoApproveCommands,
                "Auto-approve commands",
                "Skip command confirmations. Sandbox and tool authorization still apply.",
            ),
        ]
        .into_iter()
        .map(|(permission_mode, label, description)| {
            let mut entry = command(
                label,
                PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                    permission_mode: Some(permission_mode),
                    ..Default::default()
                }),
            );
            entry.description = Some(description.into());
            entry
        })
        .collect();
        self.show_palette(PaletteKind::Permissions, vec![("For the next session".into(), entries)], window, cx);
    }

    pub(super) fn open_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_select_session_options() {
            return;
        }
        let entries = self
            .configuration
            .as_ref()
            .and_then(|config| config.session_options.as_ref())
            .into_iter()
            .flat_map(|options| &options.available_modes)
            .map(|mode| {
                let mut entry = command(
                    capitalize_agent_label(&mode.display_name),
                    PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                        mode: Some(mode.name.clone()),
                        ..Default::default()
                    }),
                );
                entry.description = Some(mode.description.clone());
                entry
            })
            .collect();
        self.show_palette(PaletteKind::Modes, vec![("Default agent for this workspace".into(), entries)], window, cx);
    }

    pub(super) fn render_mode_button(&self, _cx: &Context<Self>) -> AnyElement {
        let mode = self
            .active_primary_agent_display
            .as_ref()
            .filter(|active| self.selected_session_id.as_deref() == Some(active.session_id.as_str()))
            .map(|active| active.display_name.as_str())
            .or_else(|| {
                self.configuration
                    .as_ref()
                    .and_then(|config| config.session_options.as_ref())
                    .map(|options| {
                        options
                            .available_modes
                            .iter()
                            .find(|mode| mode.name == options.mode)
                            .map_or(options.mode.as_str(), |mode| mode.display_name.as_str())
                    })
            })
            .map(capitalize_agent_label)
            .unwrap_or_else(|| "Unavailable".to_string());
        desktop_button("composer-mode")
            .child(div().id("composer-mode-label").test_support().truncate().child(mode.clone()))
            .max_w(px(180.))
            .overflow_hidden()
            .accessibility_label(format!("Agent mode: {mode}"))
            .tooltip_with_action("Choose agent mode", &OpenModePicker, Some("VTCodeDesktop"))
            .dropdown_caret(true)
            .ghost()
            .small()
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenModePicker, self.focus_handle.clone())
            .into_any_element()
    }
}

pub(super) fn model_options_header(
    config: Option<&ConfigurationSnapshot>,
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
            .and_then(|slider| effort_at(&options.supported_efforts, slider.read(cx).value().start()))
            .unwrap_or(&options.reasoning_effort)
            .as_str()
    });
    let model_fallback = rust_i18n::t!("app.composer.choose_model").to_string();
    let model = model_label
        .or_else(|| config.map(|config| config.model.as_str()))
        .unwrap_or(&model_fallback);
    let summary_label = format!("{model}{}{}", effort.map(|_| " · ").unwrap_or(""), effort.unwrap_or(""));
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
                .aria_label(format!("Current reasoning effort: {effort}"))
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(capitalize_agent_label(effort)),
        );
    }
    let mut panel = div().p_3().flex().flex_col().gap_2().min_w_0().child(summary);
    if let Some(slider) = slider {
        panel = panel.child(effort_knob(
            slider,
            focus,
            options.map_or(0, |options| options.supported_efforts.len()),
            effort,
            enabled,
            cx,
        ));
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
                .map(|effort| rust_i18n::t!("app.composer.reasoning_effort", effort = effort).to_string())
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
fn effort_at(levels: &[String], value: f32) -> Option<&String> {
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

    use super::{
        ComposerTriggerKind, DesktopSlashAction, capitalize_agent_label, composer_trigger, desktop_slash_action,
        effort_at, file_mention_token, permission_indicator,
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
    fn desktop_slash_actions_route_only_exact_native_equivalents() {
        assert_eq!(desktop_slash_action(" /new "), Some(DesktopSlashAction::NewSession));
        assert_eq!(desktop_slash_action("/exit"), Some(DesktopSlashAction::CloseSession));
        assert_eq!(desktop_slash_action("/stop"), Some(DesktopSlashAction::StopTurn));
        assert_eq!(desktop_slash_action("/models"), Some(DesktopSlashAction::ModelPicker));
        assert_eq!(desktop_slash_action("/history"), Some(DesktopSlashAction::History));
        assert_eq!(desktop_slash_action("/splash"), Some(DesktopSlashAction::Splash));
        assert_eq!(desktop_slash_action("/config"), Some(DesktopSlashAction::Configuration));
        assert_eq!(desktop_slash_action("/settings"), Some(DesktopSlashAction::Settings(None)));
        assert_eq!(desktop_slash_action("/config permissions"), Some(DesktopSlashAction::Permissions));
        assert_eq!(
            desktop_slash_action("/config appearance"),
            Some(DesktopSlashAction::Settings(Some(super::super::settings::SettingsTab::Appearance,)))
        );
        assert_eq!(desktop_slash_action("/files src/ui"), Some(DesktopSlashAction::Files("src/ui".into())));
        assert_eq!(desktop_slash_action("/fast"), Some(DesktopSlashAction::FastMode(None)));
        assert_eq!(desktop_slash_action("/fast off"), Some(DesktopSlashAction::FastMode(Some(false))));
        assert_eq!(desktop_slash_action("/theme ciapre"), Some(DesktopSlashAction::Theme(Some("ciapre".into()))));
        assert_eq!(desktop_slash_action("/theme"), Some(DesktopSlashAction::Theme(None)));

        for input in [
            "/new please",
            "/fast maybe",
            "/config reset",
            "/splash now",
            "/files\nother",
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
