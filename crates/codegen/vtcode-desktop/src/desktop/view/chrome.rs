//! Window chrome: sidebar, thread header, and toolbar. Pane resizing lives
//! in the sibling `resize` module.

use super::*;
use gpui_kit::component::menu::ContextMenuExt;

impl DesktopView {
    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tag_filter = self.render_tag_filter(cx);
        let tag_section = self.render_tag_section(cx);
        let theme = cx.theme();
        let workspace_section = rust_i18n::t!(i18n::WORKSPACE_SECTION).to_string();
        let projects_section = rust_i18n::t!("app.workspace.projects").to_string();
        let recent_threads_section = rust_i18n::t!("app.workspace.recent_threads").to_string();
        let mut sidebar = div()
            .id("project-thread-sidebar")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.navigation.sidebar_label").to_string())
            .w(px(self.sidebar_width))
            .min_w(px(MIN_PANE_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .when(!self.preferences.navigation_on_right(), |sidebar| sidebar.pt(px(TOOLBAR_HEIGHT)))
            .bg(if self.effective_translucent_sidebar() {
                themes::translucent_sidebar_surface(&theme.colors)
            } else {
                themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground)
                    .panel
            })
            .rounded_lg()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eyebrow_label(workspace_section, &theme.colors))
                    .child(
                        desktop_ghost_button("choose-workspace")
                            .tooltip_with_action(
                                rust_i18n::t!(i18n::WORKSPACE_CHOOSE).to_string(),
                                &ChooseWorkspace,
                                Some(key_context::DESKTOP),
                            )
                            .label(if self.workspace_selection_pending {
                                rust_i18n::t!("app.workspace.checking").to_string()
                            } else if self.workspace.is_some() {
                                rust_i18n::t!("app.workspace.change").to_string()
                            } else {
                                rust_i18n::t!("app.workspace.open").to_string()
                            })
                            .desktop_disabled(
                                self.workspace_picker_open
                                    || !self.workspace_switch_idle()
                                    || self.retained_submission.is_some(),
                            )
                            .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone()),
                    ),
            );

        let workspace_label = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| rust_i18n::t!("app.workspace.project_placeholder").to_string());
        sidebar = sidebar.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Icon::new(IconName::FolderGit2).text_color(theme.colors.primary))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .font_semibold()
                        .truncate()
                        .child(workspace_label),
                ),
        );

        sidebar = sidebar.child(
            div()
                .flex()
                .w_full()
                .gap_2()
                .child(
                    desktop_ghost_button("new-session")
                        .tooltip_with_action(
                            rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                            &NewSession,
                            Some(key_context::DESKTOP),
                        )
                        .label(rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string())
                        .icon(IconName::Plus)
                        .flex_1()
                        .desktop_disabled(!self.new_thread_available())
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
                .child(
                    desktop_ghost_button("close-session")
                        .tooltip_with_action(
                            rust_i18n::t!("app.navigation.close_session").to_string(),
                            &CloseSession,
                            Some(key_context::DESKTOP),
                        )
                        .icon(IconName::X)
                        .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string())
                        .desktop_disabled(self.selected_session_id.is_none() || !self.new_thread_available())
                        .dispatches_desktop_action(CloseSession, self.focus_handle.clone()),
                ),
        );

        sidebar = sidebar.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(eyebrow_label(projects_section, &theme.colors))
                .child(tag_filter),
        );
        let recent = self.visible_recent_workspaces();
        if !recent.is_empty() {
            sidebar = sidebar.child(div().w_full().flex().flex_col().gap_1().children(recent.iter().map(|path| {
                let workspace_path = path.clone();
                let current = self
                    .workspace
                    .as_ref()
                    .is_some_and(|selected| selected.display().to_string() == *path);
                let label = workspace_short_name(path).to_string();
                div().w_full().child(
                    desktop_ghost_button(opaque_path_element_id("workspace", path))
                        .accessibility_label(if current {
                            rust_i18n::t!(i18n::WORKSPACE_CURRENT_LABEL, workspace = label).to_string()
                        } else {
                            label.clone()
                        })
                        .child(navigation_label(label, IconName::Folder, current, theme.colors.primary))
                        .toggled(current)
                        .w_full()
                        .desktop_disabled(!self.workspace_switch_idle() || self.retained_submission.is_some())
                        .when(current, |button| button.bg(theme.colors.secondary).text_color(theme.colors.foreground))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.select_workspace_from_ui(PathBuf::from(workspace_path.clone()), window, cx)
                        })),
                )
            })));
        }

        sidebar = sidebar.child(tag_section);

        sidebar = sidebar.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(eyebrow_label(recent_threads_section, &theme.colors))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(self.context_sessions().len().to_string()),
                ),
        );
        let session_row_height = scale_row_height_for_ui_font(SESSION_ROW_HEIGHT, cx.theme().font_size);
        let session_sizes = self.session_row_sizes.get(self.context_sessions().len(), session_row_height);
        let session_primary = theme.colors.primary;
        sidebar = sidebar
            .child(
                div()
                    .id("session-list")
                    .test_support()
                    .track_focus(&self.session_focus)
                    .when(self.context_sessions().is_empty(), |el| el.tab_stop(false))
                    .key_context(key_context::SESSIONS)
                    .on_action(cx.listener(|view, _: &SessionPrevious, _, cx| view.move_session_focus(false, cx)))
                    .on_action(cx.listener(|view, _: &SessionNext, _, cx| view.move_session_focus(true, cx)))
                    .on_action(cx.listener(|view, _: &SessionFirst, _, cx| view.move_session_focus_edge(true, cx)))
                    .on_action(cx.listener(|view, _: &SessionLast, _, cx| view.move_session_focus_edge(false, cx)))
                    .on_action(cx.listener(|view, _: &OpenSelectedSession, window, cx| {
                        view.activate_focused_session(window, cx)
                    }))
                    .on_action(cx.listener(|view, _: &DeleteHighlightedSession, window, cx| {
                        view.delete_highlighted_session(window, cx)
                    }))
                    .role(gpui_kit::Role::List)
                    .aria_label(rust_i18n::t!("app.workspace.recent_threads").to_string())
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        v_virtual_list(
                            cx.entity(),
                            "session-virtual-list",
                            session_sizes,
                            move |view, visible_range, _, cx| {
                                visible_range
                                    .filter_map(|index| {
                                        let sessions = view.context_sessions();
                                        let session = sessions.get(index)?;
                                        let session_id = session.session_id.clone();
                                        let menu_id = session_id.clone();
                                        let tabbed = view.open_tabs.iter().any(|tab| tab == &session_id);
                                        let selected = view.selected_session_id.as_deref() == Some(&session_id);
                                        let label = session_display_title(&session_id, session.title.as_deref());
                                        let running = view.is_session_running(&session_id);
                                        let enabled = view.load_session_available(&session_id);
                                        let keyboard_focused =
                                            view.keyboard_focused_session.as_deref() == Some(session_id.as_str());
                                        Some(
                                            div()
                                                .id(format!("session-row-{session_id}"))
                                                .test_support()
                                                .role(gpui_kit::Role::ListItem)
                                                .when(keyboard_focused, |row| row.aria_active_descendant())
                                                .aria_label(label.clone())
                                                .aria_position_in_set(index + 1)
                                                .aria_size_of_set(view.context_sessions().len())
                                                .aria_selected(selected)
                                                .h(px(session_row_height))
                                                .w_full()
                                                // Absolutely positioned so the highlight never reflows the
                                                // row; mirrors the transcript focus rail.
                                                .when(keyboard_focused, |row| {
                                                    row.child(
                                                        div()
                                                            .id(format!("session-focus-rail-{session_id}"))
                                                            .test_support()
                                                            .absolute()
                                                            .left_0()
                                                            .top_0()
                                                            .bottom_0()
                                                            .w(px(2.))
                                                            .bg(cx.theme().colors.accent),
                                                    )
                                                })
                                                .child(
                                                    desktop_ghost_button(format!("session-{session_id}"))
                                                        .accessibility_label(if running {
                                                            rust_i18n::t!(
                                                                i18n::SESSION_RUNNING_THREAD_LABEL,
                                                                thread = label.as_str()
                                                            )
                                                            .to_string()
                                                        } else if selected {
                                                            rust_i18n::t!(
                                                                "app.navigation.current_thread_label",
                                                                thread = label
                                                            )
                                                            .to_string()
                                                        } else {
                                                            label.clone()
                                                        })
                                                        .child(navigation_label(
                                                            label,
                                                            IconName::MessagesSquare,
                                                            selected,
                                                            session_primary,
                                                        ))
                                                        .when(running, |button| {
                                                            button.child(running_indicator(
                                                                format!("session-running-{session_id}"),
                                                                &cx.theme().colors,
                                                            ))
                                                        })
                                                        .toggled(selected)
                                                        // Single-tab-stop list: arrows drive the shared
                                                        // highlight, so rows stay out of Tab order.
                                                        .tab_stop(false)
                                                        .desktop_disabled(!enabled)
                                                        .w_full()
                                                        .justify_start()
                                                        .when(selected, |button| {
                                                            button
                                                                .bg(cx.theme().colors.secondary)
                                                                .text_color(cx.theme().colors.foreground)
                                                        })
                                                        .on_click(cx.listener(move |this, _, window, cx| {
                                                            // Clicking anchors the keyboard highlight as
                                                            // position memory, like transcript rows.
                                                            this.keyboard_focused_session = Some(session_id.clone());
                                                            if this.load_session_from_ui(session_id.clone(), window, cx)
                                                            {
                                                                this.request_panel_dismiss(window, cx);
                                                            }
                                                        })),
                                                )
                                                .context_menu(move |menu, _, _| {
                                                    tabs::session_row_menu(menu, &menu_id, tabbed)
                                                })
                                                .into_any_element(),
                                        )
                                    })
                                    .collect()
                            },
                        )
                        .track_scroll(&self.session_scroll),
                    )
                    .child(Scrollbar::vertical(&self.session_scroll)),
            )
            .when(self.session_list_has_older, |bar| bar.child(self.render_session_list_more("session-list-more", cx)));

        let status = if self.ui_progress_active && !self.running && self.interaction.is_none() {
            rust_i18n::t!("app.service.running").to_string()
        } else {
            self.status.clone()
        };
        let status_tone = theme.colors.muted_foreground;
        sidebar.child(
            div().mt_auto().pt_3().text_xs().text_color(status_tone).child(
                div()
                    .id("sidebar-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(status.clone())
                    .when(self.global_loading_active(), |row| {
                        row.child(shimmer_label("sidebar-status-loading", status.clone(), status_tone, cx))
                    })
                    .when(!self.global_loading_active(), |row| row.child(status.clone())),
            ),
        )
    }

    /// Compact identity row for the zero/single-tab case with content or a
    /// selection: one quiet title line plus the conversation-column new-thread
    /// action. Multi-tab windows use the switch strip instead, so this row
    /// never repeats it.
    pub(super) fn render_thread_header(&self, theme: &Theme) -> impl IntoElement {
        let title_fallback = rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string();
        let title = self
            .sessions
            .iter()
            .find(|session| Some(session.session_id.as_str()) == self.selected_session_id.as_deref())
            .and_then(|session| session.title.as_deref())
            .unwrap_or(&title_fallback);
        let running = self
            .selected_session_id
            .as_deref()
            .is_some_and(|session_id| self.is_session_running(session_id));
        // Centered on the conversation spine: the title shares the transcript
        // and composer column instead of hugging the workspace edge. The
        // edge inset lives on the outer wrapper (mirroring the scroller's
        // row wrapper), so the title starts on the spine.
        div().w_full().flex().justify_center().px_3().child(
            div()
                .id("thread-header")
                .test_support()
                .w_full()
                .max_w(px(self.content_width()))
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .py_1()
                .when(running, |header| {
                    header
                        .role(gpui_kit::Role::Group)
                        .aria_label(rust_i18n::t!(i18n::SESSION_RUNNING_THREAD_LABEL, thread = title).to_string())
                        .child(running_indicator("thread-header-running", &theme.colors))
                        // The background action left the composer run
                        // cluster; the thread header is its pointer home —
                        // beside the marker naming the running state.
                        .child(
                            desktop_ghost_button("background-exec")
                                .icon(IconName::ChevronsDown)
                                .small()
                                .accessibility_label(rust_i18n::t!("app.composer.background_operation").to_string())
                                .tooltip(rust_i18n::t!("app.composer.background_operation_tooltip").to_string())
                                .dispatches_desktop_action(BackgroundOperation, self.focus_handle.clone()),
                        )
                })
                .child(
                    div()
                        .id("thread-title")
                        .test_support()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .font_semibold()
                        .truncate()
                        .child(title.to_string()),
                )
                .child(
                    desktop_ghost_button("thread-header-new")
                        .icon(IconName::Plus)
                        .small()
                        .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string())
                        .tooltip_with_action(
                            rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                            &NewSession,
                            Some(key_context::DESKTOP),
                        )
                        .desktop_disabled(!self.new_thread_available())
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
                .child(
                    desktop_ghost_button("thread-header-cli")
                        .icon(IconName::Terminal)
                        .small()
                        .accessibility_label(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string())
                        .tooltip(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string())
                        .desktop_disabled(self.selected_session_id.is_none())
                        .dispatches_desktop_action(OpenCliHandoff, self.focus_handle.clone()),
                )
                .fade_in("thread-header-fade", theme),
        )
    }

    /// Back/Forward pair shared by the normal toolbar and the zen floating
    /// chrome. IDs, labels, pending cues, disabled state, and actions stay
    /// identical so switching modes never changes their behavior.
    fn render_navigation_buttons(&self) -> impl IntoElement {
        let pending_direction = self.pending_navigation_direction();
        let pending_back = pending_direction == Some(NavigationDirection::Back);
        let pending_forward = pending_direction == Some(NavigationDirection::Forward);
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                desktop_ghost_button("navigate-back")
                    .accessibility_label(if pending_back {
                        rust_i18n::t!("app.navigation.back_navigating").to_string()
                    } else {
                        rust_i18n::t!("app.navigation.back").to_string()
                    })
                    .icon(IconName::ArrowLeft)
                    .small()
                    .loading(pending_back)
                    .tooltip_with_action(
                        rust_i18n::t!("app.navigation.back").to_string(),
                        &NavigateBack,
                        Some(key_context::DESKTOP),
                    )
                    .desktop_disabled(!self.route_back_available() && !self.can_navigate_back() && !pending_back)
                    .dispatches_desktop_action(NavigateBack, self.focus_handle.clone()),
            )
            .child(
                desktop_ghost_button("navigate-forward")
                    .accessibility_label(if pending_forward {
                        rust_i18n::t!("app.navigation.forward_navigating").to_string()
                    } else {
                        rust_i18n::t!("app.navigation.forward").to_string()
                    })
                    .icon(IconName::ArrowRight)
                    .small()
                    .loading(pending_forward)
                    .tooltip_with_action(
                        rust_i18n::t!("app.navigation.forward").to_string(),
                        &NavigateForward,
                        Some(key_context::DESKTOP),
                    )
                    .desktop_disabled(
                        !self.route_history.can_forward() && !self.can_navigate_forward() && !pending_forward,
                    )
                    .dispatches_desktop_action(NavigateForward, self.focus_handle.clone()),
            )
    }

    pub(super) fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let workspace_foreground = theme.colors.foreground;
        // The toolbar is chrome, not content: in translucent mode it floats
        // fully transparent on the window vibrancy instead of painting a
        // band over it (see `toolbar_chrome`).
        let toolbar_surface = themes::toolbar_chrome(
            self.effective_translucent_sidebar(),
            theme.colors.background,
            theme.colors.secondary,
            theme.colors.foreground,
        )
        .band;
        // Fullscreen hides the traffic lights, so the leading cluster hugs the
        // window edge; windowed mode keeps the 80px light clearance. The kit
        // adds its own fullscreen pl_3 on the inner bar, which stacks here.
        let leading_inset = |bar: TitleBar| {
            if window.is_fullscreen() { bar.pl_3() } else { bar }
        };
        // Zen drops the toolbar entirely (see `render_zen_floating_chrome`),
        // so this bar only renders in normal mode. The navigation pair is
        // shared with the zen floating chrome via `render_navigation_buttons`.
        let navigation_visible = self.preferences.navigation_visible();
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map_or_else(|| rust_i18n::t!("app.navigation.workspace_fallback").to_string(), str::to_string);
        // Title-bar budgets follow the main column, not the viewport: the bar
        // lives beside the navigation, so a wide sidebar starves it first.
        // Mirrors the root layout's expanded predicate exactly.
        let expanded = navigation_visible && (self.sidebar_open || self.navigation_peek) && !compact_viewport(window);
        let sidebar_footprint = if !navigation_visible {
            0.
        } else if expanded {
            self.sidebar_width + NAVIGATION_RAIL_WIDTH + SIDEBAR_RESIZE_SLOP
        } else {
            NAVIGATION_RAIL_WIDTH
        };
        let main_width = window.viewport_size().width - px(sidebar_footprint);
        let show_details = main_width >= px(MAIN_COLUMN_DETAILS_BREAKPOINT);
        // The leading cluster shares the trailing toolbar's compact size, so the
        // whole title bar is one band: same frame, same center line.
        leading_inset(
            TitleBar::new()
                .w_full()
                .h(px(TOOLBAR_HEIGHT))
                .pr_3()
                .border_b_0()
                .bg(toolbar_surface)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(self.render_navigation_buttons())
                        .child(div().text_sm().font_semibold().child(app::NAME))
                        .when(show_details, |row| {
                            row.child(
                                div()
                                    .id("title-bar-workspace")
                                    .test_support()
                                    .min_w_0()
                                    .max_w(px(WORKSPACE_LABEL_MAX_WIDTH))
                                    .truncate()
                                    .text_sm()
                                    .text_color(workspace_foreground)
                                    .child(format!("/ {workspace}")),
                            )
                        }),
                )
                .child(self.render_header_actions(show_details, cx)),
        )
        .fade_in("title-bar-fade", cx.theme())
        .into_any_element()
    }

    /// Window-drag strip for windowed zen mode: the toolbar (and Kit
    /// TitleBar's `start_window_move` drag) is hidden, while
    /// `app_owns_titlebar_drag` disables native AppKit dragging for the
    /// whole window — without this strip the window cannot be moved at
    /// all. The strip owns the traffic-light zone
    /// (`ZEN_DRAG_STRIP_HEIGHT`), so route pages add no padding of their
    /// own; double-click keeps the native zoom behavior. Transparent by
    /// design: it is window chrome, not content.
    pub(super) fn render_zen_drag_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("zen-drag-strip")
            .w_full()
            .h(px(ZEN_DRAG_STRIP_HEIGHT))
            .flex_shrink_0()
            // Before `test_support()`: `on_double_click` is implemented for
            // `Stateful` only (same ordering as Kit TitleBar).
            .on_double_click(|_, window, _| window.titlebar_double_click())
            .test_support()
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, _| {
                this.zen_drag_armed = false;
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, _| {
                    this.zen_drag_armed = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.zen_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
                // Fail closed: only a held left button may start the native
                // drag. A press released off-strip (or lost to a focus
                // change) never disarms — `on_mouse_up` only fires while
                // hovered — so a bare hover must heal the stuck arm instead
                // of grabbing the window with no button held. The arm is
                // consumed exactly once, on every move over the strip.
                let armed = std::mem::replace(&mut this.zen_drag_armed, false);
                if !event.dragging() || !armed {
                    return;
                }
                // Native-only: the headless test platform leaves
                // `start_window_move` unimplemented (as with `zoom_window`
                // in bootstrap), so guard the call the same way.
                #[cfg(not(test))]
                window.start_window_move();
                #[cfg(test)]
                let _ = window;
            }))
            .into_any_element()
    }

    /// Floating zen chrome: Home, Back/Forward, plus the zen exit, pinned to
    /// the main column's top-right corner and painted after the content so it
    /// floats above the edge-to-edge zen surface. The group is right-anchored
    /// and the toggle is its trailing child, so arrows appearing/disappearing
    /// never move the toggle. Home returns to the workspace in Zen while
    /// Browse threads keeps its own slot in the band.
    ///
    /// The insets reproduce the docked slot exactly: the title bar's `pr_3`
    /// (12px) plus the small toolbar's `p_1` (4px), minus the pill's 1px
    /// border; vertically the 48px toolbar centers its 32px band, leaving
    /// 12px above the 24px button. The zen chrome UI integration test in
    /// `view/tests/zen_chrome.rs` guards this alignment if either changes.
    pub(super) fn render_zen_floating_chrome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const FLOATING_TOP_INSET: f32 = 11.;
        const FLOATING_TRAILING_INSET: f32 = 15.;

        let colors = cx.theme().colors;
        div()
            .absolute()
            .top(px(FLOATING_TOP_INSET))
            .right(px(FLOATING_TRAILING_INSET))
            .flex()
            .items_center()
            .gap_1()
            .rounded_full()
            .bg(colors.popover)
            .border_1()
            .border_color(themes::soft_edge_color(&colors))
            .shadow_md()
            .fade_in("zen-toggle-float", cx.theme())
            .child(self.render_go_home_button(true))
            .child(self.render_new_thread_button())
            .child(self.render_browse_threads_button(cx))
            .child(self.render_utilities_button(cx))
            .child(self.render_navigation_buttons())
            .child(zen_mode_toggle(true, None, &self.focus_handle))
    }

    /// Trailing header actions for the normal-mode toolbar. The zen toggle is
    /// always last, so it keeps one trailing slot. The toolbar forces the
    /// compact size on `.child()` controls, while `.content()` items carry
    /// their own size.
    fn render_header_actions(&self, show_details: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut group = ToolbarGroup::new("header-actions-group")
            .label(rust_i18n::t!("app.navigation.workspace_actions").to_string())
            .gap_1();
        group = group.child(
            desktop_ghost_button("search-commands")
                .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string())
                .icon(IconName::Search)
                .tooltip_with_action(
                    rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string(),
                    &OpenCommandPalette,
                    Some(key_context::DESKTOP),
                )
                .dispatches_desktop_action(OpenCommandPalette, self.focus_handle.clone()),
        );
        // Redundant with the composer model pill: first to go when the main
        // column narrows, together with the workspace suffix.
        if show_details {
            if let Some(speed) = self.session_options().and_then(|options| options.speed.as_ref()) {
                group = group.content(
                    Tag::custom(
                        theme.colors.secondary,
                        theme.colors.foreground,
                        themes::soft_edge_color(&theme.colors),
                    )
                    .small()
                    .child(speed.clone()),
                );
            }
        }
        group = group.content(
            self.render_chrome_menu("header-overflow", rust_i18n::t!("app.navigation.window_actions").to_string()),
        );
        // Share the toolbar's surface derivation with the band fill so the
        // quiet entry icon is always calibrated to the surface it paints on,
        // including the translucent window-vibrancy case modeled by the rail.
        let faded = themes::toolbar_chrome(
            self.effective_translucent_sidebar(),
            theme.colors.background,
            theme.colors.secondary,
            theme.colors.foreground,
        )
        .icon_tone;
        // The toolbar only renders outside zen mode; the toggle always offers entry here.
        group = group.child(zen_mode_toggle(false, Some(faded), &self.focus_handle));
        Toolbar::new("header-toolbar").small().child(group)
    }
}
