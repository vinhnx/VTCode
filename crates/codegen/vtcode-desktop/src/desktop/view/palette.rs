//! Command palette domain: entry types, search builders, and action dispatch.

use super::*;

/// One executable entry in the command palette.
#[derive(Debug)]
pub(super) struct PaletteCommand {
    pub(super) label: String,
    pub(super) description: Option<String>,
    pub(super) keywords: Vec<String>,
    pub(super) shared_keywords: Vec<gpui_kit::SharedString>,
    pub(super) action: PaletteAction,
}

/// Immutable entries shared by the owning palette and its rendered callbacks.
pub(super) type PaletteGroups = Vec<(String, Vec<Rc<PaletteCommand>>)>;

/// Conversation-search entries per group. Each entry clones full message text
/// into its keywords, so the palette keeps the newest rows instead of cloning
/// all 2,000 transcript items on every open and refresh.
pub(super) const MAX_CONVERSATION_SEARCH_ROWS: usize = 200;

pub(super) fn share_palette_commands(groups: Vec<(String, Vec<PaletteCommand>)>) -> PaletteGroups {
    groups
        .into_iter()
        .map(|(label, entries)| (label, entries.into_iter().map(Rc::new).collect()))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteKind {
    Commands,
    GoTo,
    ConversationSearch,
    ConfigSettings,
    Models,
    ModelOptions,
    AddContext,
    FileMentions,
    SlashCommands,
    PromptHistory,
    Permissions,
    Modes,
    Fonts(FontTarget),
    SettingsSearch,
    ReviewFiles,
}

/// What confirming a palette entry does. Executed through existing methods so
/// palette and toolbar stay behaviorally identical.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PaletteAction {
    NewSession,
    CloseSession,
    OpenConversationSearch { session_id: String },
    CopyLatestMessage,
    JumpToTranscriptItem { session_id: String, item_id: String },
    ShowActivityOutput { session_id: String, item_id: String },
    ChooseModel,
    ChooseWorkspace,
    RefreshSessions,
    RefreshFiles,
    BrowseThreads,
    ToggleInspector,
    ToggleTheme,
    SelectFont { target: FontTarget, family: Option<String> },
    SelectSettingsTab(settings::SettingsTab),
    EditConfigSetting(String),
    LoadSession(String),
    PreviewFile(String),
    SelectModel { provider: String, model: String },
    SelectSessionOptions(protocol::SessionOptionsSelectParams),
    ToggleZenMode,
    AttachFiles,
    AttachFile(String),
    InsertFileMention(String),
    CompleteSlashCommand(String),
    InsertHistoryPrompt(String),
    InsertDraft(String),
    SelectWorkspaceReviewFile(String),
    OpenDeepLink(Box<deeplinks::DeepLink>),
    OpenGoTo,
}

