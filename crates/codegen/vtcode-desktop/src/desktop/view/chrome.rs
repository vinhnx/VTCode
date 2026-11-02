//! Window chrome: pane resizing, sidebar, thread header, and toolbar.

use super::*;
use gpui_kit::component::menu::ContextMenuExt;

/// Which divider is being dragged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ResizeEdge {
    Sidebar,
    Inspector,
}

#[derive(Clone, Copy)]
pub(super) struct PaneResize {
    pub(super) edge: ResizeEdge,
    pub(super) start_pointer_x_window: Pixels,
    pub(super) start_width: Pixels,
}

/// The splitter has no drag preview; keeping its entity empty lets the handle
/// provide GPUI's drag lifecycle without covering the panels during resizing.
struct ResizeDragPreview;

impl Render for ResizeDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

impl DesktopView {
    /// The draggable divider between panes.
    pub(super) fn render_resize_handle(&self, edge: ResizeEdge, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, placement, label, width) = match edge {
            ResizeEdge::Sidebar => (
                "resize-sidebar",
                if self.preferences.navigation_on_right() {
                    HandleEdge::Leading
                } else {
                    HandleEdge::Trailing
                },
                rust_i18n::t!("app.navigation.resize_navigation").to_string(),
                self.sidebar_width,
            ),
            ResizeEdge::Inspector => (
                "resize-inspector",
                HandleEdge::Leading,
                rust_i18n::t!("app.navigation.resize_inspector").to_string(),
                self.inspector_width,
            ),
        };
        let focus = match edge {
            ResizeEdge::Sidebar => &self.sidebar_resize_focus,
            ResizeEdge::Inspector => &self.inspector_resize_focus,
        };
        let handle = resize_handle::<ResizeEdge, ResizeDragPreview>(id, Axis::Horizontal);
        // The inspector is a separated card, not a docked pane: its edge
        // paints no resting hairline (see inspector_resize_appearance).
        let handle = if edge == ResizeEdge::Inspector {
            handle.with_appearance(inspector_resize_appearance())
        } else {
            handle
        };
        handle
            .inside(placement)
            .on_drag(edge, |_, _, _, cx| cx.new(|_| ResizeDragPreview))
            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<ResizeEdge>, _, cx| {
                let edge = *event.drag(cx);
                if this.resizing.is_some_and(|drag| drag.edge == edge) {
                    let changed = this.resize_from_drag(edge, event.event.position.x);
                    cx.stop_propagation();
                    if changed {
                        cx.notify();
                    }
                }
            }))
            .track_focus(focus)
            .role(gpui_kit::Role::Splitter)
            .aria_label(label)
            .aria_description(rust_i18n::t!("app.navigation.resize_hint").to_string())
            .aria_numeric_value(width as f64)
            .aria_min_numeric_value(MIN_PANE_WIDTH as f64)
            .aria_max_numeric_value(MAX_PANE_WIDTH as f64)
            .focus_visible(|style| style.bg(cx.theme().colors.ring.opacity(0.2)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                let direction = match event.keystroke.key.as_str() {
                    "left" => -1.,
                    "right" => 1.,
                    _ => return,
                };
                let step = if modifiers.shift { 50. } else { 10. };
                let current_width = match edge {
                    ResizeEdge::Sidebar => this.sidebar_width,
                    ResizeEdge::Inspector => this.inspector_width,
                };
                let delta = match edge {
                    ResizeEdge::Sidebar if this.preferences.navigation_on_right() => -direction * step,
                    ResizeEdge::Sidebar => direction * step,
                    ResizeEdge::Inspector => -direction * step,
                };
                if this.resize_pane_to(edge, current_width + delta) {
                    this.save_preferences();
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    match edge {
                        ResizeEdge::Sidebar => this.sidebar_resize_focus.focus(window, cx),
                        ResizeEdge::Inspector => this.inspector_resize_focus.focus(window, cx),
                    }
                    this.resizing = Some(PaneResize {
                        edge,
                        start_pointer_x_window: event.position.x,
                        start_width: px(match edge {
                            ResizeEdge::Sidebar => this.sidebar_width,
                            ResizeEdge::Inspector => this.inspector_width,
                        }),
                    });
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, _: &ResizeEdge, _, cx| {
                if this.resizing.take().is_some() {
                    this.save_preferences();
                    cx.notify();
                }
            }))
    }

    fn resize_pane_to(&mut self, edge: ResizeEdge, width: f32) -> bool {
        if !width.is_finite() {
            return false;
        }
        let width = width.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH);
        let current = match edge {
            ResizeEdge::Sidebar => &mut self.sidebar_width,
            ResizeEdge::Inspector => &mut self.inspector_width,
        };
        if (*current - width).abs() < f32::EPSILON {
            return false;
        }
        *current = width;
        true
    }

    fn resize_from_drag(&mut self, edge: ResizeEdge, pointer_x_window: Pixels) -> bool {
        let Some(drag) = self.resizing.filter(|drag| drag.edge == edge) else {
            return false;
        };
        let pointer_delta_window = pointer_x_window - drag.start_pointer_x_window;
        let width = match edge {
            ResizeEdge::Sidebar if self.preferences.navigation_on_right() => drag.start_width - pointer_delta_window,
            ResizeEdge::Sidebar => drag.start_width + pointer_delta_window,
            ResizeEdge::Inspector => drag.start_width - pointer_delta_window,
        };
        // Preferences keep pane widths as f32; convert only at that boundary.
        self.resize_pane_to(edge, width.as_f32())
    }

    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .bg(if self.preferences.translucent_sidebar {
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
                        desktop_button("choose-workspace")
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
                            .ghost()
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
                    desktop_button("new-session")
                        .tooltip_with_action(
                            rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                            &NewSession,
                            Some(key_context::DESKTOP),
                        )
                        .label(rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string())
                        .icon(IconName::Plus)
                        .ghost()
                        .flex_1()
                        .desktop_disabled(!self.new_thread_available())
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
                .child(
                    desktop_button("close-session")
                        .tooltip_with_action(
                            rust_i18n::t!("app.navigation.close_session").to_string(),
                            &CloseSession,
                            Some(key_context::DESKTOP),
                        )
                        .icon(IconName::X)
                        .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string())
                        .ghost()
                        .desktop_disabled(self.selected_session_id.is_none() || !self.new_thread_available())
                        .dispatches_desktop_action(CloseSession, self.focus_handle.clone()),
                ),
        );

        sidebar = sidebar.child(eyebrow_label(projects_section, &theme.colors));
        let recent = self.recent_workspaces();
        if !recent.is_empty() {
            sidebar = sidebar.child(div().w_full().flex().flex_col().gap_1().children(recent.iter().map(|path| {
                let workspace_path = path.clone();
                let current = self
                    .workspace
                    .as_ref()
                    .is_some_and(|selected| selected.display().to_string() == *path);
                let label = workspace_short_name(path).to_string();
                div().w_full().child(
                    desktop_button(opaque_path_element_id("workspace", path))
                        .accessibility_label(if current {
                            rust_i18n::t!(i18n::WORKSPACE_CURRENT_LABEL, workspace = label).to_string()
                        } else {
                            label.clone()
                        })
                        .child(navigation_label(label, IconName::Folder, current, theme.colors.primary))
                        .ghost()
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
                        .child(self.sessions.len().to_string()),
                ),
        );
        let session_row_height = scale_row_height_for_ui_font(SESSION_ROW_HEIGHT, cx.theme().font_size);
        let session_sizes = self.session_row_sizes.get(self.sessions.len(), session_row_height);
        let session_primary = theme.colors.primary;
        sidebar = sidebar.child(
            div()
                .id("session-list")
                .test_support()
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
                                    let session = view.sessions.get(index)?;
                                    let session_id = session.session_id.clone();
                                    let menu_id = session_id.clone();
                                    let tabbed = view.open_tabs.iter().any(|tab| tab == &session_id);
                                    let selected = view.selected_session_id.as_deref() == Some(&session_id);
                                    let label = session_display_title(&session_id, session.title.as_deref());
                                    let running = view.is_session_running(&session_id);
                                    let enabled = view.load_session_available(&session_id);
                                    Some(
                                        div()
                                            .id(format!("session-row-{session_id}"))
                                            .test_support()
                                            .role(gpui_kit::Role::ListItem)
                                            .aria_label(label.clone())
                                            .aria_position_in_set(index + 1)
                                            .aria_size_of_set(view.sessions.len())
                                            .aria_selected(selected)
                                            .h(px(session_row_height))
                                            .w_full()
                                            .child(
                                                desktop_button(format!("session-{session_id}"))
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
                                                        IconName::MessageSquare,
                                                        selected,
                                                        session_primary,
                                                    ))
                                                    .when(running, |button| {
                                                        button.child(running_indicator(
                                                            format!("session-running-{session_id}"),
                                                            &cx.theme().colors,
                                                        ))
                                                    })
                                                    .ghost()
                                                    .toggled(selected)
                                                    .desktop_disabled(!enabled)
                                                    .w_full()
                                                    .justify_start()
                                                    .when(selected, |button| {
                                                        button
                                                            .bg(cx.theme().colors.secondary)
                                                            .text_color(cx.theme().colors.foreground)
                                                    })
                                                    .on_click(cx.listener(move |this, _, window, cx| {
                                                        if this.load_session_from_ui(session_id.clone(), window, cx) {
                                                            this.close_active_route(window, cx);
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
        );

        let status = if self.ui_progress_active && !self.running && self.interaction.is_none() {
            rust_i18n::t!("app.service.running").to_string()
        } else {
            self.status.clone()
        };
        sidebar.child(
            div()
                .mt_auto()
                .pt_3()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(
                    div()
                        .id("sidebar-status")
                        .test_support()
                        .role(gpui_kit::Role::Status)
                        .aria_label(status.clone())
                        .when(self.global_loading_active(), |row| {
                            row.child(shimmer_label("sidebar-status-loading", status.clone()))
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
        // and composer column instead of hugging the workspace edge.
        div().w_full().flex().justify_center().child(
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
                .px_4()
                .py_1()
                .when(running, |header| {
                    header
                        .role(gpui_kit::Role::Group)
                        .aria_label(rust_i18n::t!(i18n::SESSION_RUNNING_THREAD_LABEL, thread = title).to_string())
                        .child(running_indicator("thread-header-running", &theme.colors))
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
                    desktop_button("thread-header-new")
                        .icon(IconName::Plus)
                        .ghost()
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
                    desktop_button("thread-header-cli")
                        .icon(IconName::Terminal)
                        .ghost()
                        .small()
                        .accessibility_label(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string())
                        .tooltip(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string())
                        .desktop_disabled(self.selected_session_id.is_none())
                        .dispatches_desktop_action(OpenCliHandoff, self.focus_handle.clone()),
                )
                .with_animation("thread-header-fade", zen_fade_animation(theme), |el, delta| el.opacity(delta)),
        )
    }

    pub(super) fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let toolbar_surface =
            themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground).rail;
        // Fullscreen hides the traffic lights, so the leading cluster hugs the
        // window edge; windowed mode keeps the 80px light clearance. The kit
        // adds its own fullscreen pl_3 on the inner bar, which stacks here.
        let leading_inset = |bar: TitleBar| {
            if window.is_fullscreen() { bar.pl_3() } else { bar }
        };
        // Zen drops the toolbar entirely (see `render_zen_toggle_floating`),
        // so this bar only renders in normal mode.
        let navigation_visible = self.preferences.navigation_visible();
        let navigation_label = if navigation_visible {
            rust_i18n::t!(i18n::NAVIGATION_TOGGLE_NAVIGATION).to_string()
        } else {
            rust_i18n::t!("app.navigation.show_navigation").to_string()
        };
        let navigation_icon = if navigation_visible && self.preferences.navigation_on_right() {
            IconName::PanelRight
        } else {
            IconName::PanelLeft
        };
        let pending_direction = self.pending_navigation_direction();
        let pending_back = pending_direction == Some(NavigationDirection::Back);
        let pending_forward = pending_direction == Some(NavigationDirection::Forward);
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map_or_else(|| rust_i18n::t!("app.navigation.workspace_fallback").to_string(), str::to_string);
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
                        .child(
                            desktop_button("navigate-back")
                                .accessibility_label(if pending_back {
                                    rust_i18n::t!("app.navigation.back_navigating").to_string()
                                } else {
                                    rust_i18n::t!("app.navigation.back").to_string()
                                })
                                .icon(IconName::ArrowLeft)
                                .ghost()
                                .small()
                                .loading(pending_back)
                                .tooltip_with_action(
                                    rust_i18n::t!("app.navigation.back").to_string(),
                                    &NavigateBack,
                                    Some(key_context::DESKTOP),
                                )
                                .desktop_disabled(
                                    !self.route_back_available() && !self.can_navigate_back() && !pending_back,
                                )
                                .dispatches_desktop_action(NavigateBack, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("navigate-forward")
                                .accessibility_label(if pending_forward {
                                    rust_i18n::t!("app.navigation.forward_navigating").to_string()
                                } else {
                                    rust_i18n::t!("app.navigation.forward").to_string()
                                })
                                .icon(IconName::ArrowRight)
                                .ghost()
                                .small()
                                .loading(pending_forward)
                                .tooltip_with_action(
                                    rust_i18n::t!("app.navigation.forward").to_string(),
                                    &NavigateForward,
                                    Some(key_context::DESKTOP),
                                )
                                .desktop_disabled(
                                    !self.route_history.can_forward()
                                        && !self.can_navigate_forward()
                                        && !pending_forward,
                                )
                                .dispatches_desktop_action(NavigateForward, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("toggle-sidebar")
                                .accessibility_label(navigation_label.as_str())
                                .toggled(navigation_visible && self.sidebar_open)
                                .icon(navigation_icon)
                                .ghost()
                                .small()
                                .tooltip_with_action(navigation_label, &ToggleSidebar, Some(key_context::DESKTOP))
                                .dispatches_desktop_action(ToggleSidebar, self.focus_handle.clone()),
                        )
                        .child(div().text_sm().font_semibold().child(app::NAME))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(theme.colors.foreground)
                                .child(format!("/ {workspace}")),
                        ),
                )
                .child(self.render_header_actions(cx)),
        )
        .with_animation("title-bar-fade", zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta))
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

    /// Floating zen exit: pinned to the main column's top-right corner and
    /// painted after the content, so it floats above the edge-to-edge zen
    /// surface. Toggling restores the toolbar and the toggle's docked slot.
    pub(super) fn render_zen_toggle_floating(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let faded = themes::faded_chrome(colors.background, colors.foreground);
        div()
            .absolute()
            .top_2()
            .right_2()
            .rounded_full()
            .bg(colors.popover)
            .border_1()
            .border_color(soft_edge_color(&colors))
            .shadow_md()
            .with_animation("zen-toggle-float", zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta))
            .child(zen_mode_toggle(true, faded, &self.focus_handle))
    }

    /// Trailing header actions for the normal-mode toolbar. The zen toggle is
    /// always last, so it keeps one trailing slot. The toolbar forces the
    /// compact size on `.child()` controls, while `.content()` items carry
    /// their own size.
    fn render_header_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mut group = ToolbarGroup::new("header-actions-group")
            .label(rust_i18n::t!("app.navigation.workspace_actions").to_string())
            .gap_1();
        if !self.preferences.navigation_visible() {
            group = group
                .child(
                    desktop_button("top-new-thread")
                        .icon(IconName::Plus)
                        .ghost()
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
                    desktop_button("top-utilities")
                        .icon(IconName::GitBranch)
                        .ghost()
                        .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_WORKSPACE_UTILITIES).to_string())
                        .tooltip_with_action(
                            rust_i18n::t!(i18n::NAVIGATION_WORKSPACE_UTILITIES).to_string(),
                            &OpenUtilities,
                            Some(key_context::DESKTOP),
                        )
                        .desktop_disabled(self.workspace.is_none())
                        .dispatches_desktop_action(OpenUtilities, self.focus_handle.clone()),
                );
        }
        group = group
            .child(
                desktop_button("search-commands")
                    .accessibility_label(rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string())
                    .icon(IconName::Search)
                    .ghost()
                    .tooltip_with_action(
                        rust_i18n::t!(i18n::NAVIGATION_SEARCH_COMMANDS).to_string(),
                        &OpenCommandPalette,
                        Some(key_context::DESKTOP),
                    )
                    .dispatches_desktop_action(OpenCommandPalette, self.focus_handle.clone()),
            )
            .child(
                desktop_button("toggle-inspector")
                    .icon(IconName::PanelRight)
                    .accessibility_label(if self.inspector_open {
                        rust_i18n::t!("app.navigation.hide_panel").to_string()
                    } else {
                        rust_i18n::t!("app.navigation.show_panel").to_string()
                    })
                    .tooltip_with_action(
                        rust_i18n::t!(i18n::NAVIGATION_TOGGLE_INSPECTOR).to_string(),
                        &ToggleInspector,
                        Some(key_context::DESKTOP),
                    )
                    .ghost()
                    .desktop_disabled(self.workspace.is_none() || self.current_route() != Route::Workspace)
                    .dispatches_desktop_action(ToggleInspector, self.focus_handle.clone()),
            )
            .child(
                desktop_button("open-settings")
                    .accessibility_label(rust_i18n::t!("app.navigation.settings_button").to_string())
                    .icon(IconName::Settings2)
                    .tooltip_with_action(
                        rust_i18n::t!("app.navigation.settings_button").to_string(),
                        &OpenSettings,
                        Some(key_context::DESKTOP),
                    )
                    .ghost()
                    .on_click(cx.listener(|view, _, window, cx| view.toggle_settings(window, cx))),
            );
        if let Some(speed) = self.session_options().and_then(|options| options.speed.as_ref()) {
            group = group.content(
                Tag::custom(theme.colors.secondary, theme.colors.foreground, theme.colors.border)
                    .small()
                    .child(speed.clone()),
            );
        }
        group = group.content(
            self.render_chrome_menu("header-overflow", rust_i18n::t!("app.navigation.window_actions").to_string()),
        );
        let faded = themes::faded_chrome(theme.colors.background, theme.colors.foreground);
        // The toolbar only renders outside zen mode; the toggle always offers entry here.
        group = group.child(zen_mode_toggle(false, faded, &self.focus_handle));
        Toolbar::new("header-toolbar").small().child(group)
    }
}
