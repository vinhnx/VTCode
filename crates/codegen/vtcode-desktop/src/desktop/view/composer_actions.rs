//! Composer behavior: slash actions, suggestions, and session-option flows.
use super::composer_controls::{
    ComposerTriggerKind, DesktopSlashAction, capitalize_agent_label, composer_trigger, effort_at, file_mention_token,
    position_at,
};
use super::*;

impl DesktopView {
    fn clear_submitted_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |input, cx| input.set_value("", window, cx));
        self.reset_history_recall();
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
                    self.status = rust_i18n::t!("app.composer.no_turn_to_stop").to_string();
                    cx.notify();
                }
                true
            }
            DesktopSlashAction::NewSession => {
                if self.create_session_from_ui(window, cx) {
                    self.clear_submitted_command(window, cx);
                }
                true
            }
            DesktopSlashAction::CloseSession => {
                if self.selected_session_id.is_some() {
                    if self.close_session_from_ui(window, cx) {
                        self.clear_submitted_command(window, cx);
                    }
                } else {
                    self.status = rust_i18n::t!("app.composer.no_thread_to_close").to_string();
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
            DesktopSlashAction::Configuration(target) => {
                self.clear_submitted_command(window, cx);
                self.open_config_link(target, window, cx);
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
                    self.error = Some(rust_i18n::t!("app.composer.choose_workspace_files").to_string());
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
                let Some(options) = self.session_options().filter(|options| options.fast_mode_supported) else {
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
                    // Theme ids are lowercase; the CLI lowercases before lookup
                    // (`handle_theme_command`), so mirror that instead of
                    // rejecting a capitalized id the CLI would accept.
                    let canonical = theme.to_ascii_lowercase();
                    // `mode_for_id` covers the palette catalog plus the
                    // system-light/system-dark sentinels the picker accepts;
                    // `find` alone would reject the sentinels here.
                    if themes::mode_for_id(&canonical).is_none() {
                        self.clear_submitted_command(window, cx);
                        self.error =
                            Some(rust_i18n::t!("app.composer.theme_unavailable", theme = theme.as_str()).to_string());
                        cx.notify();
                        return true;
                    }
                    self.clear_submitted_command(window, cx);
                    self.apply_theme(canonical, window, cx);
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
            DesktopSlashAction::Cli => {
                self.clear_submitted_command(window, cx);
                self.open_cli_handoff(cx);
                true
            }
            DesktopSlashAction::Desktop => {
                self.clear_submitted_command(window, cx);
                // English literal: the repo does not add locale keys without
                // full native-quality translations.
                self.status = "You are already using VT Code Desktop.".to_string();
                cx.notify();
                true
            }
            DesktopSlashAction::Go(target) => {
                if target.trim().is_empty() {
                    self.clear_submitted_command(window, cx);
                    self.open_goto_palette(window, cx);
                    return true;
                }
                let Some(link) = deeplinks::parse(&target) else {
                    self.clear_submitted_command(window, cx);
                    self.error =
                        Some(rust_i18n::t!(i18n::ERRORS_UNKNOWN_DESTINATION, target = target.trim()).to_string());
                    cx.notify();
                    return true;
                };
                self.clear_submitted_command(window, cx);
                self.open_deeplink(link, window, cx)
            }
        }
    }

    pub(super) fn render_workspace_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.workspace.as_ref().map(|path| path.display().to_string());
        let current_label = self
            .workspace_file_name()
            .unwrap_or_else(|| rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string());
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
            .accessibility_label(rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string())
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
                    .lock_scroll_axis()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .child(eyebrow_label(&rust_i18n::t!(i18n::WORKSPACE_SECTION), &colors)),
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
                        let label = workspace_short_name(path).to_string();
                        let selected = current.as_deref() == Some(path.as_str());
                        let entry_view = view.clone();
                        let entry_path = path.clone();
                        content = content.child(
                            desktop_button(opaque_path_element_id("recent-workspace-popover", path))
                                .accessibility_label(if selected {
                                    rust_i18n::t!(i18n::WORKSPACE_CURRENT_LABEL, workspace = label).to_string()
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
        if self.toggle_palette_kind(PaletteKind::AddContext, window, cx) {
            return;
        }
        if self.workspace.is_none() || self.attachments_picker_open || self.session_request.is_some() {
            return;
        }
        self.show_palette(PaletteKind::AddContext, Vec::new(), window, cx);
        if !self.resources_loaded && !self.resources_loading && self.session_change_idle() {
            self.resources_loading = self.request(protocol::method::COMPOSER_RESOURCES, json!({}));
            if !self.resources_loading {
                self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
                cx.notify();
            }
        }
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
            .take(MAX_FILE_ROWS)
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
                command(rust_i18n::t!(i18n::PALETTE_COMMAND_CHOOSE_MODEL).to_string(), PaletteAction::ChooseModel),
                command(
                    rust_i18n::t!(i18n::PALETTE_COMMAND_SWITCH_WORKSPACE).to_string(),
                    PaletteAction::ChooseWorkspace,
                ),
                command(
                    rust_i18n::t!(i18n::PALETTE_COMMAND_TOGGLE_INSPECTOR).to_string(),
                    PaletteAction::ToggleInspector,
                ),
                command(rust_i18n::t!(i18n::FILES_REFRESH_TOOLTIP).to_string(), PaletteAction::RefreshFiles),
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
        groups.push((rust_i18n::t!(i18n::PALETTE_SECTIONS_VTCODE_COMMANDS).to_string(), slash_commands));
        groups
    }

    pub(super) fn update_composer_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::PromptHistory {
            // Reverse search is inline: the panel shows rows only, so every
            // draft change re-ranks from the composer text. History owns the
            // session until accept/cancel — triggers must not hijack a search
            // just because the filter contains `@` or `/`.
            let draft = self.composer.read(cx).value();
            if draft == self.palette_query {
                // Cursor moves and clicks re-enter here with identical text:
                // skip the re-rank so arrow-key navigation keeps its row.
                return;
            }
            self.refresh_prompt_history(&draft, window, cx);
            return;
        }
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
                (
                    PaletteKind::FileMentions,
                    vec![(rust_i18n::t!("app.composer.palette_group_files").to_string(), entries)],
                )
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
                (
                    PaletteKind::SlashCommands,
                    vec![(rust_i18n::t!("app.composer.palette_group_commands").to_string(), matching)],
                )
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
        // Flat row order across sections; the history palette appends a
        // cross-thread recents section under the live thread.
        let section_lens: Vec<usize> = self.palette_groups.iter().map(|(_, entries)| entries.len()).collect();
        let item_count: usize = section_lens.iter().sum();
        if item_count == 0 {
            return false;
        }
        let to_flat = |section: usize, row: usize| section_lens.iter().take(section).sum::<usize>() + row;
        let from_flat = |flat: usize| {
            let mut rest = flat;
            for (section, len) in section_lens.iter().enumerate() {
                if rest < *len {
                    return Some((section, rest));
                }
                rest -= len;
            }
            None
        };
        let current = state.read(cx).selected_index().and_then(|index| {
            section_lens
                .get(index.section)
                .is_some_and(|&len| index.row < len)
                .then(|| to_flat(index.section, index.row))
        });
        let step_from = |flat: usize| {
            if down {
                (flat + 1) % item_count
            } else if flat == 0 {
                item_count - 1
            } else {
                flat - 1
            }
        };
        // The highlight resolves against Kit's installed model, which lags the
        // host rows between a refresh and its render: step until a row sticks.
        // No `on_select` is wired, so probes are side-effect-free notifies.
        let mut candidate = current.map_or(if down { 0 } else { item_count - 1 }, step_from);
        state.update(cx, |state, cx| {
            for _ in 0..item_count {
                let Some((section, row)) = from_flat(candidate) else {
                    candidate = step_from(candidate);
                    continue;
                };
                state.set_selected_index(Some(gpui_kit::component::IndexPath::new(row).section(section)), window, cx);
                if state
                    .selected_index()
                    .is_some_and(|index| index.section == section && index.row == row)
                {
                    break;
                }
                candidate = step_from(candidate);
            }
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

    pub(super) fn handle_composer_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if action.secondary {
            // Cmd+Enter carries the batched-queue intent: leave it for the
            // QueueBatched binding instead of submitting a standalone turn.
            cx.propagate();
            return;
        }
        match self.palette_kind {
            PaletteKind::FileMentions => {
                if !self.accept_composer_suggestion(window, cx) {
                    self.submit_or_jump_to_latest(window, cx);
                }
            }
            PaletteKind::PromptHistory => {
                if !self.accept_composer_suggestion(window, cx) {
                    // Empty history or a filter matching nothing: Enter
                    // submits the draft instead of dying silently.
                    self.close_palette(window, cx);
                    self.submit_or_jump_to_latest(window, cx);
                }
            }
            PaletteKind::SlashCommands => {
                let _accepted = self.accept_composer_suggestion(window, cx);
                self.submit_or_jump_to_latest(window, cx);
            }
            _ => self.submit_or_jump_to_latest(window, cx),
        }
    }

    /// Plain-Enter submit with viewport reconciliation: a non-empty draft
    /// while scrolled away from the tail first jumps to the latest message
    /// (draft and focus preserved) instead of submitting; the next Enter
    /// submits. The Send button keeps immediate submit; only Enter detours.
    fn submit_or_jump_to_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).value().to_string();
        if !draft.trim().is_empty() && !self.message_scroller.read(cx).is_following_tail() {
            self.transcript.follow_latest();
            cx.notify();
            return;
        }
        self.submit_prompt(window, cx);
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

    pub(super) fn open_model_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::ModelOptions, window, cx) {
            return;
        }
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        self.show_palette(PaletteKind::ModelOptions, Vec::new(), window, cx);
        // Reopening must not reuse a previous knob: the fallback paths below
        // leave the slider untouched, so a stale range would mislabel the new
        // levels.
        self._effort_slider_subscriptions.clear();
        self.effort_slider = None;
        let Some(config) = self.configuration.as_ref() else {
            return;
        };
        let Some(options) = config.session_options.as_ref() else {
            return;
        };
        let levels = self.visible_effort_levels();
        if options.supported_efforts.len() > 8 {
            return;
        }
        let Some(selected) = levels.iter().position(|effort| effort == &options.reasoning_effort) else {
            // An inherited CLI value can be unsupported after a model switch. Offer
            // explicit valid choices without pretending that the knob is already set.
            // These ignore the visibility preference: the user must pick a level
            // the route actually supports.
            if options.supported_efforts.is_empty() {
                return;
            }
            let entries = options
                .supported_efforts
                .iter()
                .map(|effort| {
                    command(
                        rust_i18n::t!("app.composer.use_effort", effort = effort.as_str()).to_string(),
                        PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                            reasoning_effort: Some(effort.clone()),
                            ..Default::default()
                        }),
                    )
                })
                .collect();
            self.palette_groups =
                share_palette_commands(vec![(rust_i18n::t!("app.composer.palette_group_effort").to_string(), entries)]);
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
                let revert_levels = levels.clone();
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
                            let current = view.session_options().and_then(|options| {
                                revert_levels.iter().position(|level| level == &options.reasoning_effort)
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
        self.session_change_allowed(None) && !self.attachments_picker_open && self.session_options().is_some()
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
            self.status = rust_i18n::t!("app.permissions.updating_runtime").to_string();
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
        if choice == protocol::PermissionMode::FullAccess {
            return;
        }
        if let Some(workspace) = self.permission_workspace_key() {
            self.preferences.touch_workspace(&workspace);
            let _previous = self.preferences.workspace_permissions.insert(workspace, choice);
            if let Err(error) = self.preferences.try_save() {
                self.error = Some(rust_i18n::t!("app.permissions.save_failed", error = error.to_string()).to_string());
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
            self.error = Some(rust_i18n::t!("app.permissions.restore_failed").to_string());
        }
    }

    pub(super) fn workspace_permission_ready(&self) -> bool {
        if self
            .session_options()
            .is_some_and(|options| options.permission_mode == protocol::PermissionMode::FullAccess)
        {
            return true;
        }
        self.permission_workspace_key()
            .and_then(|workspace| self.preferences.workspace_permissions.get(&workspace))
            .is_none_or(|saved| self.session_options().is_some_and(|options| options.permission_mode == *saved))
    }

    pub(super) fn session_option_is_current(&self, options: &protocol::SessionOptionsSelectParams) -> bool {
        self.session_options().is_some_and(|current| {
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
        if self.toggle_palette_kind(PaletteKind::Permissions, window, cx) {
            return;
        }
        if !self.can_select_session_options() {
            return;
        }
        let entries = [
            (
                protocol::PermissionMode::Ask,
                permission_mode_label(protocol::PermissionMode::Ask),
                rust_i18n::t!("app.permissions.desc_ask").to_string(),
            ),
            (
                protocol::PermissionMode::AutoApproveCommands,
                permission_mode_label(protocol::PermissionMode::AutoApproveCommands),
                rust_i18n::t!("app.permissions.desc_auto").to_string(),
            ),
            (
                protocol::PermissionMode::FullAccess,
                permission_mode_label(protocol::PermissionMode::FullAccess),
                rust_i18n::t!("app.permissions.desc_full").to_string(),
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
            entry.description = Some(description);
            entry
        })
        .collect();
        self.show_palette(
            PaletteKind::Permissions,
            vec![(rust_i18n::t!("app.permissions.group_next_session").to_string(), entries)],
            window,
            cx,
        );
    }

    /// Cycle the primary agent forward (CLI Shift+Tab parity). The select
    /// path enforces the mode-switch lock, so mid-run presses are dropped
    /// exactly like the CLI busy notice.
    pub(super) fn cycle_primary_agent(&mut self, cx: &mut Context<Self>) {
        let Some(options) = self.session_options() else {
            return;
        };
        if options.available_modes.is_empty() {
            return;
        }
        // Unknown current mode (stale/custom value) starts at the first mode
        // instead of skipping it via `unwrap_or(0)`-then-increment.
        let next = options
            .available_modes
            .iter()
            .position(|mode| mode.name == options.mode)
            .map_or(0, |current| (current + 1) % options.available_modes.len());
        let Some(mode) = options.available_modes.get(next).map(|mode| mode.name.clone()) else {
            return;
        };
        // CLI parity: a lock-dropped cycle surfaces a busy notice instead of
        // silently swallowing the press (`MODE_SWITCH_BUSY_NOTICE`).
        if !self
            .select_session_options(protocol::SessionOptionsSelectParams { mode: Some(mode), ..Default::default() }, cx)
        {
            self.status = rust_i18n::t!("app.composer.mode_switch_busy").to_string();
            cx.notify();
        }
    }

    pub(super) fn open_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::Modes, window, cx) {
            return;
        }
        if !self.can_select_session_options() {
            return;
        }
        let entries = self
            .session_options()
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
        self.show_palette(
            PaletteKind::Modes,
            vec![(rust_i18n::t!("app.permissions.group_default_agent").to_string(), entries)],
            window,
            cx,
        );
    }

    pub(super) fn render_mode_button(&self, _cx: &Context<Self>) -> AnyElement {
        let mode = self
            .active_primary_agent_display
            .as_ref()
            .filter(|active| self.selected_session_id.as_deref() == Some(active.session_id.as_str()))
            .map(|active| active.display_name.as_str())
            .or_else(|| {
                self.session_options().map(|options| {
                    options
                        .available_modes
                        .iter()
                        .find(|mode| mode.name == options.mode)
                        .map_or(options.mode.as_str(), |mode| mode.display_name.as_str())
                })
            })
            .map(capitalize_agent_label)
            .unwrap_or_else(|| rust_i18n::t!(i18n::CONFIGURATION_UNAVAILABLE).to_string());
        desktop_button("composer-mode")
            .child(div().id("composer-mode-label").test_support().truncate().child(mode.clone()))
            .max_w(px(180.))
            .overflow_hidden()
            .accessibility_label(rust_i18n::t!("app.composer.agent_mode_aria", mode = mode.as_str()).to_string())
            .tooltip_with_action(
                rust_i18n::t!("app.composer.choose_agent_mode").to_string(),
                &OpenModePicker,
                Some(key_context::DESKTOP),
            )
            .dropdown_caret(true)
            .ghost()
            .small()
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenModePicker, self.focus_handle.clone())
            .into_any_element()
    }
}

/// Short label for a session permission mode, shared by the composer
/// permission picker and the curated configuration row.
pub(super) fn permission_mode_label(mode: protocol::PermissionMode) -> String {
    match mode {
        protocol::PermissionMode::Ask => rust_i18n::t!(i18n::COMPOSER_PERMISSION_ASK).to_string(),
        protocol::PermissionMode::AutoApproveCommands => rust_i18n::t!(i18n::COMPOSER_PERMISSION_AUTO).to_string(),
        protocol::PermissionMode::FullAccess => rust_i18n::t!(i18n::COMPOSER_PERMISSION_FULL).to_string(),
    }
}
