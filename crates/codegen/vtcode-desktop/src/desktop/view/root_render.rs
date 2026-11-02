//! Root layout: navigation sidebar, conversation workspace, inspector, and overlays.
use super::*;

impl DesktopView {
    /// Native window title: the selected thread title, or the app name when
    /// no thread is selected or its title is blank.
    pub(super) fn window_title(&self) -> &str {
        self.sessions
            .iter()
            .find(|session| Some(session.session_id.as_str()) == self.selected_session_id.as_deref())
            .and_then(|session| session.title.as_deref())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(app::NAME)
    }
}

impl Render for DesktopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(self.window_title());
        let colors = cx.theme().colors;
        let inspector_visible = self.inspector_open && self.workspace.is_some() && !self.zen_mode;
        let navigation_visible = self.preferences.navigation_visible() && !self.zen_mode;
        let navigation_on_right = self.preferences.navigation_on_right();
        // Compact layout: an open panel yields the sidebar to its rail so the
        // embedded slot stays usable. The preference is untouched, so the
        // sidebar returns when the panel closes or the window widens.
        let narrow_panel =
            window.viewport_size().width < px(NARROW_PANEL_BREAKPOINT) && self.current_route() != Route::Workspace;
        // One identity row: several tabs get the switch strip, while zero or one
        // tab gets the compact header for a thread with content or a selection
        // (a lone tab would only repeat the window title). The header carries
        // the conversation-column new-thread action the hidden strip would
        // otherwise take with it. Threads with neither stay on the welcome or
        // pending states, which own their own calls to action.
        let multi_tab = self.open_tabs.len() > 1;
        let conversation = div()
            .id("conversation-column")
            .test_support()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .when(multi_tab && !self.zen_mode, |column| column.child(self.render_thread_tabs(cx)))
            .when(
                !multi_tab
                    && (!self.transcript.is_empty() || self.selected_session_id.is_some())
                    && !self.zen_mode
                    && !self.pending_blank_transition(),
                |column| column.child(self.render_thread_header(cx.theme())),
            )
            .when(self.pending_navigation.is_some() && !self.pending_blank_transition(), |column| {
                column.child(self.render_navigation_loading(cx))
            })
            .child(self.render_transcript(cx))
            .child(self.render_composer(window, cx));
        let workspace = div()
            .id("workspace-surface")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.navigation.conversation_workspace").to_string())
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .gap_3()
            .p_4()
            .bg(colors.background)
            .rounded_xl()
            // Zen is the distraction-free surface: the card runs edge to edge,
            // so square corners meet the title bar and window sides flush.
            .when(self.zen_mode, |card| card.rounded_none())
            .child(conversation);
        // The inspector is a separated card beside the workspace surface,
        // not nested in it: the row gap matches the pane padding so the
        // two cards float on the chrome with one even rhythm.
        let inspector = inspector_visible.then(|| {
            div()
                .relative()
                .h_full()
                .flex_shrink_0()
                .child(self.render_inspector(cx))
                .child(self.render_resize_handle(ResizeEdge::Inspector, cx))
        });
        // Route panels embed in the content slot beneath the persistent
        // toolbar: chrome stays mounted and the panel inherits the pane
        // rhythm instead of taking over the window.
        let center: AnyElement = match self.current_route() {
            Route::Workspace => div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .h_full()
                .flex()
                .gap_3()
                .child(workspace)
                .children(inspector)
                .into_any_element(),
            Route::Settings => self.render_settings_page(window, cx),
            Route::Browser => self.render_thread_browser(window, cx),
            Route::Review => self.render_change_review(window, cx),
        };
        let content = div()
            .id("pane-root")
            .test_support()
            .p_3()
            // Fullscreen hides the native traffic lights and titlebar, so the
            // chrome-side rhythm reads as a dead band; hug the panes to the
            // toolbar there while windowed mode keeps the breathing room.
            .when(window.is_fullscreen(), |pane| pane.pt_0())
            // Zen is distraction-free: the conversation spans the window edge
            // to edge beneath the title bar, with no chrome rhythm inset.
            .when(self.zen_mode, |pane| pane.p_0())
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.preferences.translucent_sidebar || !navigation_visible, |pane| {
                pane.bg(chrome_background(&colors))
            })
            .child(center);
        // Navigation sits beside this column instead of below the toolbar,
        // so the sidebar spans the full window height. Zen drops the toolbar
        // entirely: the drag strip takes its place above the edge-to-edge
        // content (a fullscreen window cannot move, so nothing renders
        // there) and the zen toggle floats above it, painted after the
        // content.
        let top_chrome = if self.zen_mode {
            (!window.is_fullscreen()).then(|| self.render_zen_drag_strip(cx))
        } else {
            Some(self.render_toolbar(window, cx))
        };
        let main = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .flex_col()
            .children(top_chrome)
            .child(content)
            .when(self.preferences.show_status_bar && !self.zen_mode, |main| main.child(self.render_status_bar(cx)))
            .when(self.zen_mode, |main| main.child(self.render_zen_toggle_floating(cx)));
        let mut body = div()
            .size_full()
            .flex()
            .when(!self.preferences.translucent_sidebar || !navigation_visible, |body| body.bg(colors.background))
            // The sidebar and inspector are siblings now, so the resize drag
            // ends on the body to catch releases over either pane.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.resizing.take().is_some() {
                        this.save_preferences();
                        cx.notify();
                    }
                }),
            )
            // Whole-window drop target for external files (Finder → composer
            // attachments). Gated on a workspace being open: a drop without
            // one would no-op, so the affordance must not advertise it.
            // Internal drags carry other payload types and never match.
            .when(self.workspace.is_some(), |body| {
                body.on_drop(
                    cx.listener(|view, paths: &ExternalPaths, _, cx| view.attach_dropped_files(paths.paths(), cx)),
                )
            })
            .text_color(colors.foreground)
            .track_focus(&self.focus_handle)
            .when(!self.capture_suppresses_bindings(window), |body| body.key_context(key_context::DESKTOP))
            .when(self.new_thread_available(), |body| {
                body.on_action(cx.listener(|view, _: &NewSession, window, cx| {
                    view.close_active_route(window, cx);
                    let _ = view.create_session_from_ui(window, cx);
                }))
            })
            .when(self.selected_session_id.is_some() && self.new_thread_available(), |body| {
                body.on_action(cx.listener(|view, _: &CloseSession, window, cx| {
                    view.close_active_route(window, cx);
                    let _ = view.close_session_from_ui(window, cx);
                }))
            })
            .when(
                self.retained_submission.is_none() && self.workspace_switch_idle() && !self.workspace_picker_open,
                |body| {
                    body.on_action(
                        cx.listener(|view, _: &ChooseWorkspace, window, cx| view.choose_workspace(window, cx)),
                    )
                },
            )
            .on_action(cx.listener(|view, _: &ToggleSidebar, _, cx| {
                if view.preferences.navigation_visible() {
                    view.sidebar_open = !view.sidebar_open;
                } else {
                    view.preferences.navigation_visible = Some(true);
                    view.save_preferences();
                }
                view.navigation_peek = false;
                cx.notify();
            }))
            .on_action(cx.listener(|view, _: &NavigateBack, window, cx| {
                // Route steps win while a panel owns the content slot; session
                // history resumes once the route cursor cannot move.
                if !view.go_route_back(window, cx) {
                    view.navigate_back(cx);
                }
            }))
            .on_action(cx.listener(|view, _: &NavigateForward, window, cx| {
                if !view.go_route_forward(window, cx) {
                    view.navigate_forward(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|view, _, _, cx| {
                    if view.swipe_navigation_allowed() {
                        view.navigate_back(cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|view, _, _, cx| {
                    if view.swipe_navigation_allowed() {
                        view.navigate_forward(cx);
                    }
                }),
            )
            .on_action(cx.listener(|view, _: &ToggleZenMode, window, cx| view.toggle_zen_mode(window, cx)))
            .on_action(cx.listener(|view, _: &ToggleStatusBar, _, cx| view.toggle_status_bar(cx)))
            .on_action(cx.listener(|view, _: &ReloadCliSettings, window, cx| {
                view.reload_cli_settings_from_ui(window, cx);
            }))
            .on_action(cx.listener(|view, _: &SyncCliTheme, _, cx| view.sync_theme_to_cli(cx)))
            .on_action(cx.listener(|view, _: &FocusComposer, window, cx| view.activate_composer(window, cx)))
            .when(self.workspace.is_some(), |body| {
                body.on_action(cx.listener(|view, _: &ToggleInspector, _, cx| view.toggle_inspector(cx)))
                    .on_action(cx.listener(|view, _: &OpenUtilities, window, cx| {
                        view.close_active_route(window, cx);
                        view.open_utilities(cx);
                    }))
                    .on_action(cx.listener(|view, _: &OpenWorkspaceFiles, window, cx| {
                        view.focus_workspace_files(window, cx);
                    }))
                    .on_action(cx.listener(|view, _: &RefreshWorkspace, _, cx| view.refresh_workspace_view(cx)))
            })
            .when(self.backend.is_some() && self.service_info.is_some(), |body| {
                body.on_action(cx.listener(|view, _: &OpenWorkspaceReview, window, cx| {
                    view.open_workspace_review(window, cx);
                }))
            })
            .when(self.running, |body| body.on_action(cx.listener(|view, _: &CancelRun, _, cx| view.cancel_turn(cx))))
            .on_action(cx.listener(|view, _: &DismissOverlay, window, cx| view.dismiss_overlay(window, cx)))
            .on_action(cx.listener(|view, _: &OpenCommandPalette, window, cx| view.open_command_palette(window, cx)))
            .on_action(cx.listener(|view, _: &OpenThreadBrowser, window, cx| view.open_thread_browser(window, cx)))
            .on_action(cx.listener(|view, _: &OpenConversationSearch, window, cx| {
                if let Some(session_id) = view.selected_session_id.clone() {
                    view.open_conversation_search(session_id, window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &OpenPromptHistory, window, cx| view.open_prompt_history(window, cx)))
            .on_action(cx.listener(|view, _: &TakeLatestQueued, window, cx| {
                if view.composer.read(cx).focus_handle(cx).is_focused(window) {
                    view.take_latest_queued(cx);
                }
            }))
            .on_action(
                cx.listener(|view, _: &OpenSettingsSearch, window, cx| view.open_settings_search_palette(window, cx)),
            )
            .when(self.can_select_session_options(), |body| {
                body.on_action(cx.listener(|view, _: &OpenPermissions, window, cx| view.open_permissions(window, cx)))
                    .on_action(cx.listener(|view, _: &OpenModePicker, window, cx| view.open_modes(window, cx)))
            })
            .on_action(cx.listener(|view, _: &OpenAddContext, window, cx| view.open_add_context(window, cx)))
            .on_action(cx.listener(|view, _: &OpenSettings, window, cx| view.toggle_settings(window, cx)))
            .on_action(cx.listener(|view, _: &AboutApplication, window, cx| view.open_about(window, cx)))
            .on_action(cx.listener(|view, action: &tabs::CloseThreadTab, window, cx| {
                view.close_tab(&action.session_id, window, cx)
            }))
            .on_action(cx.listener(|view, action: &tabs::CloseOtherThreadTabs, window, cx| {
                view.close_other_tabs(&action.session_id, window, cx)
            }))
            .on_action(
                cx.listener(|view, action: &tabs::CopyThreadTitle, _, cx| {
                    view.copy_thread_title(&action.session_id, cx)
                }),
            )
            .on_action(cx.listener(|view, action: &tabs::DeleteThread, window, cx| {
                view.confirm_delete_thread(&action.session_id, window, cx)
            }))
            .on_action(cx.listener(|view, action: &configuration::CopyConfigPath, _, cx| {
                view.copy_config_path(&action.path, cx)
            }))
            .on_action(cx.listener(|view, action: &configuration::CopyConfigValue, _, cx| {
                view.copy_config_value(&action.path, cx)
            }))
            .on_action(
                cx.listener(|view, action: &files_panel::CopyFilePath, _, cx| view.copy_file_path(&action.path, cx)),
            )
            .on_action(cx.listener(|view, action: &files_panel::OpenFilePath, window, cx| {
                view.open_file_preview(action.path.to_string(), window, cx)
            }))
            .when(self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &OpenModelPicker, window, cx| view.open_model_options(window, cx)))
            })
            .on_action(cx.listener(|_, _: &CloseWindow, window, cx| menu_bar::close_or_hide(window, cx)))
            .on_action(cx.listener(|_, _: &MinimizeWindow, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &ZoomWindow, window, _| window.zoom_window()))
            .on_action(cx.listener(|_, _: &ToggleFullScreen, window, _| window.toggle_fullscreen()));
        if navigation_visible {
            let navigation = self.render_navigation(narrow_panel, cx);
            let sidebar = div().relative().h_full().flex_shrink_0();
            let sidebar = if navigation_on_right {
                sidebar
                    .when(self.sidebar_open && !narrow_panel, |sidebar| {
                        sidebar.child(self.render_resize_handle(ResizeEdge::Sidebar, cx))
                    })
                    .child(navigation)
            } else {
                sidebar.child(navigation).when(self.sidebar_open && !narrow_panel, |sidebar| {
                    sidebar.child(self.render_resize_handle(ResizeEdge::Sidebar, cx))
                })
            };
            if navigation_on_right {
                body = body.child(main).child(sidebar);
            } else {
                body = body.child(sidebar).child(main);
            }
        } else {
            body = body.child(main);
        }
        if self.file_preview.is_some() {
            body = body.child(self.render_file_preview(cx));
        }
        if self.license_viewer.is_some() {
            body = body.child(self.render_license_viewer(cx));
        }
        if !(self.current_route() == Route::Settings && self.palette_kind == PaletteKind::SettingsSearch)
            && !self.palette_kind.is_composer_popup()
            && let Some(state) = self.palette.clone()
        {
            body = body.child(self.render_command_palette(state, window, cx));
        }
        if self.full_access_confirm.is_some() {
            body = body.child(self.render_full_access_confirm(cx));
        }
        if self.running_confirm.is_some() {
            body = body.child(self.render_running_confirm(cx));
        }
        // About is the topmost modal: it opens from the application menu over
        // any route, so it paints last and Escape dismisses it first.
        if self.about_open {
            body = body.child(self.render_about(cx));
        }
        // Drop affordance mounts above content but beneath the modals, and
        // only where a drop can actually attach.
        if self.workspace.is_some() {
            body = body.child(render_drop_overlay(cx));
        }
        body
    }
}

/// Full-window drop affordance: fully transparent while idle, and while
/// external files hover the window the kit's `drag_over` refinement lifts it
/// to a scrim with an accent ring and a labeled pill, so the target is
/// explicit. Pure style — no state or listeners — and transparent hitboxes do
/// not block the body drop listener or button hover beneath it.
fn render_drop_overlay(cx: &Context<DesktopView>) -> AnyElement {
    let colors = cx.theme().colors;
    let scrim = colors.background.opacity(0.45);
    div()
        .id("drop-overlay")
        .test_support()
        .absolute()
        .inset_0()
        .opacity(0.)
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.opacity(1.).bg(scrim))
        .flex()
        .items_center()
        .justify_center()
        .child(div().absolute().inset_2().rounded_lg().border_2().border_color(colors.primary))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .bg(colors.popover)
                .border_1()
                .border_color(colors.border)
                .shadow_md()
                .px_4()
                .py_2()
                .child(Icon::new(IconName::FileUp).small().text_color(colors.primary))
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(rust_i18n::t!("app.files.drop_to_attach").to_string()),
                ),
        )
        .into_any_element()
}
