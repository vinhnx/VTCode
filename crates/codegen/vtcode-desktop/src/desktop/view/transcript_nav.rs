//! Transcript navigation: jump-to-message, scroll-to-first, row anchors.
//!
//! Single home for every transcript jump so palette search, activity rows,
//! deep links, and the overlay buttons share one reveal path: resolve the
//! stable id to the live index, reveal collapsed tool rows, then
//! `scroll_to_item`. Row clicks anchor the highlight without scrolling.
//! MessageScroller owns virtual-list bookkeeping; this module owns VT Code's
//! id-to-index mapping and focus memory.

use super::*;

/// Snapshot of conversation-search matches for overlay cycling, in
/// transcript order with the current position. Snapshots never grow: rows
/// appended after the search join only a fresh search, and evicted rows are
/// skipped while cycling. Always non-empty by construction.
#[derive(Clone, Debug)]
pub(super) struct FilterMatches {
    ids: Vec<String>,
    index: usize,
}

impl DesktopView {
    /// Rows the renderer hides (collapsed activity members) are also skipped by
    /// keyboard navigation, so the highlight never lands on an invisible row.
    pub(super) fn transcript_row_visible(item: &TranscriptItem) -> bool {
        !(item.activity_grouped && !item.activity_group_head && !item.activity_expanded)
    }

    /// Move the keyboard highlight across messages (Up/Down with the
    /// transcript focused). Clamps at both ends and scrolls the row into view.
    pub(super) fn move_transcript_focus(&mut self, down: bool, cx: &mut Context<Self>) {
        let visible: Vec<usize> = self
            .transcript
            .items()
            .enumerate()
            .filter(|(_, item)| Self::transcript_row_visible(item))
            .map(|(index, _)| index)
            .collect();
        if visible.is_empty() {
            return;
        }
        let current = self.keyboard_focused_message.as_deref().and_then(|id| {
            visible
                .iter()
                .position(|&index| self.transcript.get(index).is_some_and(|item| item.id.as_str() == id))
        });
        let next = match (current, down) {
            (Some(position), true) => (position + 1).min(visible.len() - 1),
            (Some(position), false) => position.saturating_sub(1),
            (None, true) => 0,
            (None, false) => visible.len() - 1,
        };
        let Some(&index) = visible.get(next) else {
            return;
        };
        let Some(item) = self.transcript.get(index) else {
            return;
        };
        self.keyboard_focused_message = Some(item.id.clone());
        // Kit limitation: `scroll_to_item` pins to viewport top and the
        // scroller exposes no visible-range query, so reveal-only-if-outside
        // is unimplementable; every step recenters.
        self.message_scroller.update(cx, |state, cx| {
            let _scrolled = state.scroll_to_item(index, cx);
        });
        cx.notify();
    }

    /// Jump the transcript scroller to the row with this stable id.
    ///
    /// Collapsed tool rows are revealed first, then the scroll runs
    /// immediately: reveal queues no tail pause (its sole caller navigates at
    /// once), so nothing in the sync pass can clobber the jump. The immediate
    /// scroll uses current measurements; a deferred re-pin on the next frame
    /// settles the exact position once revealed rows are measured (matters
    /// for large collapsed groups). Frame callbacks never run in headless
    /// tests, so tests pin the synchronous base. The keyboard highlight
    /// anchors on the target as position memory. Returns whether the id
    /// resolved to a live row; unknown ids leave state untouched. Callers
    /// that cross sessions must guard the selection first.
    pub(super) fn jump_to_transcript_item(
        &mut self,
        item_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.transcript.position_by_id(item_id) else {
            return false;
        };
        if self.transcript.get(index).is_some_and(|item| item.is_tool()) {
            if self.transcript.reveal_tool_item(item_id).is_none() {
                return false;
            }
            self.keyboard_focused_message = Some(item_id.to_string());
            self.message_scroller.update(cx, |state, cx| {
                let _scrolled = state.scroll_to_item(index, cx);
            });
            cx.notify();
            let item_id = item_id.to_string();
            let session_id = self.selected_session_id.clone();
            cx.on_next_frame(window, move |view, _, cx| {
                if view.selected_session_id != session_id {
                    return;
                }
                if let Some(index) = view.transcript.position_by_id(&item_id) {
                    view.message_scroller.update(cx, |state, cx| {
                        let _scrolled = state.scroll_to_item(index, cx);
                    });
                }
            });
        } else {
            self.keyboard_focused_message = Some(item_id.to_string());
            self.message_scroller.update(cx, |state, cx| {
                let _scrolled = state.scroll_to_item(index, cx);
            });
        }
        true
    }

    /// Scroll to the first transcript row. A pure viewport move: unlike a
    /// jump it sets no anchor. Returns false when the transcript is empty.
    pub(super) fn scroll_to_first_message(&mut self, cx: &mut Context<Self>) -> bool {
        if self.transcript.is_empty() {
            return false;
        }
        self.message_scroller.update(cx, |state, cx| state.scroll_to_item(0, cx))
    }

