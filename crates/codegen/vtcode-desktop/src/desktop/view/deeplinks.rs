//! Unified in-app deeplinks: every route, palette, shortcut command, and
//! addressable palette action behind one `verb/path` syntax.
//!
//! Verbs are case-insensitive ASCII; payloads (session ids, file paths, model
//! ids, config paths) keep their case. An optional `vtcode://` prefix is
//! accepted so links stay valid if an OS-registered scheme ever wraps them.
//! Reachable through `/go <link>` and the Go to palette.
//!
//! ```text
//! workspace | settings | browser | review
//! settings/<tab>            (tab names share the /settings aliases)
//! config | config/<path>
//! palette/<kind>            (commands, models, fonts/ui, …)
//! command/<id>              (the 33 configurable shortcut ids)
//! action/<verb>[/<args>]    (sessions, files, models, …)
//! ```
//!
//! Deliberately uncovered: free-text payload actions (`InsertDraft`,
//! `InsertHistoryPrompt`), the composite session-options action (use the
//! permissions/modes palettes), and the composer-anchored `@`/`/` popups,
//! which only make sense with live trigger state.

use super::*;

/// Prefix an OS-registered scheme would wrap these links with. Accepted and
/// stripped so typed, pasted, and future external links parse identically.
const EXTERNAL_SCHEME: &str = "vtcode://";

/// Palette kinds with a standalone opener. The composer-anchored `@`-file and
/// `/`-slash popups are excluded: they need live trigger state to position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaletteLink {
    Commands,
    ConversationSearch,
    Models,
    ModelOptions,
    AddContext,
    Permissions,
    Modes,
    Fonts(FontTarget),
    SettingsSearch,
    ConfigSettings,
    PromptHistory,
}

impl PaletteLink {
    pub(super) const ALL: [Self; 12] = [
        Self::Commands,
        Self::ConversationSearch,
        Self::Models,
        Self::ModelOptions,
        Self::AddContext,
        Self::Permissions,
        Self::Modes,
        Self::Fonts(FontTarget::Ui),
        Self::Fonts(FontTarget::Code),
        Self::SettingsSearch,
        Self::ConfigSettings,
        Self::PromptHistory,
    ];

    fn verb(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::ConversationSearch => "conversation-search",
            Self::Models => "models",
            Self::ModelOptions => "model-options",
            Self::AddContext => "add-context",
            Self::Permissions => "permissions",
            Self::Modes => "modes",
            Self::Fonts(FontTarget::Ui) => "fonts/ui",
            Self::Fonts(FontTarget::Code) => "fonts/code",
            Self::SettingsSearch => "settings-search",
            Self::ConfigSettings => "config-settings",
            Self::PromptHistory => "prompt-history",
        }
    }
}

/// One addressable desktop destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DeepLink {
    Route(Route),
    SettingsTab(settings::SettingsTab),
    ConfigPage,
    ConfigPath(String),
    Palette(PaletteLink),
    Command(shortcuts::CommandId),
    Action(PaletteAction),
}

impl DeepLink {
    /// Canonical link text, or `None` for actions no link addresses
    /// (free-text inserts, composite session options, palette openers).
    pub(super) fn path(&self) -> Option<String> {
        match self {
            Self::Route(route) => Some(
                match route {
                    Route::Workspace => "workspace",
                    Route::Settings => "settings",
                    Route::Browser => "browser",
                    Route::Review => "review",
                }
                .to_string(),
            ),
            Self::SettingsTab(tab) => Some(format!("settings/{}", tab_name(*tab))),
            Self::ConfigPage => Some("config".to_string()),
            Self::ConfigPath(path) => Some(format!("config/{path}")),
            Self::Palette(kind) => Some(format!("palette/{}", kind.verb())),
            Self::Command(id) => Some(format!("command/{}", id.as_str())),
            Self::Action(action) => action_path(action),
        }
    }
}