impl DesktopView {
    /// Build and open the ⌘K command palette over the current state.
    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::Commands, window, cx) {
            return;
        }
        let mut groups: Vec<(String, Vec<PaletteCommand>)> = Vec::new();
        groups.push((
            rust_i18n::t!("app.palette.sections.session").to_string(),
            vec![
                command(rust_i18n::t!("app.palette.command.new_session").to_string(), PaletteAction::NewSession),
                command(rust_i18n::t!("app.palette.command.close_session").to_string(), PaletteAction::CloseSession),
                command(
                    rust_i18n::t!(i18n::PALETTE_COMMAND_SWITCH_WORKSPACE).to_string(),
                    PaletteAction::ChooseWorkspace,
                ),
                command(
                    rust_i18n::t!("app.palette.command.refresh_recent_threads").to_string(),
                    PaletteAction::RefreshSessions,
                ),
                command(rust_i18n::t!(i18n::FILES_REFRESH_TOOLTIP).to_string(), PaletteAction::RefreshFiles),
                command(rust_i18n::t!(i18n::PALETTE_COMMAND_CHOOSE_MODEL).to_string(), PaletteAction::ChooseModel),
            ],
        ));
        groups.push((
            rust_i18n::t!("app.palette.sections.view").to_string(),
            vec![
                command(rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string(), PaletteAction::BrowseThreads),
                command(
                    rust_i18n::t!(i18n::PALETTE_COMMAND_TOGGLE_INSPECTOR).to_string(),
                    PaletteAction::ToggleInspector,
                ),
                command(rust_i18n::t!("app.palette.command.toggle_zen_mode").to_string(), PaletteAction::ToggleZenMode),
                command(
                    if self.theme_mode == ThemeMode::Light {
                        rust_i18n::t!("app.palette.command.switch_dark_theme").to_string()
                    } else {
                        rust_i18n::t!("app.palette.command.switch_light_theme").to_string()
                    },
                    PaletteAction::ToggleTheme,
                ),
                {
                    let mut goto =
                        command(rust_i18n::t!(i18n::PALETTE_COMMAND_GO_TO).to_string(), PaletteAction::OpenGoTo);
                    goto.keywords = vec![
                        "go".to_string(),
                        "goto".to_string(),
                        "navigate".to_string(),
                        "deeplink".to_string(),
                    ];
                    goto
                },
            ],
        ));
        groups.push((
            rust_i18n::t!("app.settings.search.group_settings").to_string(),
            vec![{
                let mut entry = command(
                    settings::SettingsTab::Shortcuts.label(),
                    PaletteAction::SelectSettingsTab(settings::SettingsTab::Shortcuts),
                );
                entry.description = Some(rust_i18n::t!("app.shortcuts.group_label").to_string());
                entry.keywords = settings::SettingsTab::Shortcuts
                    .search_keywords()
                    .iter()
                    .map(|keyword| (*keyword).to_string())
                    .collect();
                entry
                    .keywords
                    .push(rust_i18n::t!("app.settings.search.group_settings").to_string());
                entry
            }],
        ));
        if let Some(session_id) = self.selected_session_id.clone()
            && self.has_searchable_conversation_content()
        {
            groups.push((
                rust_i18n::t!("app.palette.sections.conversation").to_string(),
                vec![
                    command(
                        rust_i18n::t!("app.palette.command.find_in_conversation").to_string(),
                        PaletteAction::OpenConversationSearch { session_id },
                    ),
                    command(
                        rust_i18n::t!("app.palette.command.copy_latest_message").to_string(),
                        PaletteAction::CopyLatestMessage,
                    ),
                ],
            ));
        }
        let slash_commands = self
            .slash_commands
            .iter()
            .map(|entry| {
                let mut item =
                    command(format!("/{}", entry.name), PaletteAction::InsertDraft(format!("/{} ", entry.name)));
                item.description = Some(entry.description.clone());
                item.keywords = vec![entry.name.clone(), format!("/{}", entry.name)];
                item
            })
            .collect::<Vec<_>>();
        if !slash_commands.is_empty() {
            groups.push((rust_i18n::t!(i18n::PALETTE_SECTIONS_VTCODE_COMMANDS).to_string(), slash_commands));
        }
        let sessions: Vec<PaletteCommand> = self
            .sessions
            .iter()
            .map(|session| {
                let label = session_display_title(&session.session_id, session.title.as_deref());
                command(
                    rust_i18n::t!("app.palette.command.open_session", label = label.as_str()).to_string(),
                    PaletteAction::LoadSession(session.session_id.clone()),
                )
            })
            .collect();
        if !sessions.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.recent_sessions").to_string(), sessions));
        }
        let file_commands: Vec<PaletteCommand> = self
            .files
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| {
                command(
                    rust_i18n::t!("app.palette.command.open_file", path = entry.rel_path.as_str()).to_string(),
                    PaletteAction::PreviewFile(entry.rel_path.clone()),
                )
            })
            .collect();
        if !file_commands.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.files").to_string(), file_commands));
        }

        self.show_palette(PaletteKind::Commands, groups, window, cx);
    }

    pub(super) fn open_conversation_search(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_session_id.as_deref() != Some(session_id.as_str())
            || !self.has_searchable_conversation_content()
        {
            return;
        }
        if self.toggle_palette_kind(PaletteKind::ConversationSearch, window, cx) {
            return;
        }

        self.show_palette(PaletteKind::ConversationSearch, self.conversation_search_groups(&session_id), window, cx);
    }

    fn conversation_search_groups(&self, session_id: &str) -> Vec<(String, Vec<PaletteCommand>)> {
        let messages = self.conversation_search_messages(session_id);
        let outputs = self.conversation_search_outputs(session_id);
        let mut groups = Vec::with_capacity(2);
        if !messages.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.loaded_messages").to_string(), messages));
        }
        if !outputs.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.loaded_tool_output").to_string(), outputs));
        }
        groups
    }

    fn conversation_search_messages(&self, session_id: &str) -> Vec<PaletteCommand> {
        self.transcript
            .items_newest_first()
            .filter(|item| is_searchable_conversation_role(&item.role) && !item.text.trim().is_empty())
            .take(MAX_CONVERSATION_SEARCH_ROWS)
            .map(|item| {
                let speaker = if item.is_user() {
                    rust_i18n::t!("app.palette.command.you").to_string()
                } else if item.title.trim().is_empty() && item.is_assistant() {
                    rust_i18n::t!("app.palette.command.vtcode").to_string()
                } else if item.title.trim().is_empty() {
                    rust_i18n::t!("app.palette.command.tool_activity").to_string()
                } else {
                    item.title.to_string()
                };
                let label = format!("{speaker} · {}", transcript_search_snippet(&item.text));
                let mut entry = command(
                    label,
                    PaletteAction::JumpToTranscriptItem {
                        session_id: session_id.to_string(),
                        item_id: item.id.clone(),
                    },
                );
                entry.shared_keywords.extend([item.text.clone(), item.title.clone()]);
                entry
            })
            .collect()
    }

    fn conversation_search_outputs(&self, session_id: &str) -> Vec<PaletteCommand> {
        let mut seen = HashSet::new();
        // Newest first, matching the message group: restored history is older
        // than the live transcript, so the reversed chain reads newest-first.
        self.restored_activity
            .iter()
            .chain(self.transcript.items())
            .rev()
            .filter(|item| item.is_tool() && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
            .filter(|item| seen.insert(item.id.as_str()))
            .take(MAX_CONVERSATION_SEARCH_ROWS)
            .filter_map(|item| {
                let output = item.output.as_ref()?;
                let searchable_output = inspector::output_tail(output);
                let mut entry = command(
                    format!(
                        "{} · {} · {}",
                        rust_i18n::t!("app.palette.command.output"),
                        item.title,
                        transcript_search_snippet(&searchable_output)
                    ),
                    PaletteAction::ShowActivityOutput {
                        session_id: session_id.to_string(),
                        item_id: item.id.clone(),
                    },
                );
                entry.shared_keywords.extend([searchable_output, item.title.clone()]);
                Some(entry)
            })
            .collect()
    }

    pub(super) fn refresh_conversation_search(&mut self, session_id: &str) {
        if self.palette_kind != PaletteKind::ConversationSearch
            || self.selected_session_id.as_deref() != Some(session_id)
        {
            return;
        }
        self.palette_groups = share_palette_commands(self.conversation_search_groups(session_id));
    }

    /// Raw text of the newest user/assistant message, for the keyboard
    /// copy path that backs the hover-only message actions.
    pub(super) fn latest_copyable_message(&self) -> Option<gpui_kit::SharedString> {
        self.transcript
            .items_newest_first()
            .find(|item| (item.is_user() || item.is_assistant()) && !item.text.trim().is_empty())
            .map(|item| item.text.clone())
    }

    fn copy_latest_message(&self, window: &mut Window, cx: &mut App) {
        if let Some(text) = self.latest_copyable_message() {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.to_string()));
            super::super::notifications::copied_to_clipboard(window, cx);
        }
    }

    pub(super) fn has_searchable_conversation_content(&self) -> bool {
        self.transcript.items().any(|item| {
            (is_searchable_conversation_role(&item.role) && !item.text.trim().is_empty())
                || (item.is_tool() && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
        }) || self
            .restored_activity
            .iter()
            .any(|item| item.is_tool() && item.output.as_ref().is_some_and(|output| !output.trim().is_empty()))
    }

    pub(super) fn open_model_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::Models, window, cx) {
            return;
        }
        if !self.session_change_allowed(None) || self.attachments_picker_open {
            return;
        }
        let mut groups = std::collections::BTreeMap::<String, Vec<PaletteCommand>>::new();
        let active_model = self.active_model_fallback();
        for model in self.models.iter().chain(active_model.iter()) {
            let entry = model_palette_command(model, self.model_is_current(&model.provider, &model.id));
            groups.entry(model.provider.clone()).or_default().push(entry);
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        for (_, entries) in &mut groups {
            entries.sort_by_key(|entry| !self.is_current_palette_action(&entry.action));
        }
        groups.sort_by_key(|(_, entries)| !entries.iter().any(|entry| self.is_current_palette_action(&entry.action)));
        self.show_palette(PaletteKind::Models, groups, window, cx);
    }

    fn active_model_fallback(&self) -> Option<protocol::ModelInfo> {
        self.configuration
            .as_ref()
            .filter(|configuration| {
                !self
                    .models
                    .iter()
                    .any(|model| model.provider == configuration.provider && model.id == configuration.model)
            })
            .map(|configuration| protocol::ModelInfo {
                provider: configuration.provider.clone(),
                id: configuration.model.clone(),
                display_name: configuration.model.clone(),
                context_window: configuration.context_window,
                current: true,
                recommended_rank: None,
                capabilities: None,
            })
    }

    pub(super) fn model_is_current(&self, provider: &str, model: &str) -> bool {
        if let Some(configuration) = self.configuration.as_ref() {
            configuration.provider == provider && configuration.model == model
        } else {
            self.models
                .iter()
                .any(|entry| entry.provider == provider && entry.id == model && entry.current)
        }
    }

    pub(super) fn is_current_palette_action(&self, action: &PaletteAction) -> bool {
        match action {
            PaletteAction::SelectModel { provider, model } => self.model_is_current(provider, model),
            PaletteAction::SelectSessionOptions(options) => self.session_option_is_current(options),
            PaletteAction::SelectFont { target, family } => {
                let typography = self.preferences.typography_for(self.theme_mode);
                let selected = match target {
                    FontTarget::Ui => typography.ui_font_family,
                    FontTarget::Code => typography.code_font_family,
                };
                family.as_deref() == selected
            }
            PaletteAction::SelectSettingsTab(tab) => !self.config_page_open && self.settings_tab == *tab,
            PaletteAction::OpenDeepLink(link) => {
                matches!(link.as_ref(), deeplinks::DeepLink::Route(route) if self.current_route() == *route)
            }
            PaletteAction::SelectWorkspaceReviewFile(path) => self
                .workspace_review
                .as_ref()
                .is_some_and(|review| review.selected_file.as_deref() == Some(path)),
            _ => false,
        }
    }

    /// Close the palette when the same kind is already open, so a second
    /// click on its trigger dismisses it. Returns true when it closed.
    pub(super) fn toggle_palette_kind(
        &mut self,
        kind: PaletteKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.palette.is_some() && self.palette_kind == kind {
            self.close_palette(window, cx);
            true
        } else {
            false
        }
    }

    pub(super) fn show_palette(
        &mut self,
        kind: PaletteKind,
        groups: Vec<(String, Vec<PaletteCommand>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.current_route() == Route::Review && kind != PaletteKind::ReviewFiles {
            self.close_review(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.current_route() == Route::Settings && kind != PaletteKind::SettingsSearch {
            self.close_settings(window, cx);
        }
        self.palette_return_focus = window.focused(cx);
        self.disarm_double_escape();
        let state = cx.new(|cx| CommandState::new(window, cx));
        self.palette = Some(state.clone());
        self.palette_kind = kind;
        self.palette_query = gpui_kit::SharedString::default();
        self.palette_groups = share_palette_commands(groups);
        // The palette installs its input state during its first render, so
        // focusing has to wait for that frame.
        cx.on_next_frame(window, |this, window, cx| this.focus_palette_state(window, cx));
        cx.notify();
    }

    pub(super) fn focus_palette_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::ModelOptions && self.effort_slider.is_some() {
            self.effort_focus.focus(window, cx);
            return;
        }
        if matches!(
            self.palette_kind,
            PaletteKind::FileMentions | PaletteKind::SlashCommands | PaletteKind::PromptHistory
        ) {
            let has_suggestions = self.palette_groups.first().is_some_and(|(_, entries)| !entries.is_empty());
            if has_suggestions && let Some(state) = self.palette.clone() {
                // Never clobber a live selection: with a prefilled query the
                // model already highlights the first match, whose row is not
                // necessarily row 0 (forcing row 0 would collapse to None).
                let needs_select = state.read(cx).selected_index().is_none() && state.read(cx).matched_count() > 0;
                if needs_select {
                    state.update(cx, |state, cx| {
                        state.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx);
                    });
                }
            }
            return;
        }
        if let Some(state) = self.palette.clone() {
            let current = (matches!(
                self.palette_kind,
                PaletteKind::Models
                    | PaletteKind::Permissions
                    | PaletteKind::Modes
                    | PaletteKind::Fonts(_)
                    | PaletteKind::SettingsSearch
                    | PaletteKind::ReviewFiles
            ))
            .then(|| {
                self.palette_groups.iter().enumerate().find_map(|(section, (_, entries))| {
                    entries
                        .iter()
                        .position(|entry| self.is_current_palette_action(&entry.action))
                        .map(|row| gpui_kit::component::IndexPath::new(row).section(section))
                })
            })
            .flatten();
            state.update(cx, |state, cx| {
                if let Some(current) = current {
                    state.set_selected_index(Some(current), window, cx);
                }
                state.focus(window, cx);
            });
        }
    }

    pub(super) fn run_palette_action(&mut self, action: PaletteAction, window: &mut Window, cx: &mut Context<Self>) {
        if !self.palette_action_available(&action) {
            cx.notify();
            return;
        }
        // Keep the compact model panel open for explicit effort choices so
        // users can compare levels without reopening the knob.
        if let PaletteAction::SelectSessionOptions(options) = &action
            && self.palette_kind == PaletteKind::ModelOptions
            && options.reasoning_effort.is_some()
        {
            let _selected = self.select_session_options(options.clone(), cx);
            cx.notify();
            return;
        }
        self.close_palette(window, cx);
        match action {
            PaletteAction::NewSession => {
                self.close_active_route(window, cx);
                let _ = self.create_session_from_ui(window, cx);
            }
            PaletteAction::CloseSession => {
                self.close_active_route(window, cx);
                let _ = self.close_session_from_ui(window, cx);
            }
            PaletteAction::OpenConversationSearch { session_id } => {
                self.open_conversation_search(session_id, window, cx);
            }
            PaletteAction::CopyLatestMessage => self.copy_latest_message(window, cx),
            PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
                if self.selected_session_id.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                if let Some(index) = self.transcript.position_by_id(&item_id) {
                    if self.transcript.get(index).is_some_and(|item| item.is_tool()) {
                        if self.transcript.reveal_tool_item(&item_id).is_none() {
                            return;
                        }
                        cx.notify();
                        let session_id = session_id.clone();
                        cx.on_next_frame(window, move |view, _, cx| {
                            if view.selected_session_id.as_deref() != Some(session_id.as_str()) {
                                return;
                            }
                            if let Some(index) = view.transcript.position_by_id(&item_id) {
                                view.message_scroller.update(cx, |state, cx| {
                                    let _scrolled = state.scroll_to_item(index, cx);
                                });
                            }
                        });
                    } else {
                        self.message_scroller.update(cx, |state, cx| {
                            let _scrolled = state.scroll_to_item(index, cx);
                        });
                    }
                }
            }
            PaletteAction::ShowActivityOutput { session_id, item_id } => {
                if self.selected_session_id.as_deref() == Some(session_id.as_str()) {
                    self.select_activity_output(item_id, window, cx);
                }
            }
            PaletteAction::ChooseModel => self.open_model_palette(window, cx),
            PaletteAction::ChooseWorkspace => self.choose_workspace(window, cx),
            PaletteAction::RefreshSessions => {
                let _ = self.request(protocol::method::SESSION_LIST, json!({}));
            }
            PaletteAction::RefreshFiles => self.refresh_files(cx),
            PaletteAction::BrowseThreads => self.open_thread_browser(window, cx),
            PaletteAction::ToggleInspector => self.toggle_inspector(cx),
            PaletteAction::ToggleZenMode => self.toggle_zen_mode(window, cx),
            PaletteAction::ToggleTheme => self.toggle_theme(window, cx),
            PaletteAction::OpenDeepLink(link) => {
                let _opened = self.open_deeplink(*link, window, cx);
            }
            PaletteAction::OpenGoTo => self.open_goto_palette(window, cx),
            PaletteAction::SelectFont { target, family } => self.select_font(target, family, cx),
            PaletteAction::SelectSettingsTab(tab) => {
                self.settings_tab = tab;
                self.config_page_open = false;
                if tab == settings::SettingsTab::Storage {
                    self.maybe_request_storage_summary(cx);
                }
                // Passive entry: refresh when a service can answer. Explicit
                // refresh controls keep the loud no-backend failure instead.
                if tab == settings::SettingsTab::Connections && self.backend.is_some() {
                    self.request_webmcp_status();
                }
                self.open_settings(window, cx);
                self.settings_navigation_focus.focus(window, cx);
                cx.notify();
            }
            PaletteAction::EditConfigSetting(path) => self.open_cli_config_editor(path, window, cx),
            PaletteAction::SelectSessionOptions(options) => {
                if options.permission_mode == Some(protocol::PermissionMode::FullAccess)
                    && options.full_access_confirmed != Some(true)
                    && !self.session_option_is_current(&options)
                {
                    self.open_full_access_confirm(options, window, cx);
                } else {
                    let _selected = self.select_session_options(options, cx);
                }
            }
            PaletteAction::AttachFiles => self.attach_files(window, cx),
            PaletteAction::AttachFile(path) => self.attach_context_file(path, cx),
            PaletteAction::InsertDraft(text) => {
                let current = self.composer.read(cx).value().to_string();
                let separator = if current.is_empty() || current.ends_with(char::is_whitespace) {
                    ""
                } else {
                    " "
                };
                self.replace_composer_text(format!("{current}{separator}{text} "), window, cx);
                self.reset_history_recall();
                self.focus_composer(window, cx);
            }
            PaletteAction::InsertHistoryPrompt(prompt) => {
                self.replace_composer_text(prompt, window, cx);
                self.reset_history_recall();
                self.focus_composer(window, cx);
            }
            PaletteAction::InsertFileMention(path) => self.complete_composer_trigger(
                composer_controls::ComposerTriggerKind::FileMention,
                match composer_controls::file_mention_token(&path) {
                    Some(mention) => mention,
                    None => return,
                },
                window,
                cx,
            ),
            PaletteAction::CompleteSlashCommand(name) => self.complete_composer_trigger(
                composer_controls::ComposerTriggerKind::SlashCommand,
                format!("/{name}"),
                window,
                cx,
            ),
            PaletteAction::LoadSession(session_id) => {
                let _ = self.load_session_from_ui(session_id, window, cx);
            }
            PaletteAction::PreviewFile(rel_path) => self.open_file_preview(rel_path, window, cx),
            PaletteAction::SelectWorkspaceReviewFile(path) => self.select_workspace_review_file(path, cx),
            PaletteAction::SelectModel { provider, model } => {
                if !self.model_is_current(&provider, &model) {
                    self.select_model(provider, model, cx);
                }
            }
        }
    }

    pub(super) fn palette_action_available(&self, action: &PaletteAction) -> bool {
        match action {
            PaletteAction::NewSession => self.new_thread_available(),
            PaletteAction::ChooseModel => self.session_change_allowed(None),
            PaletteAction::OpenConversationSearch { session_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && self.has_searchable_conversation_content()
            }
            PaletteAction::CopyLatestMessage => self.latest_copyable_message().is_some(),
            PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && self.transcript.position_by_id(item_id).is_some_and(|index| {
                        self.transcript
                            .get(index)
                            .is_some_and(|item| is_searchable_conversation_role(&item.role))
                    })
            }
            PaletteAction::ShowActivityOutput { session_id, item_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && (self
                        .restored_activity
                        .iter()
                        .any(|item| item.id == *item_id && item.is_tool() && item.output.is_some())
                        || self
                            .transcript
                            .position_by_id(item_id)
                            .and_then(|index| self.transcript.get(index))
                            .is_some_and(|item| item.is_tool() && item.output.is_some()))
            }
            PaletteAction::SelectSessionOptions(_) => self.can_select_session_options(),
            PaletteAction::AttachFiles | PaletteAction::AttachFile(_) => {
                self.workspace.is_some() && !self.attachments_picker_open && self.session_request.is_none()
            }
            PaletteAction::InsertDraft(_)
            | PaletteAction::InsertFileMention(_)
            | PaletteAction::CompleteSlashCommand(_)
            | PaletteAction::InsertHistoryPrompt(_) => true,
            PaletteAction::CloseSession => self.selected_session_id.is_some() && self.new_thread_available(),
            PaletteAction::LoadSession(session_id) => self.load_session_available(session_id),
            PaletteAction::SelectWorkspaceReviewFile(_) => {
                self.current_route() == Route::Review
                    && self.workspace_review.as_ref().is_some_and(|review| review.viewer.is_some())
            }
            PaletteAction::ChooseWorkspace => {
                self.retained_submission.is_none() && self.workspace_switch_idle() && !self.workspace_picker_open
            }
            PaletteAction::RefreshSessions => self.backend.is_some(),
            PaletteAction::RefreshFiles | PaletteAction::ToggleInspector | PaletteAction::PreviewFile(_) => {
                self.workspace.is_some()
            }
            PaletteAction::BrowseThreads
            | PaletteAction::ToggleZenMode
            | PaletteAction::ToggleTheme
            | PaletteAction::SelectFont { .. }
            | PaletteAction::SelectSettingsTab(_)
            | PaletteAction::OpenGoTo => true,
            PaletteAction::OpenDeepLink(link) => match link.as_ref() {
                deeplinks::DeepLink::Route(_)
                | deeplinks::DeepLink::SettingsTab(_)
                | deeplinks::DeepLink::ConfigPage
                | deeplinks::DeepLink::ConfigPath(_) => true,
                deeplinks::DeepLink::Palette(deeplinks::PaletteLink::ConversationSearch) => {
                    self.selected_session_id.is_some() && self.has_searchable_conversation_content()
                }
                deeplinks::DeepLink::Palette(deeplinks::PaletteLink::Models) => {
                    self.session_change_allowed(None) && !self.attachments_picker_open
                }
                deeplinks::DeepLink::Palette(deeplinks::PaletteLink::Permissions | deeplinks::PaletteLink::Modes) => {
                    self.can_select_session_options()
                }
                deeplinks::DeepLink::Palette(deeplinks::PaletteLink::ConfigSettings) => self.can_edit_cli_settings(),
                deeplinks::DeepLink::Palette(_) | deeplinks::DeepLink::Command(_) => true,
                deeplinks::DeepLink::Action(action) => self.palette_action_available(action),
            },
            PaletteAction::EditConfigSetting(path) => {
                self.can_edit_cli_settings()
                    && self
                        .cli_config_settings
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.settings.iter().any(|setting| setting.path == *path))
            }
            PaletteAction::SelectModel { provider, model } => {
                self.session_change_allowed(None)
                    && (self.model_is_current(provider, model)
                        || self
                            .models
                            .iter()
                            .any(|entry| entry.provider == *provider && entry.id == *model))
            }
        }
    }
}

