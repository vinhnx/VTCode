/// Custom widget implementations following Ratatui best practices
///
/// This module contains reusable widget components that implement the Widget and WidgetRef traits.
/// These widgets enable better composition, testability, and separation of concerns.

pub mod header;
pub mod input;
pub mod modal;
pub mod palette;
pub mod session;
pub mod slash;
pub mod transcript;

pub use header::HeaderWidget;
pub use input::InputWidget;
pub use modal::{ModalWidget, ModalType};
pub use palette::{FilePaletteWidget, PromptPaletteWidget};
pub use session::SessionWidget;
pub use slash::SlashWidget;
pub use transcript::TranscriptWidget;
