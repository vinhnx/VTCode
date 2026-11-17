//! TUI protocol types and session interface.
//!
//! Re-exports the full app-layer protocol from `vtcode-ui` plus the shared
//! data types from `vtcode-commons`. Headless consumers depend only on
//! `vtcode-core` and never compile this crate.

// ── Shared data types (always available from vtcode-commons) ────────────────

pub use vtcode_commons::ui_protocol::{
    BusyEnterBehavior, CompactActivityMetadata, InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge,
    InlineHeaderStatusTone, InlineLinkRange, InlineLinkTarget, InlineListItem, InlineListSearchConfig,
    InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme, InlineTranscriptLine,
    LayoutModeOverride, PlanContent, PlanPhase, PlanStep, ReasoningDisplayMode, RewindAction, SecurePromptConfig,
    SessionSurface, SlashCommandItem, ThinkingBlockState, ToolOutputId, UiMode, WizardModalMode, WizardStep,
    convert_style, theme_from_color_fields,
};

pub use vtcode_commons::ui_protocol::KeyboardProtocolSettings;

// ── Full TUI re-exports ─────────────────────────────────────────────────────

pub use vtcode_ui::tui::app::*;