/// Canonical tab segment: the primary name, not the slash aliases.
fn tab_name(tab: settings::SettingsTab) -> &'static str {
    match tab {
        settings::SettingsTab::Appearance => "appearance",
        settings::SettingsTab::Accessibility => "accessibility",
        settings::SettingsTab::Shortcuts => "shortcuts",
        settings::SettingsTab::VtCode => "vtcode",
        settings::SettingsTab::Connections => "connections",
        settings::SettingsTab::Notifications => "notifications",
        settings::SettingsTab::Storage => "storage",
    }
}

/// Inverse of [`parse_action`] for the addressable subset, plus the
/// overlapping tab/config actions in their canonical `settings/` and
/// `config/` forms. Free-text inserts, composite session options, and the
/// deeplink openers themselves have no link form.
fn action_path(action: &PaletteAction) -> Option<String> {
    match action {
        PaletteAction::NewSession => Some("action/new-session".to_string()),
        PaletteAction::CloseSession => Some("action/close-session".to_string()),
        PaletteAction::CopyLatestMessage => Some("action/copy-latest".to_string()),
        PaletteAction::ChooseModel => Some("action/choose-model".to_string()),
        PaletteAction::ChooseWorkspace => Some("action/choose-workspace".to_string()),
        PaletteAction::RefreshSessions => Some("action/refresh-sessions".to_string()),
        PaletteAction::RefreshFiles => Some("action/refresh-files".to_string()),
        PaletteAction::BrowseThreads => Some("action/browse-threads".to_string()),
        PaletteAction::ToggleInspector => Some("action/toggle-inspector".to_string()),
        PaletteAction::ToggleZenMode => Some("action/toggle-zen".to_string()),
        PaletteAction::ToggleTheme => Some("action/toggle-theme".to_string()),
        PaletteAction::AttachFiles => Some("action/attach-files".to_string()),
        PaletteAction::OpenConversationSearch { session_id } => {
            Some(format!("action/conversation-search/{session_id}"))
        }
        PaletteAction::JumpToTranscriptItem { session_id, item_id } => {
            Some(format!("action/jump/{session_id}/{item_id}"))
        }
        PaletteAction::ShowActivityOutput { session_id, item_id } => {
            Some(format!("action/output/{session_id}/{item_id}"))
        }
        PaletteAction::SelectFont { target, family } => {
            let target = match target {
                FontTarget::Ui => "ui",
                FontTarget::Code => "code",
            };
            match family {
                Some(family) => Some(format!("action/font/{target}/{family}")),
                None => Some(format!("action/font/{target}")),
            }
        }
        PaletteAction::SelectSettingsTab(tab) => Some(format!("settings/{}", tab_name(*tab))),
        PaletteAction::EditConfigSetting(path) => Some(format!("config/{path}")),
        PaletteAction::LoadSession(session_id) => Some(format!("action/load-session/{session_id}")),
        PaletteAction::PreviewFile(path) => Some(format!("action/preview-file/{path}")),
        PaletteAction::SelectModel { provider, model } => Some(format!("action/select-model/{provider}/{model}")),
        PaletteAction::AttachFile(path) => Some(format!("action/attach-file/{path}")),
        PaletteAction::InsertFileMention(path) => Some(format!("action/mention-file/{path}")),
        PaletteAction::CompleteSlashCommand(name) => Some(format!("action/slash/{name}")),
        PaletteAction::SelectWorkspaceReviewFile(path) => Some(format!("action/review-file/{path}")),
        PaletteAction::SelectSessionOptions(_)
        | PaletteAction::InsertDraft(_)
        | PaletteAction::InsertHistoryPrompt(_)
        | PaletteAction::OpenDeepLink(_)
        | PaletteAction::OpenGoTo => None,
    }
}

