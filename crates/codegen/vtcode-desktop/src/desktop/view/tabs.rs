//! Open-thread tabs: quick-switch strip above the conversation.
//!
//! Tabs are lightweight shortcuts over loaded sessions. The sidebar stays the
//! source of truth for recents; tabs track what is open in this window.
//! Closing a tab only untabs the session — server-side state is untouched.
//! Tab-scoped menu items dispatch payload actions (the first in this crate)
//! because popup menus cannot run view callbacks directly.

use super::session_nav::step_highlight_order;
use super::*;
use gpui_kit::Action;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::PopupMenu;

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct CloseThreadTab {
    pub session_id: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct CloseOtherThreadTabs {
    pub session_id: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct CopyThreadTitle {
    pub session_id: gpui_kit::SharedString,
}

#[derive(Action, Clone, PartialEq, Debug)]
#[action(namespace = desktop, no_json)]
pub(super) struct DeleteThread {
    pub session_id: gpui_kit::SharedString,
}

/// Right-click menu shared by every session-row surface (sidebar recents,
/// thread browser): copy the title, close the tab when tabbed, delete the
/// thread. The tab strip keeps its own menu: it adds "Close other tabs".
pub(super) fn session_row_menu(menu: PopupMenu, session_id: &str, tabbed: bool) -> PopupMenu {
    menu.menu("Copy thread title", Box::new(CopyThreadTitle { session_id: session_id.into() }))
        .menu_with_disabled("Close tab", Box::new(CloseThreadTab { session_id: session_id.into() }), !tabbed)
        .separator()
        .menu("Delete thread…", Box::new(DeleteThread { session_id: session_id.into() }))
}

/// Items for one thread tab's right-click menu. Kit gives `Tab` no wrapper
/// to own a menu (`.context_menu()` returns a `ContextMenu<Tab>` the bar
/// cannot take back), so the tab-surface trigger and the close button each
/// carry these items and together cover the whole tab.
fn thread_tab_menu(menu: PopupMenu, session_id: &str) -> PopupMenu {
    menu.menu("Close tab", Box::new(CloseThreadTab { session_id: session_id.into() }))
        .menu("Close other tabs", Box::new(CloseOtherThreadTabs { session_id: session_id.into() }))
        .separator()
        .menu("Copy thread title", Box::new(CopyThreadTitle { session_id: session_id.into() }))
}

impl DesktopView {
    /// Track a freshly selected session as an open tab. Order is first-open;
    /// selection stays in [`DesktopView::selected_session_id`]. Every
    /// selection path funnels through here, so this is also where the strip
    /// reveals the tab: Kit only reveals on clicks, and `scroll_to_item`
    /// scrolls minimally, making this a no-op for fully visible tabs.
    pub(super) fn ensure_tab(&mut self, session_id: String) {
        if !self.open_tabs.iter().any(|tab| tab == &session_id) {
            self.open_tabs.push(session_id.clone());
        }
        if let Some(position) = self.open_tabs.iter().position(|tab| tab == &session_id) {
            self.tab_strip_scroll.scroll_to_item(position);
        }
    }

    /// Forget one tab without touching the session itself.
    pub(super) fn remove_tab(&mut self, session_id: &str) {
        self.open_tabs.retain(|tab| tab != session_id);
    }

    /// Drop sessions that no longer exist from the strip.
    pub(super) fn prune_tabs(&mut self) {
        self.open_tabs
            .retain(|tab| self.sessions.iter().any(|session| session.session_id == *tab));
    }

    /// Close one tab. Closing the selected tab loads the nearest remaining
    /// neighbor (next first, else previous); when the load is refused the tab
    /// stays so the selection remains reachable. Mid-turn the neighbor switch
    /// confirms first and the tab untabs once the deferred load is sent.
    pub(super) fn close_tab(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let position = self.open_tabs.iter().position(|tab| tab == session_id);
        if self.selected_session_id.as_deref() == Some(session_id)
            && let Some(index) = position
        {
            let mut next: Option<String> = None;
            let mut previous: Option<String> = None;
            for (candidate_index, tab) in self.open_tabs.iter().enumerate() {
                if tab == session_id {
                    continue;
                }
                if candidate_index > index {
                    next = Some(tab.clone());
                    break;
                }
                previous = Some(tab.clone());
            }
            if let Some(neighbor) = next.or(previous) {
                if !self.session_change_allowed(Some(&neighbor)) && self.running_switch_available() {
                    self.open_running_confirm(
                        RunningSwitchIntent::LoadSession {
                            target: neighbor,
                            untab: vec![session_id.to_string()],
                        },
                        window,
                        cx,
                    );
                    return;
                }
                if !self.load_session(neighbor, cx) {
                    return;
                }
            }
        }
        self.remove_tab(session_id);
        cx.notify();
    }

    /// Close every tab except one, selecting it when it is not already.
    /// Mid-turn the selection switch confirms first and the other tabs untab
    /// once the deferred load is sent.
    pub(super) fn close_other_tabs(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_tabs.iter().any(|tab| tab == session_id) {
            return;
        }
        if self.selected_session_id.as_deref() != Some(session_id) {
            if !self.session_change_allowed(Some(session_id)) && self.running_switch_available() {
                let untab = self
                    .open_tabs
                    .iter()
                    .filter(|tab| tab.as_str() != session_id)
                    .cloned()
                    .collect::<Vec<_>>();
                self.open_running_confirm(
                    RunningSwitchIntent::LoadSession { target: session_id.to_string(), untab },
                    window,
                    cx,
                );
                return;
            }
            if !self.load_session(session_id.to_string(), cx) {
                return;
            }
        }
        self.open_tabs.retain(|tab| tab == session_id);
        cx.notify();
    }

    /// Copy one tab's display title to the clipboard.
    pub(super) fn copy_thread_title(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let title = self.tab_title(session_id);
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(title));
    }

    fn tab_title(&self, session_id: &str) -> String {
        let title = self
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .and_then(|session| session.title.clone());
        session_display_title(session_id, title.as_deref())
    }

    /// Move the keyboard highlight across open tabs (Left/Right with the
    /// strip focused), stepping the shared thread highlight in first-open
    /// order. Clamps at both ends; the strip reveals the highlight when it
    /// would sit outside the visible tabs.
    pub(super) fn move_tab_focus(&mut self, next: bool, cx: &mut Context<Self>) {
        let target = {
            let order: Vec<&str> = self.open_tabs.iter().map(String::as_str).collect();
            step_highlight_order(&order, self.keyboard_focused_session.as_deref(), next)
                .and_then(|position| order.get(position).map(|id| (position, id.to_string())))
        };
        let Some((position, session_id)) = target else {
            return;
        };
        self.keyboard_focused_session = Some(session_id);
        self.tab_strip_scroll.scroll_to_item(position);
        cx.notify();
    }

    /// Jump the tab highlight to the first/last open tab (Home/End).
    pub(super) fn move_tab_focus_edge(&mut self, first: bool, cx: &mut Context<Self>) {
        let target = if first {
            self.open_tabs.first().map(|id| (0, id.clone()))
        } else {
            let last = self.open_tabs.len().saturating_sub(1);
            self.open_tabs.last().map(|id| (last, id.clone()))
        };
        let Some((position, session_id)) = target else {
            return;
        };
        self.keyboard_focused_session = Some(session_id);
        self.tab_strip_scroll.scroll_to_item(position);
        cx.notify();
    }

    /// Load the highlighted tab (Enter). The strip is a subset view: a
    /// highlight left on an untabbed thread is position memory for the
    /// thread lists, not a visible tab action, so Enter only fires when
    /// the highlight names an open tab.
    pub(super) fn activate_focused_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self
            .keyboard_focused_session
            .as_deref()
            .is_some_and(|id| self.open_tabs.iter().any(|tab| tab == id));
        if visible {
            self.activate_focused_session(window, cx);
        }
    }

    /// Horizontal quick-switch strip: one folder tab per open thread with a
    /// close affordance, a trailing new-thread button, an overflow menu, and
    /// a right-click menu per tab. Rendered only while more than one tab is
    /// open; zero/single-tab windows use the compact thread header instead.
    /// Tabs take no keyboard focus themselves (Kit contract), so the strip
    /// keeps its single tab stop while Left/Right/Home/End rove the shared
    /// thread highlight. The bar sits flush on the canvas so the selected
    /// tab joins the conversation below it.
    pub(super) fn render_thread_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let selected_position = self
            .selected_session_id
            .as_deref()
            .and_then(|id| self.open_tabs.iter().position(|tab| tab == id));
        // No width cap (Kit default): tabs keep their natural width and the
        // strip scrolls with an overflow menu. Clipped close buttons stay
        // clickable through explicit reveal — keyboard moves reveal the
        // highlight, and tests scroll via click_thread_tab_close.
        let entries = self
            .open_tabs
            .iter()
            .map(|id| (id.clone(), self.tab_title(id)))
            .collect::<Vec<_>>();
        let current = self.selected_session_id.clone();
        let weak_view = cx.entity().downgrade();
        let overflow = desktop_button("thread-tabs-menu")
            .ghost()
            .xsmall()
            .dropdown_caret(true)
            .accessibility_label("Open threads")
            .tooltip("Open threads")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.scrollable(true);
                for (id, title) in &entries {
                    let id = id.clone();
                    let view = weak_view.clone();
                    menu = menu.item(
                        gpui_kit::component::menu::PopupMenuItem::new(title.clone())
                            .checked(current.as_deref() == Some(id.as_str()))
                            .on_click(move |_, window, cx| {
                                if let Some(view) = view.upgrade() {
                                    view.update(cx, |view, cx| {
                                        if let Some(index) = view.open_tabs.iter().position(|tab| tab == &id) {
                                            // Immediate feedback; ensure_tab re-reveals once the load lands.
                                            view.tab_strip_scroll.scroll_to_item(index);
                                            view.keyboard_focused_session = Some(id.clone());
                                            let _ = view.load_session_from_ui(id.clone(), window, cx);
                                        }
                                    });
                                }
                            }),
                    );
                }
                menu
            })
            // Right-edge trigger: keep the TopRight anchor Kit's menu button
            // used so the popup opens leftward instead of past the window.
            .anchor(Anchor::TopRight);
        let mut bar = desktop_folder_tab_bar("thread-tabs-bar")
            .track_scroll(&self.tab_strip_scroll)
            .suffix(
                div().flex().items_center().child(overflow).child(
                    desktop_button("thread-tab-new")
                        .icon(IconName::Plus)
                        .ghost()
                        .small()
                        .accessibility_label("New thread")
                        .tooltip("New thread")
                        .desktop_disabled(!self.new_thread_available())
                        .on_click(cx.listener(|this, _, window, cx| {
                            let _ = this.create_session_from_ui(window, cx);
                        })),
                ),
            )
            .on_click(cx.listener(|view, index: &usize, window, cx| {
                let Some(session_id) = view.open_tabs.get(*index).cloned() else {
                    return;
                };
                // Clicking anchors the keyboard highlight as position memory,
                // like thread-list rows.
                view.keyboard_focused_session = Some(session_id.clone());
                let _ = view.load_session_from_ui(session_id, window, cx);
            }));
        if let Some(index) = selected_position {
            bar = bar.selected_index(index);
        }
        for session_id in &self.open_tabs {
            let id = session_id.clone();
            let title = self.tab_title(&id);
            let selected = self.selected_session_id.as_deref() == Some(id.as_str());
            let running = self.is_session_running(&id);
            let keyboard_focused = self.keyboard_focused_session.as_deref() == Some(id.as_str());
            let close_id = id.clone();
            let menu_id = id.clone();
            let trigger_menu_id = id.clone();
            // The rail slot is constant width and always mounted: the accent
            // marker only mounts while focused, so the highlight never
            // reflows the tab. It spans the icon lane rather than the full
            // tab — Kit owns the tab box, so a full-height overlay has no
            // anchor that survives clipping.
            let rail = div().w(px(2.)).self_stretch().when(keyboard_focused, |slot| {
                slot.child(
                    div()
                        .id(format!("thread-tab-focus-rail-{id}"))
                        .test_support()
                        .w_full()
                        .h_full()
                        .bg(colors.accent),
                )
            });
            // The Kit Tab carries the accessible name and selected state
            // (addressable by its index id); the overlay below keeps the
            // historic per-tab id for clicks and menus.
            let tab_name = if running {
                rust_i18n::t!(i18n::SESSION_RUNNING_THREAD_LABEL, thread = title.as_str()).to_string()
            } else if selected {
                format!("Current thread: {title}")
            } else {
                format!("Switch to thread: {title}")
            };
            let mut tab = Tab::new()
                .label(title.clone())
                .aria_label(tab_name)
                .aria_selected(selected)
                .when(keyboard_focused, |tab| tab.aria_active_descendant())
                .prefix(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(rail)
                        .child(Icon::new(IconName::MessageSquare).small()),
                )
                .suffix(
                    desktop_button(format!("thread-tab-close-{id}"))
                        .icon(IconName::X)
                        .ghost()
                        .xsmall()
                        .accessibility_label(format!("Close tab {title}"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // The close button nests inside the tab: stop the
                            // click here or it bubbles up and selects the tab.
                            cx.stop_propagation();
                            this.close_tab(&close_id, window, cx);
                        }))
                        // The surface trigger below sits under the suffix, so
                        // the close button carries the same menu items to keep
                        // right-click coverage whole-tab.
                        .context_menu(move |menu, _, _| thread_tab_menu(menu, &menu_id)),
                );
            if running {
                tab = tab.child(running_indicator(format!("thread-tab-running-{id}"), &colors));
            }
            // Transparent menu trigger over the tab body. It keeps the
            // historic per-tab test id: clicks bubble through to the tab, so
            // selection and right-click coverage stay id-addressable.
            tab = tab.child(
                div()
                    .id(format!("thread-tab-{id}"))
                    .test_support()
                    .absolute()
                    .inset_0()
                    .context_menu(move |menu, _, _| thread_tab_menu(menu, &trigger_menu_id)),
            );
            bar = bar.child(tab);
        }
        div()
            .id("thread-tabs")
            .test_support()
            .track_focus(&self.tab_strip_focus)
            .key_context(key_context::THREAD_TABS)
            .on_action(cx.listener(|view, _: &PreviousThreadTab, _, cx| view.move_tab_focus(false, cx)))
            .on_action(cx.listener(|view, _: &NextThreadTab, _, cx| view.move_tab_focus(true, cx)))
            .on_action(cx.listener(|view, _: &FirstThreadTab, _, cx| view.move_tab_focus_edge(true, cx)))
            .on_action(cx.listener(|view, _: &LastThreadTab, _, cx| view.move_tab_focus_edge(false, cx)))
            .on_action(cx.listener(|view, _: &OpenSelectedThreadTab, window, cx| view.activate_focused_tab(window, cx)))
            .w_full()
            .child(bar)
            .into_any_element()
    }
}
