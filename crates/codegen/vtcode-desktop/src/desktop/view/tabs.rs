//! Open-thread tabs: quick-switch strip above the conversation.
//!
//! Tabs are lightweight shortcuts over loaded sessions. The sidebar stays the
//! source of truth for recents; tabs track what is open in this window.
//! Closing a tab only untabs the session — server-side state is untouched.
//! Tab-scoped menu items dispatch payload actions (the first in this crate)
//! because popup menus cannot run view callbacks directly.

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

impl DesktopView {
    /// Track a freshly selected session as an open tab. Order is first-open;
    /// selection stays in [`DesktopView::selected_session_id`].
    pub(super) fn ensure_tab(&mut self, session_id: String) {
        if !self.open_tabs.iter().any(|tab| tab == &session_id) {
            self.open_tabs.push(session_id);
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

    /// Horizontal quick-switch strip: one button per open tab with a close
    /// affordance, a trailing new-thread button, and a right-click menu per
    /// tab. Rendered only while more than one tab is open; zero/single-tab
    /// windows use the compact thread header instead.
    pub(super) fn render_thread_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let mut strip = div()
            .id("thread-tabs")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .w_full()
            .pb_1();
        for session_id in &self.open_tabs {
            let id = session_id.clone();
            let title = self.tab_title(&id);
            let selected = self.selected_session_id.as_deref() == Some(id.as_str());
            let running = self.is_session_running(&id);
            let select_id = id.clone();
            let close_id = id.clone();
            let menu_id = id.clone();
            strip = strip.child(
                div()
                    .id(format!("thread-tab-row-{id}"))
                    .flex()
                    .items_center()
                    .flex_shrink_1()
                    .min_w_0()
                    .max_w(px(220.))
                    .child(
                        desktop_button(format!("thread-tab-{id}"))
                            .icon(IconName::MessageSquare)
                            .ghost()
                            .toggled(selected)
                            .with_size(gpui_kit::component::Size::Medium)
                            .flex_1()
                            .min_w_0()
                            .accessibility_label(if running {
                                rust_i18n::t!(i18n::SESSION_RUNNING_THREAD_LABEL, thread = title.as_str()).to_string()
                            } else if selected {
                                format!("Current thread: {title}")
                            } else {
                                format!("Switch to thread: {title}")
                            })
                            .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            // The label is a child so the tab can carry body-size
                            // type: the kit's small/medium buttons pin text_sm,
                            // which reads thin and small for a tab title on a
                            // dark surface. Medium weight keeps the stems dense
                            // (smoother at this size) while the color stays
                            // inherited so hover/selected states keep working.
                            .child(div().min_w_0().truncate().text_base().font_medium().child(title.clone()))
                            .when(running, |button| {
                                button.child(running_indicator(format!("thread-tab-running-{id}"), &colors))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let _ = this.load_session_from_ui(select_id.clone(), window, cx);
                            })),
                    )
                    .child(
                        desktop_button(format!("thread-tab-close-{id}"))
                            .icon(IconName::X)
                            .ghost()
                            .xsmall()
                            .accessibility_label(format!("Close tab {title}"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab(&close_id, window, cx);
                            })),
                    )
                    .context_menu(move |menu, _, _| {
                        menu.menu("Close tab", Box::new(CloseThreadTab { session_id: menu_id.clone().into() }))
                            .menu(
                                "Close other tabs",
                                Box::new(CloseOtherThreadTabs { session_id: menu_id.clone().into() }),
                            )
                            .separator()
                            .menu("Copy thread title", Box::new(CopyThreadTitle { session_id: menu_id.clone().into() }))
                    }),
            );
        }
        strip
            .child(
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
            )
            .into_any_element()
    }
}