/// Parse link text into a destination. Returns `None` for unknown verbs,
/// unknown names, missing or empty payloads, and trailing segments on fixed
/// forms, so `/go` can report the input as an unknown destination instead of
/// falling through to the CLI runtime.
pub(super) fn parse(input: &str) -> Option<DeepLink> {
    let text = input.trim();
    let text = strip_scheme(text).trim();
    let text = text.trim_matches('/');
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    let (head, tail) = match text.split_once('/') {
        Some((head, tail)) => (head, tail),
        None => (text, ""),
    };
    match head.to_ascii_lowercase().as_str() {
        "workspace" => exact(tail, DeepLink::Route(Route::Workspace)),
        "browser" => exact(tail, DeepLink::Route(Route::Browser)),
        "review" => exact(tail, DeepLink::Route(Route::Review)),
        "settings" => parse_settings(tail),
        "config" => parse_config(tail),
        "palette" => parse_palette(tail),
        "command" => parse_command(tail),
        "action" => parse_action(tail),
        _ => None,
    }
}

/// Case-insensitive [`EXTERNAL_SCHEME`] strip. Boundary-safe through
/// `str::get`: a non-boundary split returns the input unchanged.
fn strip_scheme(text: &str) -> &str {
    text.get(..EXTERNAL_SCHEME.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(EXTERNAL_SCHEME))
        .and_then(|_| text.get(EXTERNAL_SCHEME.len()..))
        .unwrap_or(text)
}

/// Fixed forms accept no trailing segments.
fn exact<T>(tail: &str, value: T) -> Option<T> {
    tail.is_empty().then_some(value)
}

/// Payload join that rejects empty segments (`a//b` never addresses).
fn payload(segments: &[&str]) -> Option<String> {
    if segments.is_empty() || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    Some(segments.join("/"))
}

/// Single-segment payloads reject interior slashes: ids and names never
/// span segments, so `action/slash/a/b` is malformed, not a nested name.
fn single(segment: &str) -> Option<String> {
    (!segment.is_empty() && !segment.contains('/')).then(|| segment.to_string())
}

/// Multi-segment path payloads reject empty segments.
fn path_payload(rest: &str) -> Option<String> {
    payload(&rest.split('/').collect::<Vec<_>>())
}

/// Two-segment payloads (`provider/model`, `session/item`): both segments
/// required, neither spanning further.
fn pair(rest: &str) -> Option<(String, String)> {
    let (first, second) = rest.split_once('/')?;
    if first.is_empty() || second.is_empty() || second.contains('/') {
        return None;
    }
    Some((first.to_string(), second.to_string()))
}

fn parse_settings(tail: &str) -> Option<DeepLink> {
    if tail.is_empty() {
        return Some(DeepLink::Route(Route::Settings));
    }
    if tail.contains('/') {
        return None;
    }
    composer_controls::settings_tab_for_name(&tail.to_ascii_lowercase()).map(DeepLink::SettingsTab)
}

fn parse_config(tail: &str) -> Option<DeepLink> {
    if tail.is_empty() {
        return Some(DeepLink::ConfigPage);
    }
    path_payload(tail).map(DeepLink::ConfigPath)
}

fn parse_palette(tail: &str) -> Option<DeepLink> {
    let (verb, rest) = match tail.split_once('/') {
        Some((verb, rest)) => (verb, rest),
        None => (tail, ""),
    };
    match verb.to_ascii_lowercase().as_str() {
        "commands" => exact(rest, DeepLink::Palette(PaletteLink::Commands)),
        "conversation-search" => exact(rest, DeepLink::Palette(PaletteLink::ConversationSearch)),
        "models" => exact(rest, DeepLink::Palette(PaletteLink::Models)),
        "model-options" => exact(rest, DeepLink::Palette(PaletteLink::ModelOptions)),
        "add-context" => exact(rest, DeepLink::Palette(PaletteLink::AddContext)),
        "permissions" => exact(rest, DeepLink::Palette(PaletteLink::Permissions)),
        "modes" => exact(rest, DeepLink::Palette(PaletteLink::Modes)),
        "settings-search" => exact(rest, DeepLink::Palette(PaletteLink::SettingsSearch)),
        "config-settings" => exact(rest, DeepLink::Palette(PaletteLink::ConfigSettings)),
        "prompt-history" => exact(rest, DeepLink::Palette(PaletteLink::PromptHistory)),
        "fonts" => match rest.to_ascii_lowercase().as_str() {
            "ui" => Some(DeepLink::Palette(PaletteLink::Fonts(FontTarget::Ui))),
            "code" => Some(DeepLink::Palette(PaletteLink::Fonts(FontTarget::Code))),
            _ => None,
        },
        _ => None,
    }
}

