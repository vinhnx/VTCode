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
//! command/<id>              (the 34 configurable shortcut ids)
//! action/<verb>[/<args>]    (sessions, files, models, …)
//! ```
//!
//! Deliberately uncovered: free-text payload actions (`InsertDraft`,
//! `InsertHistoryPrompt`), the composite session-options action (use the
//! permissions/modes palettes), and the composer-anchored `@`/`/` popups,
//! which only make sense with live trigger state.

use super::*;
use gpui_kit::{ClickEvent, WeakEntity};

/// Prefix an OS-registered scheme would wrap these links with. Accepted and
/// stripped so typed, pasted, and future external links parse identically.
const EXTERNAL_SCHEME: &str = "vtcode://";

/// Canonical link segments shared by the writers (`path`, `verb`,
/// `action_path`) and the parsers (`parse`, `parse_palette`, `parse_action`).
/// Both directions reference one table: a typo in either side would silently
/// break a `/go` link while still compiling.
struct Verb;

impl Verb {
    // Top-level heads (and fixed shortcut/config payload kinds).
    const WORKSPACE: &'static str = "workspace";
    const SETTINGS: &'static str = "settings";
    const BROWSER: &'static str = "browser";
    const REVIEW: &'static str = "review";
    const CHANGELOG: &'static str = "changelog";
    const CONFIG: &'static str = "config";
    const PALETTE: &'static str = "palette";
    const COMMAND: &'static str = "command";
    const ACTION: &'static str = "action";

    // Palette kinds.
    const COMMANDS: &'static str = "commands";
    const CONVERSATION_SEARCH: &'static str = "conversation-search";
    const MODELS: &'static str = "models";
    const MODEL_OPTIONS: &'static str = "model-options";
    const ADD_CONTEXT: &'static str = "add-context";
    const PERMISSIONS: &'static str = "permissions";
    const MODES: &'static str = "modes";
    const FONTS: &'static str = "fonts";
    const FONTS_UI: &'static str = "fonts/ui";
    const FONTS_CODE: &'static str = "fonts/code";
    const UI: &'static str = "ui";
    const CODE: &'static str = "code";
    const SETTINGS_SEARCH: &'static str = "settings-search";
    const CONFIG_SETTINGS: &'static str = "config-settings";
    const PROMPT_HISTORY: &'static str = "prompt-history";

    // Addressable palette actions.
    const NEW_SESSION: &'static str = "new-session";
    const CLOSE_SESSION: &'static str = "close-session";
    const COPY_THREAD_TITLE: &'static str = "copy-thread-title";
    const COPY_FILE_PATH: &'static str = "copy-file-path";
    const CLOSE_OTHER_TABS: &'static str = "close-other-tabs";
    const OPEN_IN_CLI: &'static str = "open-in-cli";
    const COPY_LATEST: &'static str = "copy-latest";
    const CHOOSE_MODEL: &'static str = "choose-model";
    const CHOOSE_WORKSPACE: &'static str = "choose-workspace";
    const REFRESH_SESSIONS: &'static str = "refresh-sessions";
    const REFRESH_FILES: &'static str = "refresh-files";
    const BROWSE_THREADS: &'static str = "browse-threads";
    const TOGGLE_INSPECTOR: &'static str = "toggle-inspector";
    const TOGGLE_ZEN: &'static str = "toggle-zen";
    const TOGGLE_THEME: &'static str = "toggle-theme";
    const TOGGLE_COMPUTER_USE: &'static str = "toggle-computer-use";
    const ATTACH_FILES: &'static str = "attach-files";
    const LOAD_SESSION: &'static str = "load-session";
    const PREVIEW_FILE: &'static str = "preview-file";
    const ATTACH_FILE: &'static str = "attach-file";
    const MENTION_FILE: &'static str = "mention-file";
    const REVIEW_FILE: &'static str = "review-file";
    const EDIT_CONFIG: &'static str = "edit-config";
    const SLASH: &'static str = "slash";
    const SELECT_MODEL: &'static str = "select-model";
    const JUMP: &'static str = "jump";
    const OUTPUT: &'static str = "output";
    const FONT: &'static str = "font";
}

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
            Self::Commands => Verb::COMMANDS,
            Self::ConversationSearch => Verb::CONVERSATION_SEARCH,
            Self::Models => Verb::MODELS,
            Self::ModelOptions => Verb::MODEL_OPTIONS,
            Self::AddContext => Verb::ADD_CONTEXT,
            Self::Permissions => Verb::PERMISSIONS,
            Self::Modes => Verb::MODES,
            Self::Fonts(FontTarget::Ui) => Verb::FONTS_UI,
            Self::Fonts(FontTarget::Code) => Verb::FONTS_CODE,
            Self::SettingsSearch => Verb::SETTINGS_SEARCH,
            Self::ConfigSettings => Verb::CONFIG_SETTINGS,
            Self::PromptHistory => Verb::PROMPT_HISTORY,
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
    /// The changelog viewer for the installed binary's release notes. A
    /// cached but uninstalled release is an offer, not reader content. A
    /// viewer, not a palette action, so it bypasses [`PaletteAction`].
    Changelog,
}

