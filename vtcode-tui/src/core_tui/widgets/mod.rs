/// Custom widget implementations following Ratatui best practices
///
/// This module contains reusable widget components that implement the Widget and WidgetRef traits.
/// These widgets enable better composition, testability, and separation of concerns.
///
/// ## Layout System
///
/// The UI is organized into a responsive panel-based layout:
/// - **Header**: Session identity and status (model, git, tokens)
/// - **Main**: Transcript area with optional sidebar (wide mode)
/// - **Footer**: Status line and contextual hints
///
/// The `LayoutMode` enum determines the layout variant based on terminal size:
/// - `Compact`: Minimal chrome for small terminals (< 80 cols)
/// - `Standard`: Default layout with borders and titles
/// - `Wide`: Enhanced layout with sidebar (>= 120 cols)
///
/// ## Visual Hierarchy
///
/// Panels use consistent styling via the `Panel` wrapper:
/// - Active panels have highlighted borders
/// - Inactive panels have dimmed borders
/// - Titles are shown in Standard/Wide modes only
pub mod footer;
pub mod header;
pub mod history_picker;
pub mod input;
pub mod layout_mode;
pub mod modal;
pub mod palette;
pub mod panel;
pub mod session;
pub mod sidebar;
pub mod slash;
pub mod transcript;

pub use footer::{FooterWidget, hints as footer_hints};
pub use header::HeaderWidget;
pub use history_picker::HistoryPickerWidget;
pub use input::InputWidget;
pub use layout_mode::LayoutMode;
pub use modal::{ModalType, ModalWidget};
pub use palette::FilePaletteWidget;
pub use panel::{Panel, PanelStyles};
pub use session::SessionWidget;
pub use sidebar::{SidebarSection, SidebarWidget};
pub use slash::SlashWidget;
pub use transcript::TranscriptWidget;
