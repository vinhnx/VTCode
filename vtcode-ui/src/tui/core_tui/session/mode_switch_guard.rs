//! Shared guard that locks primary-agent cycling / planning-mode switches while a
//! turn is actively processing.
//!
//! The app session and the inline session each implement [`ModeSwitchGuardSession`]
//! so the lock behavior lives in exactly one place behind a strict per-session
//! interface. The only legitimate per-session difference is how a warning is flushed
//! to the transcript, which is isolated in [`ModeSwitchGuardSession::notify_mode_switch_busy`].

use ratatui::crossterm::event::KeyEvent;

/// Warning shown when a mode switch (primary-agent cycle or planning workflow)
/// is requested while a turn is actively processing.
pub(crate) const MODE_SWITCH_BUSY_NOTICE: &str = "Mode switching is disabled while the agent is processing. It will be available once this turn finishes.";

/// Minimal surface a session must expose for the shared mode-switch guard.
///
/// Implementing this lets [`try_cycle_primary_agent`] lock mode switches without
/// depending on either concrete session type.
pub(crate) trait ModeSwitchGuardSession {
    /// Whether a turn is currently processing and mode switches must be locked.
    fn is_running_activity(&self) -> bool;

    /// Whether the given key may cycle the primary agent (ignoring activity).
    fn can_cycle_primary_agent(&self, key: &KeyEvent) -> bool;

    /// Render the "mode switch busy" warning using the session's own flush path.
    fn notify_mode_switch_busy(&mut self);
}

/// Returns `true` when the user may cycle the primary agent right now.
///
/// While a turn is processing, mode switching is locked: the request is dropped
/// and a notice is shown instead, and this returns `false`. The caller is
/// responsible for constructing the cycle event, since the concrete `InlineEvent`
/// type differs between the app session and the inline session.
pub(crate) fn try_cycle_primary_agent<S: ModeSwitchGuardSession>(
    session: &mut S,
    key: &KeyEvent,
) -> bool {
    if session.is_running_activity() {
        session.notify_mode_switch_busy();
        return false;
    }
    session.can_cycle_primary_agent(key)
}