fn parse_command(tail: &str) -> Option<DeepLink> {
    if tail.is_empty() || tail.contains('/') {
        return None;
    }
    shortcuts::CommandId::from_id(&tail.to_ascii_lowercase()).map(DeepLink::Command)
}

fn parse_action(tail: &str) -> Option<DeepLink> {
    let (verb, rest) = match tail.split_once('/') {
        Some((verb, rest)) => (verb, rest),
        None => (tail, ""),
    };
    let action = match verb.to_ascii_lowercase().as_str() {
        "new-session" => exact(rest, PaletteAction::NewSession)?,
        "close-session" => exact(rest, PaletteAction::CloseSession)?,
        "copy-latest" => exact(rest, PaletteAction::CopyLatestMessage)?,
        "choose-model" => exact(rest, PaletteAction::ChooseModel)?,
        "choose-workspace" => exact(rest, PaletteAction::ChooseWorkspace)?,
        "refresh-sessions" => exact(rest, PaletteAction::RefreshSessions)?,
        "refresh-files" => exact(rest, PaletteAction::RefreshFiles)?,
        "browse-threads" => exact(rest, PaletteAction::BrowseThreads)?,
        "toggle-inspector" => exact(rest, PaletteAction::ToggleInspector)?,
        "toggle-zen" => exact(rest, PaletteAction::ToggleZenMode)?,
        "toggle-theme" => exact(rest, PaletteAction::ToggleTheme)?,
        "attach-files" => exact(rest, PaletteAction::AttachFiles)?,
        "conversation-search" => PaletteAction::OpenConversationSearch { session_id: single(rest)? },
        "load-session" => PaletteAction::LoadSession(single(rest)?),
        "preview-file" => PaletteAction::PreviewFile(path_payload(rest)?),
        "attach-file" => PaletteAction::AttachFile(path_payload(rest)?),
        "mention-file" => PaletteAction::InsertFileMention(path_payload(rest)?),
        "review-file" => PaletteAction::SelectWorkspaceReviewFile(path_payload(rest)?),
        "edit-config" => PaletteAction::EditConfigSetting(path_payload(rest)?),
        "slash" => PaletteAction::CompleteSlashCommand(single(rest)?),
        "select-model" => {
            let (provider, model) = pair(rest)?;
            PaletteAction::SelectModel { provider, model }
        }
        "jump" => {
            let (session_id, item_id) = pair(rest)?;
            PaletteAction::JumpToTranscriptItem { session_id, item_id }
        }
        "output" => {
            let (session_id, item_id) = pair(rest)?;
            PaletteAction::ShowActivityOutput { session_id, item_id }
        }
        "font" => {
            let (target, family) = match rest.split_once('/') {
                Some((target, family)) => (target, Some(family)),
                None => (rest, None),
            };
            let target = match target.to_ascii_lowercase().as_str() {
                "ui" => FontTarget::Ui,
                "code" => FontTarget::Code,
                _ => return None,
            };
            let family = match family {
                Some(family) => Some(path_payload(family)?),
                None => None,
            };
            // `font/ui/` already trimmed to `font/ui`; an explicit empty
            // family (`font/ui/`) cannot survive the trim, so this only
            // rejects `font/` (empty target falls through above).
            PaletteAction::SelectFont { target, family }
        }
        _ => return None,
    };
    Some(DeepLink::Action(action))
}

