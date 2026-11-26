//! Command palette domain: entry types, search builders, and action dispatch.

use super::context_format::format_context_window;
use super::*;
use vtcode_commons::provider::Provider;

mod provider_identity;
use provider_identity::{model_route_matches, provider_display_label, provider_group_key, provider_picker_rank};
mod icons;
pub(super) use icons::palette_action_icon;
mod model_helpers;
pub(super) use model_helpers::*;

/// One executable entry in the command palette.
///
/// Displayed text is retained as `SharedString`: the palette re-renders on every
/// query keystroke and passes labels into `'static` row closures, so cloning an
/// entry must bump a refcount rather than copy message/config text per frame.
#[derive(Debug)]
pub(super) struct PaletteCommand {
    pub(super) label: gpui_kit::SharedString,
    pub(super) description: Option<gpui_kit::SharedString>,
    /// `label · description`, materialized once at ingestion so a render frame
    /// clones a shared handle instead of formatting every visible row again.
    pub(super) accessible_label: gpui_kit::SharedString,
    pub(super) keywords: Vec<String>,
    pub(super) shared_keywords: Vec<gpui_kit::SharedString>,
    pub(super) action: PaletteAction,
}

/// Immutable entries shared by the owning palette and its rendered callbacks.
/// Group labels are shared too — [`crate::desktop::view::palette_render`] clones
/// the whole table on each render frame.
pub(super) type PaletteGroups = Vec<(gpui_kit::SharedString, Vec<Rc<PaletteCommand>>)>;

/// Conversation-search entries per group. Each entry clones full message text
/// into its keywords, so the palette keeps the newest rows instead of cloning
/// all 2,000 transcript items on every open and refresh.
pub(super) const MAX_CONVERSATION_SEARCH_ROWS: usize = 200;

/// Workspace file rows per palette section. Each row clones its path (and the
/// Commands palette translates a label), so file sections keep the sorted
/// prefix of the shared snapshot instead of materializing all 5,000 rows on
/// every open.
pub(super) const MAX_FILE_ROWS: usize = 200;