/// Click destination for a notification banner. Owned so the banner callback
/// (`Fn + 'static`) can carry it past the notifying event; resolved by
/// [`DesktopView::open_notification_target`], which tolerates threads the
/// user has since left and interactions that have since resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NotificationTarget {
    /// A thread that finished or needs attention.
    Thread { session_id: String },
    /// A waiting approval/question; focuses the interaction when still current.
    Interaction { session_id: String, interaction_id: String },
    /// Service disconnect: land on the workspace status.
    Workspace,
    /// New release: the About settings tab (Update status row + Check /
    /// Install buttons). The offer is an action, not a readable changelog,
    /// so the notification never opens the viewer for an uninstalled release.
    UpdateSettings,
}

impl NotificationTarget {
    /// Canonical deeplink address for this target, so every notification
    /// destination round-trips through the same `verb/path` spelling `/go`
    /// accepts. Address only: clicks still resolve through
    /// [`DesktopView::open_notification_target`], which carries the
    /// stale-tolerance (thread re-select, focus-only-if-current) a bare
    /// deeplink lacks. An Interaction degrades to its thread link; the
    /// waiting-prompt focus has no link form.
    #[cfg(test)]
    fn to_deeplink(&self) -> DeepLink {
        match self {
            Self::Thread { session_id } | Self::Interaction { session_id, .. } => {
                DeepLink::Action(PaletteAction::LoadSession(session_id.clone()))
            }
            Self::Workspace => DeepLink::Route(Route::Workspace),
            Self::UpdateSettings => DeepLink::SettingsTab(settings::SettingsTab::About),
        }
    }

    /// Canonical link text for this target. Test-only contract pin: every
    /// target must stay addressable through the same `verb/path` spelling
    /// `/go` accepts. Clicks resolve through
    /// [`DesktopView::open_notification_target`], not this address.
    #[cfg(test)]
    pub(super) fn path(&self) -> Option<String> {
        self.to_deeplink().path()
    }
}

/// Shared banner/toast click handler: carry an owned target behind a weak
/// view so clicks land after thread switches and view teardown alike. One
/// construction for the agent-activity, update, and turn-undo emitters.
pub(super) fn click_handler(
    target: NotificationTarget,
    view: WeakEntity<DesktopView>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        let _updated = view.update(cx, |view, cx| {
            view.open_notification_target(target.clone(), window, cx);
        });
    }
}

impl DeepLink {
    /// Canonical link text, or `None` for actions no link addresses
    /// (free-text inserts, composite session options, palette openers).
    pub(super) fn path(&self) -> Option<String> {
        match self {
            Self::Route(route) => Some(
                match route {
                    Route::Workspace => Verb::WORKSPACE,
                    Route::Settings => Verb::SETTINGS,
                    Route::Browser => Verb::BROWSER,
                    Route::Review => Verb::REVIEW,
                }
                .to_string(),
            ),
            Self::SettingsTab(tab) => Some(format!("{}/{}", Verb::SETTINGS, tab.as_str())),
            Self::ConfigPage => Some(Verb::CONFIG.to_string()),
            Self::ConfigPath(path) => Some(format!("{}/{}", Verb::CONFIG, path)),
            Self::Palette(kind) => Some(format!("{}/{}", Verb::PALETTE, kind.verb())),
            Self::Command(id) => Some(format!("{}/{}", Verb::COMMAND, id.as_str())),
            Self::Action(action) => action_path(action),
            Self::Changelog => Some(Verb::CHANGELOG.to_string()),
        }
    }
}