pub(super) fn command(label: impl Into<String>, action: PaletteAction) -> PaletteCommand {
    PaletteCommand {
        label: label.into(),
        description: None,
        keywords: Vec::new(),
        shared_keywords: Vec::new(),
        action,
    }
}

/// Shield mode for permission-picker rows; every other palette and action stays text-only.
pub(super) fn permission_palette_mode(kind: PaletteKind, action: &PaletteAction) -> Option<protocol::PermissionMode> {
    if kind != PaletteKind::Permissions {
        return None;
    }
    match action {
        PaletteAction::SelectSessionOptions(params) => params.permission_mode,
        _ => None,
    }
}

pub(super) fn transcript_search_snippet(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    let mut chars = line.chars();
    let snippet = chars.by_ref().take(96).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}…")
    } else {
        snippet
    }
}

fn is_searchable_conversation_role(role: &str) -> bool {
    TranscriptRole::from_str(role).is_some_and(TranscriptRole::is_searchable_conversation)
}

/// Keep palette label identity attached to the command when search/filtering changes row order.
/// Hash payloads so file paths and draft text do not appear in rendered element IDs.
pub(super) fn palette_entry_element_id(action: &PaletteAction) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(action).hash(&mut hasher);
    match action {
        PaletteAction::SelectFont { target, family } => {
            match target {
                FontTarget::Ui => 0_u8,
                FontTarget::Code => 1_u8,
            }
            .hash(&mut hasher);
            family.hash(&mut hasher);
        }
        PaletteAction::EditConfigSetting(value)
        | PaletteAction::LoadSession(value)
        | PaletteAction::PreviewFile(value)
        | PaletteAction::SelectWorkspaceReviewFile(value)
        | PaletteAction::AttachFile(value)
        | PaletteAction::InsertFileMention(value)
        | PaletteAction::CompleteSlashCommand(value)
        | PaletteAction::InsertHistoryPrompt(value)
        | PaletteAction::InsertDraft(value) => value.hash(&mut hasher),
        PaletteAction::OpenConversationSearch { session_id } => session_id.hash(&mut hasher),
        PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
        }
        PaletteAction::ShowActivityOutput { session_id, item_id } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
        }
        PaletteAction::SelectSettingsTab(tab) => std::mem::discriminant(tab).hash(&mut hasher),
        PaletteAction::SelectModel { provider, model } => {
            provider.hash(&mut hasher);
            model.hash(&mut hasher);
        }
        PaletteAction::SelectSessionOptions(options) => {
            match options.permission_mode {
                None => 0_u8,
                Some(protocol::PermissionMode::Ask) => 1,
                Some(protocol::PermissionMode::AutoApproveCommands) => 2,
                Some(protocol::PermissionMode::FullAccess) => 3,
            }
            .hash(&mut hasher);
            options.mode.hash(&mut hasher);
            options.reasoning_effort.hash(&mut hasher);
            options.fast_mode.hash(&mut hasher);
        }
        PaletteAction::OpenDeepLink(link) => link.path().unwrap_or_else(|| format!("{link:?}")).hash(&mut hasher),
        PaletteAction::NewSession
        | PaletteAction::CloseSession
        | PaletteAction::CopyLatestMessage
        | PaletteAction::ChooseModel
        | PaletteAction::ChooseWorkspace
        | PaletteAction::RefreshSessions
        | PaletteAction::RefreshFiles
        | PaletteAction::BrowseThreads
        | PaletteAction::ToggleInspector
        | PaletteAction::ToggleZenMode
        | PaletteAction::ToggleTheme
        | PaletteAction::AttachFiles
        | PaletteAction::OpenGoTo => {}
    }
    format!("palette-label-{:016x}", hasher.finish())
}