impl DesktopView {
    /// Open a deeplink destination through the same openers the palette,
    /// slash commands, and keybindings use. Returns whether the destination
    /// was reachable; unreachable links leave state untouched so `/go` can
    /// report them instead of half-navigating.
    pub(super) fn open_deeplink(&mut self, link: DeepLink, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match link {
            DeepLink::Route(route) => {
                self.open_route_link(route, window, cx);
                true
            }
            DeepLink::SettingsTab(tab) => {
                self.run_palette_action(PaletteAction::SelectSettingsTab(tab), window, cx);
                true
            }
            DeepLink::ConfigPage => {
                self.open_config_link(None, window, cx);
                true
            }
            DeepLink::ConfigPath(path) => {
                self.open_config_link(Some(path), window, cx);
                true
            }
            DeepLink::Palette(kind) => self.open_palette_link(kind, window, cx),
            DeepLink::Command(id) => {
                // Same action object the keybinding dispatches, from the same
                // focus origin, so links and chords share behavior exactly.
                window.dispatch_action(shortcuts::action(id), cx);
                true
            }
            DeepLink::Action(action) => {
                // `run_palette_action` re-checks availability and no-ops when
                // unreachable, so refused links leave state untouched; report
                // reachability so `/go` surfaces the unavailable status error.
                let reachable = self.palette_action_available(&action);
                self.run_palette_action(action, window, cx);
                reachable
            }
        }
    }

    fn open_route_link(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        match route {
            Route::Workspace => self.close_active_route(window, cx),
            Route::Settings => self.open_settings(window, cx),
            Route::Browser => self.open_thread_browser(window, cx),
            Route::Review => self.show_review(window, cx),
        }
    }

    /// Configuration page with an optional deep target. Shared by the
    /// `/config` slash command and deeplinks so both land identically.
    pub(super) fn open_config_link(&mut self, target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Settings {
            self.close_settings(window, cx);
        }
        self.config_page_open = true;
        if let Some(target) = target {
            self.resolve_config_deep_link(&target, window, cx);
        }
        self.open_settings(window, cx);
    }

    fn open_palette_link(&mut self, kind: PaletteLink, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match kind {
            PaletteLink::Commands => {
                self.open_command_palette(window, cx);
                true
            }
            PaletteLink::ConversationSearch => match self.selected_session_id.clone() {
                Some(session_id) if self.has_searchable_conversation_content() => {
                    self.open_conversation_search(session_id, window, cx);
                    true
                }
                _ => false,
            },
            PaletteLink::Models => {
                if !self.session_change_allowed(None) || self.attachments_picker_open {
                    return false;
                }
                self.open_model_palette(window, cx);
                true
            }
            PaletteLink::ModelOptions => {
                self.open_model_options(window, cx);
                true
            }
            PaletteLink::AddContext => {
                self.open_add_context(window, cx);
                true
            }
            PaletteLink::Permissions => {
                if !self.can_select_session_options() {
                    return false;
                }
                self.open_permissions(window, cx);
                true
            }
            PaletteLink::Modes => {
                if !self.can_select_session_options() {
                    return false;
                }
                self.open_modes(window, cx);
                true
            }
            PaletteLink::Fonts(target) => {
                self.open_font_palette(target, window, cx);
                true
            }
            PaletteLink::SettingsSearch => {
                self.open_settings_search_palette(window, cx);
                true
            }
            PaletteLink::ConfigSettings => {
                if !self.can_edit_cli_settings() {
                    return false;
                }
                self.open_cli_config_palette(window, cx);
                true
            }
            PaletteLink::PromptHistory => {
                self.open_prompt_history(window, cx);
                true
            }
        }
    }

