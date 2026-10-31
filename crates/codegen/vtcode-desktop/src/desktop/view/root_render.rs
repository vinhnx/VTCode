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
        let conversation = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .when(!self.open_tabs.is_empty() && !self.zen_mode, |column| column.child(self.render_thread_tabs(cx)))
            .when(!self.transcript.is_empty() && !self.zen_mode && !self.pending_blank_transition(), |column| {
                column.child(self.render_thread_header(cx.theme()))
            })
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
            .child(conversation)
            .when(inspector_visible, |workspace| {
                workspace.child(
                    div()
                        .relative()
                        .h_full()
                        .flex_shrink_0()
                        .child(self.render_inspector(cx))
                        .child(self.render_resize_handle(ResizeEdge::Inspector, cx)),
                )
            });
        // Route panels embed in the content slot beneath the persistent
        // toolbar: chrome stays mounted and the panel inherits the pane
        // rhythm instead of taking over the window.
        // Route panels embed in the content slot beneath the persistent
        // toolbar: chrome stays mounted and the panel inherits the pane
        // rhythm instead of taking over the window.
        let center: AnyElement = match self.current_route() {
            Route::Workspace => workspace.into_any_element(),
            Route::Settings => self.render_settings_page(window, cx),
            Route::Browser => self.render_thread_browser(cx),
            Route::Review => self.render_change_review(cx),
        };
        let content = div()
            .id("pane-root")
            .test_support()
            .p_3()
            .flex_1()
            .min_h_0()
            .flex()
            .when(!self.preferences.translucent_sidebar || !navigation_visible, |pane| {
                pane.bg(chrome_background(&colors))
            })
            .child(center);
        // Navigation sits beside this column instead of below the toolbar,
        // so the sidebar spans the full window height.
        let main = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .flex_col()
            .child(self.render_toolbar(cx))
            .child(content)
            .when(self.preferences.show_status_bar && !self.zen_mode, |main| main.child(self.render_status_bar(cx)));
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
            .text_color(colors.foreground)
            .track_focus(&self.focus_handle)
            .when(!self.capture_suppresses_bindings(window), |body| body.key_context(key_context::DESKTOP))
            .when(self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &NewSession, window, cx| {
                    view.close_active_route(window, cx);
                    view.create_session(cx);
                }))
            })
            .when(self.selected_session_id.is_some() && self.session_change_allowed(None), |body| {
                body.on_action(cx.listener(|view, _: &CloseSession, window, cx| {
                    view.close_active_route(window, cx);
                    view.close_session(cx);
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
            .on_action(cx.listener(|view, _: &RouteBack, window, cx| {
                if !view.go_route_back(window, cx) {
                    cx.propagate();
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
            .on_action(
                cx.listener(|view, _: &OpenSettingsSearch, window, cx| view.open_settings_search_palette(window, cx)),
            )
            .when(self.can_select_session_options(), |body| {
                body.on_action(cx.listener(|view, _: &OpenPermissions, window, cx| view.open_permissions(window, cx)))
                    .on_action(cx.listener(|view, _: &OpenModePicker, window, cx| view.open_modes(window, cx)))
            })
            .on_action(cx.listener(|view, _: &OpenAddContext, window, cx| view.open_add_context(window, cx)))
            .on_action(cx.listener(|view, _: &OpenSettings, window, cx| view.toggle_settings(window, cx)))
            .on_action(cx.listener(|view, action: &tabs::CloseThreadTab, _, cx| view.close_tab(&action.session_id, cx)))
            .on_action(cx.listener(|view, action: &tabs::CloseOtherThreadTabs, _, cx| {
                view.close_other_tabs(&action.session_id, cx)
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
        if !(self.current_route() == Route::Settings && self.palette_kind == PaletteKind::SettingsSearch)
            && !self.palette_kind.is_composer_popup()
            && let Some(state) = self.palette.clone()
        {
            body = body.child(self.render_command_palette(state, window, cx));
        }
        if self.full_access_confirm.is_some() {
            body = body.child(self.render_full_access_confirm(cx));
        }
        body
    }
}