pub(super) fn model_palette_command(model: &protocol::ModelInfo, current: bool) -> PaletteCommand {
    let context = model
        .context_window
        .map(|tokens| rust_i18n::t!("app.palette.context_suffix", window = format_context_window(tokens)).to_string())
        .unwrap_or_default();
    let current = if current {
        rust_i18n::t!("app.palette.current_suffix").to_string()
    } else {
        String::new()
    };
    let capabilities = model.capabilities.as_ref().map(model_capability_badges).unwrap_or_default();
    let mut entry = command(
        model.display_name.clone(),
        PaletteAction::SelectModel {
            provider: model.provider.clone(),
            model: model.id.clone(),
        },
    );
    let mut details = vec![model.provider.clone()];
    details.extend(capabilities.iter().map(|(_, display)| display.clone()));
    if !context.is_empty() {
        details.push(context.trim_start_matches(display::MIDDOT).to_string());
    }
    details.push(model.id.clone());
    if !current.is_empty() {
        details.push(current.trim_start_matches(display::MIDDOT).to_string());
    }
    entry.description = Some(details.join(display::MIDDOT));
    entry.keywords = vec![model.provider.clone(), model.id.clone()];
    entry
        .keywords
        .extend(capabilities.into_iter().map(|(keyword, _)| keyword.to_string()));
    entry
}