    /// Searchable destination palette behind `/go` and the ⌘K Go to entry:
    /// routes and palette openers, settings tabs plus the config page, and
    /// every shortcut command (badged through the shared badge mapping).
    pub(super) fn open_goto_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::GoTo, window, cx) {
            return;
        }
        self.show_palette(PaletteKind::GoTo, self.goto_palette_groups(), window, cx);
    }

    fn goto_palette_groups(&self) -> Vec<(String, Vec<PaletteCommand>)> {
        let mut destinations = Vec::new();
        for (route, label) in [
            (Route::Workspace, rust_i18n::t!(i18n::WORKSPACE_SECTION).to_string()),
            (Route::Settings, rust_i18n::t!(i18n::SETTINGS_TITLE).to_string()),
            (Route::Browser, rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string()),
            (Route::Review, rust_i18n::t!(i18n::WORKSPACE_REVIEW_TITLE).to_string()),
        ] {
            let link = DeepLink::Route(route);
            let mut entry = command(label, PaletteAction::OpenDeepLink(Box::new(link.clone())));
            entry.description = link.path();
            entry.keywords = vec![link.path().unwrap_or_default(), "route".to_string()];
            destinations.push(entry);
        }
        for kind in PaletteLink::ALL {
            let link = DeepLink::Palette(kind);
            let mut entry = command(palette_link_label(kind), PaletteAction::OpenDeepLink(Box::new(link.clone())));
            entry.description = link.path();
            entry.keywords = vec![link.path().unwrap_or_default(), "palette".to_string()];
            destinations.push(entry);
        }
        let mut settings = Vec::new();
        for tab in settings::SettingsTab::ALL {
            let mut entry = command(tab.label(), PaletteAction::SelectSettingsTab(tab));
            entry.description = DeepLink::SettingsTab(tab).path();
            entry.keywords = tab.search_keywords().iter().map(|keyword| (*keyword).to_string()).collect();
            entry.keywords.push(DeepLink::SettingsTab(tab).path().unwrap_or_default());
            settings.push(entry);
        }
        let mut config = command(
            rust_i18n::t!("app.configuration.page_title").to_string(),
            PaletteAction::OpenDeepLink(Box::new(DeepLink::ConfigPage)),
        );
        config.description = DeepLink::ConfigPage.path();
        config.keywords = vec!["config".to_string(), "configuration".to_string(), "cli".to_string()];
        settings.push(config);
        let mut commands = Vec::new();
        for shortcut in shortcuts::COMMANDS {
            let link = DeepLink::Command(shortcut.id);
            let mut entry = command(shortcut.label(), PaletteAction::OpenDeepLink(Box::new(link.clone())));
            entry.description = Some(shortcut.description());
            entry.keywords = vec![shortcut.id.as_str().to_string(), link.path().unwrap_or_default()];
            let staged = shortcuts::staged_value(&self.preferences.shortcuts, &shortcut);
            if !staged.is_empty() {
                entry.keywords.push(staged.to_string());
            }
            commands.push(entry);
        }
        vec![
            (rust_i18n::t!(i18n::PALETTE_TITLE_GO_TO).to_string(), destinations),
            (rust_i18n::t!("app.settings.search.group_settings").to_string(), settings),
            (rust_i18n::t!("app.palette.sections.commands").to_string(), commands),
        ]
    }
}

