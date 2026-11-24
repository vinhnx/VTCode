//! Composer caret policy: the persisted Blink/Steady choice and the shared
//! idle window that bounds the application-owned soft fade.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A focused caret's soft fade pulse stops after this much time with no edit
/// or cursor movement, leaving a solid caret with no animation frames. Shared
/// by the theme policy and the composer's bounded pulse.
pub(crate) const CARET_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether the text caret gently fades while active or stays solid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CaretBlinkPreference {
    /// Soft, bounded fade pulse on the focused composer.
    #[default]
    Blink,
    /// Solid caret, no pulse and no animation frames.
    Steady,
}

impl CaretBlinkPreference {
    pub(crate) fn blinks(self) -> bool {
        matches!(self, Self::Blink)
    }
}