/// Inverse of [`parse_action`] for the addressable subset, plus the
/// overlapping tab/config actions in their canonical `settings/` and
/// `config/` forms. Free-text inserts, composite session options, and the
/// deeplink openers themselves have no link form.
fn action_path(action: &PaletteAction) -> Option<String> {
    match action {
        PaletteAction::NewSession => Some(format!("action/{}", Verb::NEW_SESSION)),
        PaletteAction::CloseSession => Some(format!("action/{}", Verb::CLOSE_SESSION)),
        PaletteAction::CopyThreadTitle => Some(format!("action/{}", Verb::COPY_THREAD_TITLE)),
        PaletteAction::CopyFilePath => Some(format!("action/{}", Verb::COPY_FILE_PATH)),
        PaletteAction::CloseOtherTabs => Some(format!("action/{}", Verb::CLOSE_OTHER_TABS)),
        PaletteAction::OpenInCli => Some(format!("action/{}", Verb::OPEN_IN_CLI)),
        PaletteAction::CopyLatestMessage => Some(format!("action/{}", Verb::COPY_LATEST)),
        PaletteAction::ChooseModel => Some(format!("action/{}", Verb::CHOOSE_MODEL)),
        PaletteAction::ChooseWorkspace => Some(format!("action/{}", Verb::CHOOSE_WORKSPACE)),
        PaletteAction::RefreshSessions => Some(format!("action/{}", Verb::REFRESH_SESSIONS)),
        PaletteAction::RefreshFiles => Some(format!("action/{}", Verb::REFRESH_FILES)),
        PaletteAction::BrowseThreads => Some(format!("action/{}", Verb::BROWSE_THREADS)),
        PaletteAction::ToggleInspector => Some(format!("action/{}", Verb::TOGGLE_INSPECTOR)),
        PaletteAction::ToggleZenMode => Some(format!("action/{}", Verb::TOGGLE_ZEN)),
        PaletteAction::ToggleTheme => Some(format!("action/{}", Verb::TOGGLE_THEME)),
        PaletteAction::ToggleComputerUse => Some(format!("action/{}", Verb::TOGGLE_COMPUTER_USE)),
        PaletteAction::AttachFiles => Some(format!("action/{}", Verb::ATTACH_FILES)),
        PaletteAction::OpenConversationSearch { session_id } => {
            Some(format!("action/{}/{session_id}", Verb::CONVERSATION_SEARCH))
        }
        PaletteAction::JumpToTranscriptItem { session_id, item_id, .. } => {
            Some(format!("action/{}/{session_id}/{item_id}", Verb::JUMP))
        }
        PaletteAction::ShowActivityOutput { session_id, item_id } => {
            Some(format!("action/{}/{session_id}/{item_id}", Verb::OUTPUT))
        }
        PaletteAction::SelectFont { target, family } => {
            let target = match target {
                FontTarget::Ui => Verb::UI,
                FontTarget::Code => Verb::CODE,
            };
            match family {
                Some(family) => Some(format!("action/{}/{target}/{family}", Verb::FONT)),
                None => Some(format!("action/{}/{target}", Verb::FONT)),
            }
        }
        PaletteAction::SelectSettingsTab(tab) => Some(format!("{}/{}", Verb::SETTINGS, tab.as_str())),
        PaletteAction::EditConfigSetting(path) => Some(format!("{}/{}", Verb::CONFIG, path)),
        PaletteAction::LoadSession(session_id) => Some(format!("action/{}/{session_id}", Verb::LOAD_SESSION)),
        PaletteAction::PreviewFile(path) => Some(format!("action/{}/{path}", Verb::PREVIEW_FILE)),
        PaletteAction::SelectModel { provider, model } => {
            Some(format!("action/{}/{provider}/{model}", Verb::SELECT_MODEL))
        }
        PaletteAction::AttachFile(path) => Some(format!("action/{}/{path}", Verb::ATTACH_FILE)),
        PaletteAction::InsertFileMention(path) => Some(format!("action/{}/{path}", Verb::MENTION_FILE)),
        PaletteAction::CompleteSlashCommand(name) => Some(format!("action/{}/{name}", Verb::SLASH)),
        PaletteAction::SelectWorkspaceReviewFile(path) => Some(format!("action/{}/{path}", Verb::REVIEW_FILE)),
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
        Verb::WORKSPACE => exact(tail, DeepLink::Route(Route::Workspace)),
        Verb::BROWSER => exact(tail, DeepLink::Route(Route::Browser)),
        Verb::REVIEW => exact(tail, DeepLink::Route(Route::Review)),
        Verb::CHANGELOG => exact(tail, DeepLink::Changelog),
        Verb::SETTINGS => parse_settings(tail),
        Verb::CONFIG => parse_config(tail),
        Verb::PALETTE => parse_palette(tail),
        Verb::COMMAND => parse_command(tail),
        Verb::ACTION => parse_action(tail),
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
    settings::SettingsTab::from_name(&tail.to_ascii_lowercase()).map(DeepLink::SettingsTab)
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
        Verb::COMMANDS => exact(rest, DeepLink::Palette(PaletteLink::Commands)),
        Verb::CONVERSATION_SEARCH => exact(rest, DeepLink::Palette(PaletteLink::ConversationSearch)),
        Verb::MODELS => exact(rest, DeepLink::Palette(PaletteLink::Models)),
        Verb::MODEL_OPTIONS => exact(rest, DeepLink::Palette(PaletteLink::ModelOptions)),
        Verb::ADD_CONTEXT => exact(rest, DeepLink::Palette(PaletteLink::AddContext)),
        Verb::PERMISSIONS => exact(rest, DeepLink::Palette(PaletteLink::Permissions)),
        Verb::MODES => exact(rest, DeepLink::Palette(PaletteLink::Modes)),
        Verb::SETTINGS_SEARCH => exact(rest, DeepLink::Palette(PaletteLink::SettingsSearch)),
        Verb::CONFIG_SETTINGS => exact(rest, DeepLink::Palette(PaletteLink::ConfigSettings)),
        Verb::PROMPT_HISTORY => exact(rest, DeepLink::Palette(PaletteLink::PromptHistory)),
        Verb::FONTS => match rest.to_ascii_lowercase().as_str() {
            Verb::UI => Some(DeepLink::Palette(PaletteLink::Fonts(FontTarget::Ui))),
            Verb::CODE => Some(DeepLink::Palette(PaletteLink::Fonts(FontTarget::Code))),
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
        Verb::NEW_SESSION => exact(rest, PaletteAction::NewSession)?,
        Verb::CLOSE_SESSION => exact(rest, PaletteAction::CloseSession)?,
        Verb::COPY_THREAD_TITLE => exact(rest, PaletteAction::CopyThreadTitle)?,
        Verb::COPY_FILE_PATH => exact(rest, PaletteAction::CopyFilePath)?,
        Verb::CLOSE_OTHER_TABS => exact(rest, PaletteAction::CloseOtherTabs)?,
        Verb::OPEN_IN_CLI => exact(rest, PaletteAction::OpenInCli)?,
        Verb::COPY_LATEST => exact(rest, PaletteAction::CopyLatestMessage)?,
        Verb::CHOOSE_MODEL => exact(rest, PaletteAction::ChooseModel)?,
        Verb::CHOOSE_WORKSPACE => exact(rest, PaletteAction::ChooseWorkspace)?,
        Verb::REFRESH_SESSIONS => exact(rest, PaletteAction::RefreshSessions)?,
        Verb::REFRESH_FILES => exact(rest, PaletteAction::RefreshFiles)?,
        Verb::BROWSE_THREADS => exact(rest, PaletteAction::BrowseThreads)?,
        Verb::TOGGLE_INSPECTOR => exact(rest, PaletteAction::ToggleInspector)?,
        Verb::TOGGLE_ZEN => exact(rest, PaletteAction::ToggleZenMode)?,
        Verb::TOGGLE_THEME => exact(rest, PaletteAction::ToggleTheme)?,
        Verb::TOGGLE_COMPUTER_USE => exact(rest, PaletteAction::ToggleComputerUse)?,
        Verb::ATTACH_FILES => exact(rest, PaletteAction::AttachFiles)?,
        Verb::CONVERSATION_SEARCH => PaletteAction::OpenConversationSearch { session_id: single(rest)? },
        Verb::LOAD_SESSION => PaletteAction::LoadSession(single(rest)?),
        Verb::PREVIEW_FILE => PaletteAction::PreviewFile(path_payload(rest)?),
        Verb::ATTACH_FILE => PaletteAction::AttachFile(path_payload(rest)?),
        Verb::MENTION_FILE => PaletteAction::InsertFileMention(path_payload(rest)?),
        Verb::REVIEW_FILE => PaletteAction::SelectWorkspaceReviewFile(path_payload(rest)?),
        Verb::EDIT_CONFIG => PaletteAction::EditConfigSetting(path_payload(rest)?),
        Verb::SLASH => PaletteAction::CompleteSlashCommand(single(rest)?),
        Verb::SELECT_MODEL => {
            let (provider, model) = pair(rest)?;
            PaletteAction::SelectModel { provider, model }
        }
        Verb::JUMP => {
            let (session_id, item_id) = pair(rest)?;
            PaletteAction::JumpToTranscriptItem { session_id, item_id, from_search: false }
        }
        Verb::OUTPUT => {
            let (session_id, item_id) = pair(rest)?;
            PaletteAction::ShowActivityOutput { session_id, item_id }
        }
        Verb::FONT => {
            let (target, family) = match rest.split_once('/') {
                Some((target, family)) => (target, Some(family)),
                None => (rest, None),
            };
            let target = match target.to_ascii_lowercase().as_str() {
                Verb::UI => FontTarget::Ui,
                Verb::CODE => FontTarget::Code,
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
                let reachable = self.palette_action_available(&action, cx);
                self.run_palette_action(action, window, cx);
                reachable
            }
            DeepLink::Changelog => {
                let reachable = self.can_view_any_changelog();
                self.open_changelog(window, cx);
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

    /// Open a notification click destination. Stale targets degrade to the
    /// workspace: a resolved interaction still lands on its thread rather
    /// than failing, and a switched thread is re-selected through the UI
    /// entry so a running turn gets the confirm modal instead of a yank.
    pub(super) fn open_notification_target(
        &mut self,
        target: NotificationTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            NotificationTarget::Thread { session_id } => {
                self.ensure_notification_session(&session_id, window, cx);
                self.open_route_link(Route::Workspace, window, cx);
            }
            NotificationTarget::Interaction { session_id, interaction_id } => {
                self.ensure_notification_session(&session_id, window, cx);
                self.open_route_link(Route::Workspace, window, cx);
                if self
                    .interaction
                    .as_ref()
                    .is_some_and(|current| current.interaction_id == interaction_id)
                {
                    self.interaction_focus.focus(window, cx);
                }
            }
            NotificationTarget::Workspace => self.open_route_link(Route::Workspace, window, cx),
            NotificationTarget::UpdateSettings => {
                self.run_palette_action(PaletteAction::SelectSettingsTab(settings::SettingsTab::About), window, cx);
            }
        }
    }

    /// Re-select the notified thread when the user has since moved elsewhere.
    fn ensure_notification_session(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_session_id.as_deref() != Some(session_id) {
            let _loaded = self.load_session_from_ui(session_id.to_string(), window, cx);
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
            ("about", SettingsTab::About),
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
        assert_eq!(parse("action/toggle-computer-use"), Some(DeepLink::Action(PaletteAction::ToggleComputerUse)));
        assert_eq!(parse("settings/computer-use"), Some(DeepLink::SettingsTab(SettingsTab::ComputerUse)));
        assert_eq!(parse("action/open-in-cli"), Some(DeepLink::Action(PaletteAction::OpenInCli)));
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
                from_search: false,
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
            DeepLink::Action(PaletteAction::CopyThreadTitle),
            DeepLink::Action(PaletteAction::CopyFilePath),
            DeepLink::Action(PaletteAction::CloseOtherTabs),
            DeepLink::Changelog,
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

    #[test]
    fn notification_target_paths_round_trip_through_parse() {
        let targets = [
            NotificationTarget::Thread { session_id: "thread-a".into() },
            NotificationTarget::Interaction {
                session_id: "thread-a".into(),
                interaction_id: "prompt-1".into(),
            },
            NotificationTarget::Workspace,
            NotificationTarget::UpdateSettings,
        ];
        for target in targets {
            let path = target.path().expect("every target has a link form");
            let link = parse(&path).expect("target paths parse as deeplinks");
            assert_eq!(link.path().as_deref(), Some(path.as_str()), "re-parsed link keeps its spelling");
        }
        // Asymmetric by design: an interaction shares its thread link (the
        // waiting-prompt focus has no link form), while workspace and update
        // settings address distinctly.
        assert_eq!(
            NotificationTarget::Thread { session_id: "s".into() }.path(),
            NotificationTarget::Interaction { session_id: "s".into(), interaction_id: "i".into() }.path()
        );
        assert_eq!(
            NotificationTarget::Thread { session_id: "s".into() }.path().as_deref(),
            Some("action/load-session/s")
        );
        assert_eq!(NotificationTarget::Workspace.path().as_deref(), Some("workspace"));
        // The update toast lands on About → Update status, never on the
        // changelog viewer for a release that is not installed yet.
        assert_eq!(NotificationTarget::UpdateSettings.path().as_deref(), Some("settings/about"));
    }
}