/// GoTo row label for a palette kind, reusing the palette's own title.
fn palette_link_label(kind: PaletteLink) -> String {
    match kind {
        PaletteLink::Commands => rust_i18n::t!("app.palette.title.commands").to_string(),
        PaletteLink::ConversationSearch => rust_i18n::t!("app.palette.title.conversation_search").to_string(),
        PaletteLink::Models => rust_i18n::t!(i18n::COMPOSER_CHOOSE_MODEL).to_string(),
        PaletteLink::ModelOptions => rust_i18n::t!("app.composer.select_model").to_string(),
        PaletteLink::AddContext => rust_i18n::t!(i18n::COMPOSER_ADD_CONTEXT).to_string(),
        PaletteLink::Permissions => rust_i18n::t!("app.palette.title.permissions").to_string(),
        PaletteLink::Modes => rust_i18n::t!("app.palette.title.modes").to_string(),
        PaletteLink::Fonts(FontTarget::Ui) => rust_i18n::t!("app.palette.title.ui_font").to_string(),
        PaletteLink::Fonts(FontTarget::Code) => rust_i18n::t!("app.palette.title.code_font").to_string(),
        PaletteLink::SettingsSearch => rust_i18n::t!(i18n::SETTINGS_SEARCH_TITLE).to_string(),
        PaletteLink::ConfigSettings => rust_i18n::t!(i18n::PALETTE_TITLE_CONFIGURATION).to_string(),
        PaletteLink::PromptHistory => rust_i18n::t!("app.palette.title.prompt_history").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeplink_paths_cover_every_route_tab_palette_and_command() {
        use settings::SettingsTab;
        // Routes, exact forms only.
        assert_eq!(parse("workspace"), Some(DeepLink::Route(Route::Workspace)));
        assert_eq!(parse("settings"), Some(DeepLink::Route(Route::Settings)));
        assert_eq!(parse("browser"), Some(DeepLink::Route(Route::Browser)));
        assert_eq!(parse("review"), Some(DeepLink::Route(Route::Review)));
        // Every settings tab by canonical name.
        for (name, tab) in [
            ("appearance", SettingsTab::Appearance),
            ("accessibility", SettingsTab::Accessibility),
            ("shortcuts", SettingsTab::Shortcuts),
            ("vtcode", SettingsTab::VtCode),
            ("connections", SettingsTab::Connections),
            ("notifications", SettingsTab::Notifications),
            ("storage", SettingsTab::Storage),
        ] {
            assert_eq!(parse(&format!("settings/{name}")), Some(DeepLink::SettingsTab(tab)), "{name}");
        }
        // Aliases share the slash table, including storage coverage.
        assert_eq!(parse("settings/keybindings"), Some(DeepLink::SettingsTab(SettingsTab::Shortcuts)));
        assert_eq!(parse("settings/disk"), Some(DeepLink::SettingsTab(SettingsTab::Storage)));
        // Config page and paths.
        assert_eq!(parse("config"), Some(DeepLink::ConfigPage));
        assert_eq!(parse("config/agent.default_model"), Some(DeepLink::ConfigPath("agent.default_model".into())));
        // Every palette kind.
        for (name, kind) in [
            ("commands", PaletteLink::Commands),
            ("conversation-search", PaletteLink::ConversationSearch),
            ("models", PaletteLink::Models),
            ("model-options", PaletteLink::ModelOptions),
            ("add-context", PaletteLink::AddContext),
            ("permissions", PaletteLink::Permissions),
            ("modes", PaletteLink::Modes),
            ("fonts/ui", PaletteLink::Fonts(FontTarget::Ui)),
            ("fonts/code", PaletteLink::Fonts(FontTarget::Code)),
            ("settings-search", PaletteLink::SettingsSearch),
            ("config-settings", PaletteLink::ConfigSettings),
            ("prompt-history", PaletteLink::PromptHistory),
        ] {
            assert_eq!(parse(&format!("palette/{name}")), Some(DeepLink::Palette(kind)), "{name}");
        }
        assert_eq!(PaletteLink::ALL.len(), 12);
        // Every shortcut command id round-trips.
        for id in shortcuts::CommandId::ALL {
            assert_eq!(parse(&format!("command/{}", id.as_str())), Some(DeepLink::Command(id)), "{}", id.as_str());
        }
        // Representative actions, parameterless and parameterized.
        assert_eq!(parse("action/toggle-zen"), Some(DeepLink::Action(PaletteAction::ToggleZenMode)));
        assert_eq!(parse("action/load-session/abc"), Some(DeepLink::Action(PaletteAction::LoadSession("abc".into()))));
        assert_eq!(
            parse("action/preview-file/src/main.rs"),
            Some(DeepLink::Action(PaletteAction::PreviewFile("src/main.rs".into())))
        );
        assert_eq!(
            parse("action/select-model/openai/gpt-5"),
            Some(DeepLink::Action(PaletteAction::SelectModel { provider: "openai".into(), model: "gpt-5".into() }))
        );
        assert_eq!(
            parse("action/font/code"),
            Some(DeepLink::Action(PaletteAction::SelectFont { target: FontTarget::Code, family: None }))
        );
        assert_eq!(
            parse("action/jump/sess/item-1"),
            Some(DeepLink::Action(PaletteAction::JumpToTranscriptItem {
                session_id: "sess".into(),
                item_id: "item-1".into(),
            }))
        );
    }

    #[test]
    fn deeplink_parse_is_lenient_on_form_and_strict_on_content() {
        use settings::SettingsTab;
        // Lenient: scheme prefix (any case), surrounding whitespace and
        // slashes, verb casing. Payloads keep their case.
        assert_eq!(parse("vtcode://settings/shortcuts"), parse("settings/shortcuts"));
        assert_eq!(parse("VTCODE://settings/shortcuts"), parse("settings/shortcuts"));
        assert_eq!(parse("  settings/shortcuts  "), parse("settings/shortcuts"));
        assert_eq!(parse("/settings/shortcuts/"), parse("settings/shortcuts"));
        assert_eq!(parse("settings/"), parse("settings"));
        assert_eq!(parse("action/font/ui/"), parse("action/font/ui"));
        assert_eq!(parse("SETTINGS/SHORTCUTS"), Some(DeepLink::SettingsTab(SettingsTab::Shortcuts)));
        assert_eq!(
            parse("action/preview-file/SRC/App.TS"),
            Some(DeepLink::Action(PaletteAction::PreviewFile("SRC/App.TS".into())))
        );
        assert_eq!(
            parse("action/select-model/OpenAI/GPT-5"),
            Some(DeepLink::Action(PaletteAction::SelectModel { provider: "OpenAI".into(), model: "GPT-5".into() }))
        );
        // Strict: unknown verbs and names, missing or empty payloads,
        // trailing segments on fixed forms, and control characters.
        for bad in [
            "",
            "   ",
            "nope",
            "workspace/extra",
            "settings/nope",
            "settings/shortcuts/extra",
            "command/",
            "command/nope",
            "command/commands/extra",
            "palette/",
            "palette/nope",
            "palette/commands/extra",
            "palette/fonts",
            "palette/fonts/nope",
            "action/",
            "action/nope",
            "action/toggle-zen/extra",
            "action/load-session",
            "action/load-session/",
            "action/load-session/a/b",
            "action/conversation-search/a/b",
            "action/slash/a/b",
            "action/preview-file",
            "action/preview-file//x",
            "action/select-model/only-provider",
            "action/select-model/a/b/c",
            "action/jump/only-session",
            "action/font",
            "action/font/nope",
            "action/font/ui//x",
            "config//x",
            "set\x07tings",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn deeplink_paths_round_trip_and_exclude_non_addressable_actions() {
        use settings::SettingsTab;
        use shortcuts::CommandId;
        let links = [
            DeepLink::Route(Route::Review),
            DeepLink::SettingsTab(SettingsTab::Storage),
            DeepLink::ConfigPage,
            DeepLink::ConfigPath("agent.default_model".into()),
            DeepLink::Palette(PaletteLink::Fonts(FontTarget::Code)),
            DeepLink::Command(CommandId::ZenMode),
            DeepLink::Action(PaletteAction::ToggleInspector),
            DeepLink::Action(PaletteAction::PreviewFile("src/main.rs".into())),
        ];
        for link in links {
            let path = link.path().expect("addressable links have paths");
            assert_eq!(parse(&path), Some(link), "{path}");
        }
        for id in CommandId::ALL {
            let link = DeepLink::Command(id);
            assert_eq!(parse(&link.path().unwrap()), Some(link));
        }
        // Non-addressable actions have no link form.
        assert_eq!(DeepLink::Action(PaletteAction::InsertDraft("hi".into())).path(), None);
        assert_eq!(DeepLink::Action(PaletteAction::InsertHistoryPrompt("hi".into())).path(), None);
        assert_eq!(DeepLink::Action(PaletteAction::SelectSessionOptions(Default::default())).path(), None);
        assert_eq!(
            DeepLink::Action(PaletteAction::OpenDeepLink(Box::new(DeepLink::Route(Route::Workspace)))).path(),
            None
        );
        assert_eq!(DeepLink::Action(PaletteAction::OpenGoTo).path(), None);
        // Overlapping actions canonicalize to their settings/config forms.
        assert_eq!(
            DeepLink::Action(PaletteAction::SelectSettingsTab(SettingsTab::Shortcuts))
                .path()
                .as_deref(),
            Some("settings/shortcuts")
        );
        assert_eq!(
            DeepLink::Action(PaletteAction::EditConfigSetting("agent.x".into()))
                .path()
                .as_deref(),
            Some("config/agent.x")
        );
    }
}
