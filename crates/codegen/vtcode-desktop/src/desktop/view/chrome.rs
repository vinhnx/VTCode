//! Window chrome: pane resizing, sidebar, thread header, and toolbar.

use super::*;

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
                "Resize navigation",
                self.sidebar_width,
            ),
            ResizeEdge::Inspector => {
                ("resize-inspector", HandleEdge::Leading, "Resize inspector", self.inspector_width)
            }
        };
        let focus = match edge {
            ResizeEdge::Sidebar => &self.sidebar_resize_focus,
            ResizeEdge::Inspector => &self.inspector_resize_focus,
        };
        resize_handle::<ResizeEdge, ResizeDragPreview>(id, Axis::Horizontal)
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
            .aria_description("Use Left and Right arrow keys to resize; hold Shift for larger steps.")
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
        let workspace_section = rust_i18n::t!("app.workspace.section").to_string().to_uppercase();
        let projects_section = rust_i18n::t!("app.workspace.projects").to_string().to_uppercase();
        let recent_threads_section = rust_i18n::t!("app.workspace.recent_threads").to_string().to_uppercase();
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
            .bg(
                themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground)
                    .panel,
            )
            .rounded_lg()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label(workspace_section, &theme.colors))
                    .child(
                        desktop_button("choose-workspace")
                            .tooltip_with_action(
                                rust_i18n::t!("app.workspace.choose").to_string(),
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
                            rust_i18n::t!("app.navigation.new_thread").to_string(),
                            &NewSession,
                            Some(key_context::DESKTOP),
                        )
                        .label(rust_i18n::t!("app.navigation.new_thread").to_string())
                        .icon(IconName::Plus)
                        .ghost()
                        .flex_1()
                        .desktop_disabled(!self.session_change_allowed(None))
                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                )
                .child(
                    desktop_button("close-session")
                        .tooltip_with_action("Close session", &CloseSession, Some(key_context::DESKTOP))
                        .icon(IconName::X)
                        .accessibility_label(rust_i18n::t!("app.navigation.close_thread").to_string())
                        .ghost()
                        .desktop_disabled(self.selected_session_id.is_none() || !self.session_change_allowed(None))
                        .dispatches_desktop_action(CloseSession, self.focus_handle.clone()),
                ),
        );

        sidebar = sidebar.child(section_label(projects_section, &theme.colors));
        let recent = self.recent_workspaces();
        if !recent.is_empty() {
            sidebar = sidebar.child(div().w_full().flex().flex_col().gap_1().children(recent.iter().map(|path| {
                let workspace_path = path.clone();
                let current = self
                    .workspace
                    .as_ref()
                    .is_some_and(|selected| selected.display().to_string() == *path);
                let label = Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_string();
                div().w_full().child(
                    desktop_button(opaque_path_element_id("workspace", path))
                        .accessibility_label(if current {
                            rust_i18n::t!("app.workspace.current_label", workspace = label).to_string()
                        } else {
                            label.clone()
                        })
                        .child(navigation_label(label, IconName::Folder, current, theme.colors.primary))
                        .ghost()
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
                .mt_2()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label(recent_threads_section, &theme.colors))
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
                                    let selected = view.selected_session_id.as_deref() == Some(&session_id);
                                    let label = session.title.clone().unwrap_or_else(|| {
                                        format!("Session {}", session_id.chars().take(8).collect::<String>())
                                    });
                                    let enabled = view.session_change_allowed(Some(&session_id));
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
                                                    .accessibility_label(if selected {
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
                                                    .ghost()
                                                    .desktop_disabled(!enabled)
                                                    .w_full()
                                                    .justify_start()
                                                    .when(selected, |button| {
                                                        button
                                                            .bg(cx.theme().colors.secondary)
                                                            .text_color(cx.theme().colors.foreground)
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.load_session(session_id.clone(), cx)
                                                    })),
                                            )
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
                        .aria_label(self.status.clone())
                        .child(self.status.clone()),
                ),
        )
    }

    pub(super) fn render_thread_header(&self, theme: &Theme) -> impl IntoElement {
        let title_fallback = rust_i18n::t!("app.navigation.new_thread").to_string();
        let title = self
            .sessions
            .iter()
            .find(|session| Some(session.session_id.as_str()) == self.selected_session_id.as_deref())
            .and_then(|session| session.title.as_deref())
            .unwrap_or(&title_fallback);
        div()
            .id("thread-header")
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_8()
            .py_3()
            .child(
                div()
                    .id("thread-title")
                    .test_support()
                    .min_w_0()
                    .flex_1()
                    .text_lg()
                    .font_semibold()
                    .truncate()
                    .child(title.to_string()),
            )
            .with_animation("thread-header-fade", zen_fade_animation(theme), |el, delta| el.opacity(delta))
    }

    /// Leaf toggle pinned to the main container's top-trailing corner while
    /// both side panels are collapsed. It keeps one fixed position in both
    /// modes and only changes paint: a ghost button to enter Zen, faded text
    /// to exit.
    pub(super) fn render_floating_zen_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let button = if self.zen_mode {
            let faded = themes::faded_chrome(colors.background, colors.foreground);
            exit_zen_button(ZEN_FLOAT_TOGGLE_ELEMENT_ID, faded, &self.focus_handle)
        } else {
            enter_zen_button(ZEN_FLOAT_TOGGLE_ELEMENT_ID, &self.focus_handle)
        };
        // The toggle persists across modes, so the animation id is keyed by
        // mode: each toggle replays the fade while the button stays fixed.
        div()
            .absolute()
            .top_2()
            .right_2()
            .child(button)
            .with_animation(
                gpui_kit::ElementId::NamedInteger("zen-toggle-fade".into(), self.zen_mode as u64),
                zen_fade_animation(cx.theme()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let toolbar_surface =
            themes::navigation_surfaces(theme.colors.background, theme.colors.secondary, theme.colors.foreground).rail;
        if self.zen_mode {
            // The toggle stays fixed in the title bar while the workspace
            // zooms; it only fades. The text variant paints no background in
            // any state, so the tooltip and keyboard focus ring identify it.
            // Padding keeps a comfortable pointer target on the content-sized
            // button. When both side panels are collapsed, the floating toggle
            // over the main container is the exit instead.
            // The bar persists across modes, so its fade id is keyed by mode:
            // each toggle crossfades the whole bar between states.
            let bar = TitleBar::new()
                .w_full()
                .h(px(48.))
                .pr_3()
                .border_b_0()
                .bg(toolbar_surface)
                .child(div().flex_1())
                .with_animation(
                    gpui_kit::ElementId::NamedInteger("title-bar-fade".into(), self.zen_mode as u64),
                    zen_fade_animation(cx.theme()),
                    |el, delta| el.opacity(delta),
                );
            if self.side_panels_collapsed() {
                return bar.into_any_element();
            }
            let faded = themes::faded_chrome(theme.colors.background, theme.colors.foreground);
            return bar
                .child(exit_zen_button("zen-mode-exit", faded, &self.focus_handle))
                .into_any_element();
        }
        let navigation_visible = self.preferences.navigation_visible();
        let navigation_label = if navigation_visible {
            "Toggle navigation"
        } else {
            "Show navigation"
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
            .unwrap_or("Workspace");
        TitleBar::new()
            .w_full()
            .h(px(48.))
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
                            .accessibility_label(if pending_back { "Back (navigating…)" } else { "Back" })
                            .icon(IconName::ArrowLeft)
                            .ghost()
                            .loading(pending_back)
                            .tooltip_with_action("Back", &NavigateBack, Some(key_context::DESKTOP))
                            .desktop_disabled(!self.can_navigate_back() && !pending_back)
                            .dispatches_desktop_action(NavigateBack, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("navigate-forward")
                            .accessibility_label(if pending_forward {
                                "Forward (navigating…)"
                            } else {
                                "Forward"
                            })
                            .icon(IconName::ArrowRight)
                            .ghost()
                            .loading(pending_forward)
                            .tooltip_with_action("Forward", &NavigateForward, Some(key_context::DESKTOP))
                            .desktop_disabled(!self.can_navigate_forward() && !pending_forward)
                            .dispatches_desktop_action(NavigateForward, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("toggle-sidebar")
                            .accessibility_label(navigation_label)
                            .toggled(navigation_visible && self.sidebar_open)
                            .icon(navigation_icon)
                            .ghost()
                            .tooltip_with_action(navigation_label, &ToggleSidebar, Some(key_context::DESKTOP))
                            .dispatches_desktop_action(ToggleSidebar, self.focus_handle.clone()),
                    )
                    .child(brand_logo(22.))
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
            .child(
                Toolbar::new("header-toolbar").small().child(
                    ToolbarGroup::new("header-actions-group")
                        .label(rust_i18n::t!("app.navigation.workspace_actions").to_string())
                        .gap_1()
                        .when(!navigation_visible, |group| {
                            group
                                .child(
                                    desktop_button("top-new-thread")
                                        .icon(IconName::Plus)
                                        .ghost()
                                        .accessibility_label("New thread")
                                        .tooltip_with_action("New thread", &NewSession, Some(key_context::DESKTOP))
                                        .desktop_disabled(!self.session_change_allowed(None))
                                        .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                                )
                                .child(
                                    desktop_button("top-utilities")
                                        .icon(IconName::GitBranch)
                                        .ghost()
                                        .accessibility_label("Workspace utilities")
                                        .tooltip_with_action(
                                            "Workspace utilities",
                                            &OpenUtilities,
                                            Some(key_context::DESKTOP),
                                        )
                                        .desktop_disabled(self.workspace.is_none())
                                        .dispatches_desktop_action(OpenUtilities, self.focus_handle.clone()),
                                )
                        })
                        .when(!navigation_visible && self.inspector_open, |group| {
                            // When the inspector is closed too, the floating
                            // toggle over the main container replaces this slot.
                            group.child(enter_zen_button("top-zen-mode-toggle", &self.focus_handle))
                        })
                        .child(
                            desktop_button("search-commands")
                                .accessibility_label("Search commands")
                                .icon(IconName::Search)
                                .ghost()
                                .tooltip_with_action("Search commands", &OpenCommandPalette, Some(key_context::DESKTOP))
                                .dispatches_desktop_action(OpenCommandPalette, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("toggle-inspector")
                                .icon(IconName::PanelRight)
                                .accessibility_label(if self.inspector_open {
                                    "Hide panel"
                                } else {
                                    "Show panel"
                                })
                                .tooltip_with_action("Toggle inspector", &ToggleInspector, Some(key_context::DESKTOP))
                                .ghost()
                                .desktop_disabled(self.workspace.is_none())
                                .dispatches_desktop_action(ToggleInspector, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("open-settings")
                                .accessibility_label("Settings")
                                .icon(IconName::Settings2)
                                .tooltip_with_action("Settings", &OpenSettings, Some(key_context::DESKTOP))
                                .ghost()
                                .dispatches_desktop_action(OpenSettings, self.focus_handle.clone()),
                        )
                        .when_some(self.session_options().and_then(|options| options.speed.as_ref()), |group, speed| {
                            group.content(
                                Tag::custom(theme.colors.secondary, theme.colors.foreground, theme.colors.border)
                                    .small()
                                    .child(speed.clone()),
                            )
                        })
                        .content(self.render_chrome_menu("header-overflow", "Window actions")),
                ),
            )
            .with_animation(
                gpui_kit::ElementId::NamedInteger("title-bar-fade".into(), self.zen_mode as u64),
                zen_fade_animation(cx.theme()),
                |el, delta| el.opacity(delta),
            )
            .into_any_element()
    }
}
