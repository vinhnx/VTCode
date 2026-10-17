//! Inline-event emission and mouse-scroll helpers.

use super::*;

/// Emits an InlineEvent through the event channel and callback
#[inline]
#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) fn emit_inline_event(
    event: &InlineEvent,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    if let Some(cb) = callback {
        cb(event);
    }
    let _ = events.send(event.clone());
}

/// Handles scroll down event from mouse input
#[inline]
pub(crate) fn handle_scroll_down(
    session: &mut Session,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    session.scroll_line_down();
    session.mark_dirty();
    emit_inline_event(&InlineEvent::ScrollLineDown, events, callback);
}

/// Handles scroll up event from mouse input
#[inline]
pub(crate) fn handle_scroll_up(
    session: &mut Session,
    events: &UnboundedSender<InlineEvent>,
    callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
) {
    session.scroll_line_up();
    session.mark_dirty();
    emit_inline_event(&InlineEvent::ScrollLineUp, events, callback);
}