/// Capability badges as `(search_keyword, display_label)` pairs: keywords stay
/// English so palette search is locale-independent, while display labels are
/// localized. Tier and format names (`Pro`, `PDF`) are proper nouns/symbols
/// and stay English in both positions.
fn model_capability_badges(capabilities: &protocol::ModelCapabilities) -> Vec<(&'static str, String)> {
    let mut badges = Vec::new();
    if capabilities.reasoning {
        badges.push(("Reasoning", rust_i18n::t!("app.palette.cap_reasoning").to_string()));
    }
    if capabilities.vision {
        badges.push(("Vision", rust_i18n::t!("app.palette.cap_vision").to_string()));
    }
    if capabilities.tool_call {
        badges.push(("Tools", rust_i18n::t!("app.palette.cap_tools").to_string()));
    }
    if capabilities.structured_output {
        badges.push(("Structured output", rust_i18n::t!("app.palette.cap_structured").to_string()));
    }
    if capabilities.is_pro {
        badges.push(("Pro", "Pro".to_string()));
    }
    for (modality, keyword) in [("audio", "Audio"), ("video", "Video"), ("pdf", "PDF")] {
        if capabilities
            .input_modalities
            .iter()
            .any(|value| value.eq_ignore_ascii_case(modality))
        {
            let display = match modality {
                "audio" => rust_i18n::t!("app.palette.cap_audio").to_string(),
                "video" => rust_i18n::t!("app.palette.cap_video").to_string(),
                _ => keyword.to_string(),
            };
            badges.push((keyword, display));
        }
    }
    badges
}