/// The one ingestion boundary for palette groups: builders keep owning plain
/// `String`s, while the table retained by the view holds shared text so the
/// render path never copies it.
pub(super) fn share_palette_commands(groups: Vec<(String, Vec<PaletteCommand>)>) -> PaletteGroups {
    groups
        .into_iter()
        .map(|(label, entries)| {
            let entries = entries
                .into_iter()
                .map(|mut entry| {
                    // The palette rebuilds its row closures on every query
                    // keystroke. Derive the combined a11y label here, where the
                    // description is final, rather than formatting it per row
                    // per frame.
                    let accessible_label = entry
                        .description
                        .as_ref()
                        .map(|details| format!("{} · {}", entry.label, details).into())
                        .unwrap_or_else(|| entry.label.clone());
                    entry.accessible_label = accessible_label;
                    Rc::new(entry)
                })
                .collect();
            (label.into(), entries)
        })
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
    CopyThreadTitle,
    CopyFilePath,
    CloseOtherTabs,
    OpenInCli,
    OpenConversationSearch {
        session_id: String,
    },
    CopyLatestMessage,
    JumpToTranscriptItem {
        session_id: String,
        item_id: String,
        from_search: bool,
    },
    ShowActivityOutput {
        session_id: String,
        item_id: String,
    },
    ChooseModel,
    ChooseWorkspace,
    RefreshSessions,
    RefreshFiles,
    BrowseThreads,
    ToggleInspector,
    ToggleTheme,
    ToggleComputerUse,
    SelectFont {
        target: FontTarget,
        family: Option<String>,
    },
    SelectSettingsTab(settings::SettingsTab),
    EditConfigSetting(String),
    LoadSession(String),
    PreviewFile(String),
    SelectModel {
        provider: String,
        model: String,
    },
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
                // English literals match the thread-menu precedent: new
                // user-visible strings stay untranslated until full
                // native-quality translations ship for every locale.
                command("Copy thread title".to_string(), PaletteAction::CopyThreadTitle),
                command("Copy file path".to_string(), PaletteAction::CopyFilePath),
                command("Close other tabs".to_string(), PaletteAction::CloseOtherTabs),
                command(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string(), PaletteAction::OpenInCli),
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
                suggested_shortcut(shortcuts::CommandId::GoHome, &self.preferences.shortcuts),
                command(rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string(), PaletteAction::BrowseThreads),
                command(rust_i18n::t!("app.palette.command.toggle_zen_mode").to_string(), PaletteAction::ToggleZenMode),
                command(
                    if self.theme_mode == ThemeMode::Light {
                        rust_i18n::t!("app.palette.command.switch_dark_theme").to_string()
                    } else {
                        rust_i18n::t!("app.palette.command.switch_light_theme").to_string()
                    },
                    PaletteAction::ToggleTheme,
                ),
                suggested_shortcut(shortcuts::CommandId::Sidebar, &self.preferences.shortcuts),
                command(
                    rust_i18n::t!(i18n::PALETTE_COMMAND_TOGGLE_INSPECTOR).to_string(),
                    PaletteAction::ToggleInspector,
                ),
                suggested_shortcut(shortcuts::CommandId::StatusBar, &self.preferences.shortcuts),
                suggested_shortcut(shortcuts::CommandId::PromptHistory, &self.preferences.shortcuts),
                suggested_shortcut(shortcuts::CommandId::NavigateBack, &self.preferences.shortcuts),
                suggested_shortcut(shortcuts::CommandId::NavigateForward, &self.preferences.shortcuts),
                {
                    // English literal until full translations ship (see the
                    // session-group precedent above).
                    let mut toggle = command(
                        if self.computer_use_enabled() {
                            "Computer use: Turn off"
                        } else {
                            "Computer use: Turn on"
                        }
                        .to_string(),
                        PaletteAction::ToggleComputerUse,
                    );
                    toggle.keywords = vec![
                        "computer use".to_string(),
                        "computer-use".to_string(),
                        "cua".to_string(),
                        "browser".to_string(),
                    ];
                    toggle
                },
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
            vec![
                {
                    let mut entry = command(
                        settings::SettingsTab::Shortcuts.label(),
                        PaletteAction::SelectSettingsTab(settings::SettingsTab::Shortcuts),
                    );
                    entry.description = Some(rust_i18n::t!("app.shortcuts.group_label").to_string().into());
                    entry.keywords = settings::SettingsTab::Shortcuts
                        .search_keywords()
                        .iter()
                        .map(|keyword| (*keyword).to_string())
                        .collect();
                    entry
                        .keywords
                        .push(rust_i18n::t!("app.settings.search.group_settings").to_string());
                    entry
                },
                suggested_shortcut(shortcuts::CommandId::SettingsSearch, &self.preferences.shortcuts),
            ],
        ));
        if self.workspace.is_some() {
            let mut workspace_rows = vec![
                suggested_shortcut(shortcuts::CommandId::WorkspaceFiles, &self.preferences.shortcuts),
                suggested_shortcut(shortcuts::CommandId::RefreshWorkspace, &self.preferences.shortcuts),
                suggested_shortcut(shortcuts::CommandId::Utilities, &self.preferences.shortcuts),
            ];
            if self.backend.is_some() && self.service_info.is_some() {
                workspace_rows
                    .push(suggested_shortcut(shortcuts::CommandId::WorkspaceReview, &self.preferences.shortcuts));
            }
            groups.push((rust_i18n::t!(i18n::WORKSPACE_SECTION).to_string(), workspace_rows));
        }
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
                item.description = Some(entry.description.clone().into());
                item.keywords = vec![entry.name.clone(), format!("/{}", entry.name)];
                item
            })
            .collect::<Vec<_>>();
        if !slash_commands.is_empty() {
            groups.push((rust_i18n::t!(i18n::PALETTE_SECTIONS_VTCODE_COMMANDS).to_string(), slash_commands));
        }
        let sessions: Vec<PaletteCommand> = self
            .context_sessions()
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
            .take(MAX_FILE_ROWS)
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
            .filter(|item| item.is_conversation_searchable())
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
                        from_search: true,
                    },
                );
                entry.shared_keywords.extend(item.search_keywords());
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
                let searchable_output = inspector::display_output_tail(output);
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
            item.is_conversation_searchable()
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
            groups.entry(provider_group_key(&model.provider)).or_default().push(entry);
        }
        let mut groups = groups.into_iter().collect::<Vec<_>>();
        for (_, entries) in &mut groups {
            entries.sort_by_key(|entry| !self.is_current_palette_action(&entry.action));
        }
        // Groups follow the CLI picker's provider order: frontier labs
        // (OpenAI, Anthropic), then the open-source/local family, then the
        // rest. The active provider is highlighted in place instead of being
        // hoisted so the frontier-first order stays stable. Unknown codes rank
        // after every known provider; the sort is stable so they keep arrival
        // order. Headers show human-readable labels.
        let display_order = Provider::picker_display_order();
        groups.sort_by_key(|(code, _)| provider_picker_rank(code, &display_order));
        let groups = groups
            .into_iter()
            .map(|(code, entries)| (provider_display_label(&code), entries))
            .collect();
        self.show_palette(PaletteKind::Models, groups, window, cx);
    }

    fn active_model_fallback(&self) -> Option<protocol::ModelInfo> {
        self.configuration
            .as_ref()
            .filter(|configuration| {
                !self.models.iter().any(|model| {
                    model_route_matches(&model.provider, &model.id, &configuration.provider, &configuration.model)
                })
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
            model_route_matches(provider, model, &configuration.provider, &configuration.model)
        } else {
            self.models
                .iter()
                .any(|entry| model_route_matches(&entry.provider, &entry.id, provider, model) && entry.current)
        }
    }

    /// Human-readable display name for the active model route, when the
    /// catalog knows it. Provider aliases and model-id casing still resolve.
    pub(super) fn model_display_name(&self, provider: &str, model: &str) -> Option<&str> {
        self.models
            .iter()
            .find(|entry| model_route_matches(&entry.provider, &entry.id, provider, model))
            .map(|entry| entry.display_name.as_str())
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

    /// Open-state predicate for palette triggers: the popup is up and belongs
    /// to one of the trigger's kinds. `open` is state metadata only (the
    /// vendored Kit no longer paints it as a selection), so the visible open
    /// indicator is the popup itself and the trigger stays quiet while its
    /// popup is up. Triggers whose popup closes their own route (settings
    /// rows) unmount with it and need no mark; Kit-owned popovers mark their
    /// trigger themselves.
    pub(super) fn palette_open_for(&self, kinds: &[PaletteKind]) -> bool {
        self.palette.is_some() && kinds.contains(&self.palette_kind)
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
        // Composer-anchored transients are mutually exclusive: a palette
        // opening over the picker drops it first, mirroring
        // `open_attachment_picker` closing the palette.
        self.close_attachment_picker(window, cx);
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
        if !self.palette_action_available(&action, cx) {
            cx.notify();
            return;
        }
        // An explicit effort choice in the model panel is a complete task:
        // commit it and dismiss the panel, like the compact knob does. A
        // blocked choice leaves the panel open so the user can retry.
        if let PaletteAction::SelectSessionOptions(options) = &action
            && self.palette_kind == PaletteKind::ModelOptions
            && options.reasoning_effort.is_some()
        {
            if self.select_session_options(options.clone(), cx) {
                self.close_palette(window, cx);
            } else {
                cx.notify();
            }
            return;
        }
        // Close clears the query, so search jumps capture it first.
        let palette_query = self.palette_query.clone();
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
            PaletteAction::CopyThreadTitle => {
                if let Some(session_id) = self.selected_session_id.clone() {
                    self.copy_thread_title(&session_id, cx);
                }
            }
            PaletteAction::CopyFilePath => self.copy_selected_file_path(cx),
            PaletteAction::CloseOtherTabs => {
                if let Some(session_id) = self.selected_session_id.clone() {
                    self.close_other_tabs(&session_id, window, cx);
                }
            }
            PaletteAction::OpenInCli => self.open_cli_handoff(cx),
            PaletteAction::OpenConversationSearch { session_id } => {
                self.open_conversation_search(session_id, window, cx);
            }
            PaletteAction::CopyLatestMessage => self.copy_latest_message(window, cx),
            PaletteAction::JumpToTranscriptItem { session_id, item_id, from_search } => {
                if self.selected_session_id.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                if self.jump_to_transcript_item(&item_id, window, cx) && from_search {
                    self.remember_filter_matches(palette_query.as_str(), &item_id);
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
                // Refresh keeps the expanded depth: collapsing an explicit
                // "show more" would discard rows the user asked to see.
                let limit = self.session_list_limit;
                let _queued = self.request_session_list(limit);
            }
            PaletteAction::RefreshFiles => self.refresh_files(cx),
            PaletteAction::BrowseThreads => self.open_thread_browser(window, cx),
            PaletteAction::ToggleInspector => self.toggle_inspector(cx),
            PaletteAction::ToggleZenMode => self.toggle_zen_mode(window, cx),
            PaletteAction::ToggleTheme => self.toggle_theme(window, cx),
            PaletteAction::ToggleComputerUse => self.toggle_computer_use(cx),
            PaletteAction::OpenDeepLink(link) => {
                let _opened = self.open_deeplink(*link, window, cx);
            }
            PaletteAction::OpenGoTo => self.open_goto_palette(window, cx),
            PaletteAction::SelectFont { target, family } => self.select_font(target, family, cx),
            PaletteAction::SelectSettingsTab(tab) => {
                self.select_settings_tab(tab);
                if tab == settings::SettingsTab::VtCode {
                    self.refresh_login_item_status(cx);
                }
                if tab == settings::SettingsTab::Storage {
                    self.maybe_request_storage_summary(cx);
                }
                // The About page reads cached release notes; ask only when
                // launch never delivered a snapshot. Snapshot- and
                // pending-gated, so repeat selection costs nothing.
                if tab == settings::SettingsTab::About {
                    self.maybe_request_update_status();
                }
                // Passive entry: refresh when a service can answer. Explicit
                // refresh controls keep the loud no-backend failure instead.
                if tab == settings::SettingsTab::Connections && self.backend.is_some() {
                    self.request_webmcp_status();
                }
                self.open_settings(window, cx);
                self.focus_settings_selection(window, cx);
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

    pub(super) fn palette_action_available(&self, action: &PaletteAction, cx: &App) -> bool {
        match action {
            PaletteAction::NewSession => self.new_thread_available(),
            PaletteAction::ChooseModel => self.session_change_allowed(None),
            PaletteAction::OpenConversationSearch { session_id } => {
                self.selected_session_id.as_deref() == Some(session_id.as_str())
                    && self.has_searchable_conversation_content()
            }
            PaletteAction::CopyLatestMessage => self.latest_copyable_message().is_some(),
            PaletteAction::JumpToTranscriptItem { session_id, item_id, .. } => {
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
                        .any(|item| item.id == *item_id && item.is_tool() && item.has_output())
                        || self
                            .transcript
                            .position_by_id(item_id)
                            .and_then(|index| self.transcript.get(index))
                            .is_some_and(|item| item.is_tool() && item.has_output()))
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
            PaletteAction::CopyThreadTitle => self.selected_session_id.is_some(),
            PaletteAction::CopyFilePath => self.file_tree.read(cx).selected_item().is_some(),
            PaletteAction::CloseOtherTabs => {
                self.selected_session_id
                    .as_deref()
                    .is_some_and(|id| self.open_tabs.iter().any(|tab| tab == id))
                    && self.open_tabs.len() > 1
            }
            PaletteAction::OpenInCli => self.selected_session_id.is_some(),
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
            PaletteAction::ToggleComputerUse => self.can_edit_cli_settings(),
            PaletteAction::OpenDeepLink(link) => match link.as_ref() {
                deeplinks::DeepLink::Route(_)
                | deeplinks::DeepLink::SettingsTab(_)
                | deeplinks::DeepLink::ConfigPage
                | deeplinks::DeepLink::ConfigPath(_) => true,
                deeplinks::DeepLink::Changelog => self.can_view_any_changelog(),
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
                deeplinks::DeepLink::Action(action) => self.palette_action_available(action, cx),
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
                            .any(|entry| model_route_matches(&entry.provider, &entry.id, provider, model)))
            }
        }
    }
}

pub(super) fn command(label: impl Into<String>, action: PaletteAction) -> PaletteCommand {
    let label: gpui_kit::SharedString = label.into().into();
    PaletteCommand {
        accessible_label: label.clone(),
        label,
        description: None,
        keywords: Vec::new(),
        shared_keywords: Vec::new(),
        action,
    }
}

/// Suggested ⌘K row for a configurable command. The label and description
/// come from the shortcut table, and the `DeepLink::Command` action inherits
/// the command's badge through `palette_action_command_id`, so a suggested
/// row can never imply a chord that runs something else.
fn suggested_shortcut(
    id: shortcuts::CommandId,
    overrides: &std::collections::BTreeMap<String, String>,
) -> PaletteCommand {
    let shortcut = shortcuts::lookup(id);
    let link = deeplinks::DeepLink::Command(id);
    let mut entry = command(shortcut.label(), PaletteAction::OpenDeepLink(Box::new(link.clone())));
    entry.description = Some(shortcut.description().into());
    entry.keywords = vec![id.as_str().to_string(), link.path().unwrap_or_default()];
    let staged = shortcuts::staged_value(overrides, shortcut);
    if !staged.is_empty() {
        entry.keywords.push(staged.to_string());
    }
    entry
}

/// Shield mode for permission-picker rows (Commands rows use icons instead).
pub(super) fn permission_palette_mode(kind: PaletteKind, action: &PaletteAction) -> Option<protocol::PermissionMode> {
    if kind != PaletteKind::Permissions {
        return None;
    }
    match action {
        PaletteAction::SelectSessionOptions(params) => params.permission_mode,
        _ => None,
    }
}

#[cfg(test)]
mod tests;
