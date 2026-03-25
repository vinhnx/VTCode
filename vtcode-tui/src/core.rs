use std::path::PathBuf;
use std::sync::Arc;

use crate::config::KeyboardProtocolConfig;
use crate::core_tui;

pub use crate::core_tui::session::config::AppearanceConfig as SessionAppearanceConfig;
pub use crate::core_tui::session::config::{LayoutModeOverride, ReasoningDisplayMode, UiMode};
pub use crate::core_tui::session::mouse_selection::MouseSelectionState;
pub use crate::core_tui::style::{convert_style, theme_from_styles};
pub use crate::core_tui::theme_parser::ThemeConfigParser;
pub use crate::core_tui::types::{
    ContentPart, EditingMode, FocusChangeCallback, InlineEventCallback, InlineHeaderContext,
    InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone, InlineLinkRange,
    InlineLinkTarget, InlineListItem, InlineListSearchConfig, InlineListSelection,
    InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme, ListOverlayRequest,
    ModalOverlayRequest, OpenAIServiceTierChoice, OverlayEvent, OverlayHotkey, OverlayHotkeyAction,
    OverlayHotkeyKey, OverlayRequest, OverlaySelectionChange, OverlaySubmission, RewindAction,
    SecurePromptConfig, WizardModalMode, WizardOverlayRequest, WizardStep,
};
pub use crate::options::{KeyboardProtocolSettings, SessionSurface};

pub type CoreCommand = core_tui::types::InlineCommand;
pub type CoreEvent = core_tui::types::InlineEvent;
pub type CoreHandle = core_tui::types::InlineHandle;
pub type CoreSession = core_tui::types::InlineSession;

/// Core session launch options for reusable TUI integrations.
#[derive(Clone)]
pub struct CoreSessionOptions {
    pub placeholder: Option<String>,
    pub surface_preference: SessionSurface,
    pub inline_rows: u16,
    pub event_callback: Option<InlineEventCallback>,
    pub focus_callback: Option<FocusChangeCallback>,
    pub active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    pub input_activity_counter: Option<Arc<std::sync::atomic::AtomicU64>>,
    pub keyboard_protocol: KeyboardProtocolSettings,
    pub workspace_root: Option<PathBuf>,
    pub appearance: Option<SessionAppearanceConfig>,
    pub app_name: String,
    pub non_interactive_hint: Option<String>,
}

impl Default for CoreSessionOptions {
    fn default() -> Self {
        Self {
            placeholder: None,
            surface_preference: SessionSurface::Auto,
            inline_rows: crate::config::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS,
            event_callback: None,
            focus_callback: None,
            active_pty_sessions: None,
            input_activity_counter: None,
            keyboard_protocol: KeyboardProtocolSettings::default(),
            workspace_root: None,
            appearance: None,
            app_name: "Agent TUI".to_string(),
            non_interactive_hint: None,
        }
    }
}

/// Spawn a core session using standalone options.
pub fn spawn_core_session(
    theme: InlineTheme,
    options: CoreSessionOptions,
) -> anyhow::Result<CoreSession> {
    core_tui::spawn_session_with_prompts_and_options(
        theme,
        options.placeholder,
        options.surface_preference.into(),
        options.inline_rows,
        options.event_callback,
        options.focus_callback,
        options.active_pty_sessions,
        options.input_activity_counter,
        KeyboardProtocolConfig::from(options.keyboard_protocol),
        options.workspace_root,
        options.appearance,
        options.app_name,
        options.non_interactive_hint,
    )
}

/// Commonly used core TUI API items.
pub mod prelude {
    pub use super::{
        CoreCommand, CoreEvent, CoreHandle, CoreSession, CoreSessionOptions, InlineHeaderContext,
        InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone, InlineMessageKind,
        InlineSegment, InlineTextStyle, InlineTheme, KeyboardProtocolSettings, LayoutModeOverride,
        ListOverlayRequest, ModalOverlayRequest, OverlayEvent, OverlayHotkey, OverlayHotkeyAction,
        OverlayHotkeyKey, OverlayRequest, OverlaySelectionChange, OverlaySubmission,
        ReasoningDisplayMode, SessionAppearanceConfig, SessionSurface, UiMode, WizardModalMode,
        WizardOverlayRequest, WizardStep, convert_style, spawn_core_session, theme_from_styles,
    };
}