    /// Snapshot the rows matching the conversation-search `query` for overlay
    /// cycling, anchored at `picked_id`. Case-insensitive substring over the
    /// same searchable roles the palette offers (title and body), across the
    /// full transcript rather than the palette's capped window. The picked
    /// row joins the set even when fuzzy-only so the cycle always contains
    /// the landing row. Blank queries clear the state: nothing to cycle.
    pub(super) fn remember_filter_matches(&mut self, query: &str, picked_id: &str) {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            self.filter_matches = None;
            return;
        }
        let mut ids: Vec<String> = self
            .transcript
            .items()
            .filter(|item| item.is_conversation_searchable())
            .filter(|item| item.title.to_lowercase().contains(&needle) || item.text.to_lowercase().contains(&needle))
            .map(|item| item.id.clone())
            .collect();
        if !ids.iter().any(|id| id == picked_id)
            && let Some(picked_pos) = self.transcript.position_by_id(picked_id)
        {
            let insert_at = ids
                .iter()
                .position(|id| self.transcript.position_by_id(id).is_some_and(|pos| pos > picked_pos))
                .unwrap_or(ids.len());
            ids.insert(insert_at, picked_id.to_string());
        }
        if ids.is_empty() {
            self.filter_matches = None;
            return;
        }
        let index = ids.iter().position(|id| id == picked_id).unwrap_or(0);
        self.filter_matches = Some(FilterMatches { ids, index });
    }

    /// Advance to the next filter match, skipping rows lost to eviction.
    /// Clears the state when nothing resolves. Returns whether a jump
    /// happened.
    pub(super) fn cycle_filter_match(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((start_index, len)) = self.filter_matches.as_ref().map(|matches| (matches.index, matches.ids.len()))
        else {
            return false;
        };
        for step in 1..=len {
            let index = (start_index + step) % len;
            let candidate = self.filter_matches.as_ref().and_then(|matches| matches.ids.get(index)).cloned();
            let Some(id) = candidate else {
                continue;
            };
            if self.jump_to_transcript_item(&id, window, cx) {
                if let Some(matches) = self.filter_matches.as_mut() {
                    matches.index = index;
                }
                return true;
            }
        }
        self.filter_matches = None;
        false
    }

    /// Dismiss the filter-match cycle (overlay dismiss control).
    pub(super) fn clear_filter_matches(&mut self) {
        self.filter_matches = None;
    }

    /// Top-centered transcript overlay row: scroll-to-first circle plus the
    /// filter-match pill and its dismiss control. Renders nothing when no
    /// control applies. Controls appear instantly (no enter transition) so
    /// reduced motion needs no gate.
    pub(super) fn render_transcript_overlays(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let first = self.scroll_to_first_button(cx);
        let (next_match, dismiss) = self.filter_match_buttons(cx);
        if first.is_none() && next_match.is_none() {
            return None;
        }
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top_2()
                .flex()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .children(first)
                        .children(next_match)
                        .children(dismiss),
                )
                .into_any_element(),
        )
    }

    /// Scroll-to-first control in the inspector jump-button shape: icon-only
    /// circle, label carried by tooltip and accessible name. Renders nothing
    /// at the top edge, when the transcript is empty, or before the first
    /// layout settles list readers.
    fn scroll_to_first_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.transcript.is_empty() || !self.transcript_layout_settled {
            return None;
        }
        if self.message_scroller.read(cx).logical_scroll_top().item_ix == 0 {
            return None;
        }
        let label = rust_i18n::t!("app.transcript.scroll_first").to_string();
        let colors = cx.theme().colors;
        Some(
            desktop_button("transcript-scroll-first")
                .icon(IconName::ArrowUp)
                .accessibility_label(label.clone())
                .tooltip(label)
                .secondary()
                .rounded(cx.theme().radius_full())
                .border_1()
                .border_color(colors.border)
                .text_color(colors.foreground)
                .on_click(cx.listener(|view, _, _, cx| {
                    let _scrolled = view.scroll_to_first_message(cx);
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    /// Filter-match cycling pill plus dismiss control. The pill carries the
    /// visible position and full accessible name; the dismiss control shows
    /// exactly when the pill does. Position-independent: unlike scroll
    /// navigation, an active filter shows at any scroll offset.
    fn filter_match_buttons(&self, cx: &mut Context<Self>) -> (Option<AnyElement>, Option<AnyElement>) {
        let Some(matches) = self.filter_matches.as_ref() else {
            return (None, None);
        };
        if matches.ids.is_empty() {
            return (None, None);
        }
        let position = matches.index + 1;
        let total = matches.ids.len();
        let name = rust_i18n::t!("app.transcript.filter_next", position = position, total = total).to_string();
        // Locale-independent numeric position (same digits in every language);
        // the accessible name above carries the translated meaning.
        let count = format!("{position}/{total}");
        let colors = cx.theme().colors;
        let next = desktop_button("transcript-filter-next")
            .icon(IconName::ArrowDown)
            .label(count)
            .accessibility_label(name.clone())
            .tooltip(name)
            .secondary()
            .small()
            .rounded(cx.theme().radius_full())
            .border_1()
            .border_color(colors.border)
            .text_color(colors.foreground)
            .on_click(cx.listener(|view, _, window, cx| {
                let _cycled = view.cycle_filter_match(window, cx);
                cx.notify();
            }))
            .into_any_element();
        let clear = rust_i18n::t!("app.transcript.filter_clear").to_string();
        let dismiss = desktop_button("transcript-filter-clear")
            .icon(IconName::X)
            .accessibility_label(clear.clone())
            .tooltip(clear)
            .ghost()
            .small()
            .on_click(cx.listener(|view, _, _, cx| {
                view.clear_filter_matches();
                cx.notify();
            }))
            .into_any_element();
        (Some(next), Some(dismiss))
    }
}
