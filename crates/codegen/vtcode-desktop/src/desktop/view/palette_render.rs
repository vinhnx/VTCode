//! Modal command palette and anchored composer popups.
use super::*;
use gpui_kit::EntityId;
use gpui_kit::component::kbd::Kbd;

impl DesktopView {
    /// Modal ⌘K palette. The state entity is created on open, never during
    /// render; confirm and cancel run through the owning view.
    pub(super) fn render_command_palette(
        &self,
        state: Entity<CommandState>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let ui_font_size = cx.theme().font_size;
        let popup_margin = ui_font_size * 0.75;
        let viewport_width = window.viewport_size().width;
        let max_popup_width = if viewport_width > popup_margin * 2. {
            viewport_width - popup_margin * 2.
        } else {
            px(0.)
        };
        let weak = cx.entity().downgrade();
        let groups = if self.palette_kind == PaletteKind::AddContext {
            share_palette_commands(self.composer_resource_groups())
        } else {
            self.palette_groups.clone()
        };
        let confirm_groups = groups
            .iter()
            .map(|(label, entries)| {
                (
                    label.clone(),
                    entries
                        .iter()
                        .filter(|entry| {
                            self.palette_kind == PaletteKind::AddContext
                                || self.palette_action_available(&entry.action, cx)
                        })
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            })
            .filter(|(_, entries)| self.palette_kind == PaletteKind::AddContext || !entries.is_empty())
            .collect::<Vec<_>>();
        let models = self.palette_kind == PaletteKind::Models;
        let prompt_history = self.palette_kind == PaletteKind::PromptHistory;
        let modes = self.palette_kind == PaletteKind::Modes;
        let conversation_search = self.palette_kind == PaletteKind::ConversationSearch;
        let older_history_available = self.history_has_earlier;
        let history_page_loading = self.history_page_loading;
        let history_page_failed = self.history_page_error.is_some();
        let transcript_has_capacity = self.transcript.remaining_capacity() > 0;
        let composer_popup = self.palette_kind.is_composer_popup();
        let inline_completion = matches!(self.palette_kind, PaletteKind::FileMentions | PaletteKind::SlashCommands);
        let title = match self.palette_kind {
            PaletteKind::Models => rust_i18n::t!(i18n::COMPOSER_CHOOSE_MODEL).to_string(),
            PaletteKind::ConversationSearch => rust_i18n::t!("app.palette.title.conversation_search").to_string(),
            PaletteKind::ModelOptions => rust_i18n::t!("app.composer.select_model").to_string(),
            PaletteKind::AddContext => rust_i18n::t!(i18n::COMPOSER_ADD_CONTEXT).to_string(),
            PaletteKind::FileMentions => rust_i18n::t!("app.palette.title.file_mention").to_string(),
            PaletteKind::SlashCommands => rust_i18n::t!("app.palette.title.slash_commands").to_string(),
            PaletteKind::PromptHistory => rust_i18n::t!("app.palette.title.prompt_history").to_string(),
            PaletteKind::Permissions => rust_i18n::t!("app.palette.title.permissions").to_string(),
            PaletteKind::Modes => rust_i18n::t!("app.palette.title.modes").to_string(),
            PaletteKind::Fonts(FontTarget::Ui) => rust_i18n::t!("app.palette.title.ui_font").to_string(),
            PaletteKind::Fonts(FontTarget::Code) => rust_i18n::t!("app.palette.title.code_font").to_string(),
            PaletteKind::SettingsSearch => rust_i18n::t!(i18n::SETTINGS_SEARCH_TITLE).to_string(),
            PaletteKind::ConfigSettings => rust_i18n::t!(i18n::PALETTE_TITLE_CONFIGURATION).to_string(),
            PaletteKind::ReviewFiles => rust_i18n::t!("app.palette.title.changed_file").to_string(),
            PaletteKind::Commands => rust_i18n::t!("app.palette.title.commands").to_string(),
            PaletteKind::GoTo => rust_i18n::t!(i18n::PALETTE_TITLE_GO_TO).to_string(),
        };
        let quick_model = self.palette_kind == PaletteKind::ModelOptions;
        // Prompt history filters host-side, so its rows are empty whenever the
        // query misses. "No history yet" must then come from the unfiltered
        // catalog, never from an empty filtered list — otherwise a typo reads
        // as a wiped history.
        let catalog_empty = if prompt_history {
            self.prompt_history_is_empty()
        } else {
            confirm_groups.is_empty()
        };
        let placeholder = if self.palette_kind == PaletteKind::SettingsSearch {
            rust_i18n::t!("app.settings.search.placeholder").to_string()
        } else if self.palette_kind == PaletteKind::ConfigSettings {
            rust_i18n::t!("app.palette.placeholder.configuration").to_string()
        } else if matches!(self.palette_kind, PaletteKind::Fonts(_)) {
            rust_i18n::t!("app.palette.placeholder.fonts").to_string()
        } else if models {
            rust_i18n::t!("app.composer.search_models").to_string()
        } else if self.palette_kind == PaletteKind::FileMentions {
            rust_i18n::t!("app.palette.placeholder.workspace_files").to_string()
        } else if self.palette_kind == PaletteKind::SlashCommands {
            rust_i18n::t!("app.palette.placeholder.commands").to_string()
        } else if self.palette_kind == PaletteKind::ReviewFiles {
            rust_i18n::t!("app.palette.placeholder.changed_files").to_string()
        } else if conversation_search {
            rust_i18n::t!("app.palette.placeholder.conversation").to_string()
        } else if prompt_history {
            rust_i18n::t!("app.palette.placeholder.filter_history").to_string()
        } else if modes {
            rust_i18n::t!("app.palette.placeholder.modes").to_string()
        } else {
            rust_i18n::t!("app.palette.placeholder.default").to_string()
        };
        let empty_title = if models && catalog_empty {
            rust_i18n::t!("app.palette.empty.no_models").to_string()
        } else if prompt_history && catalog_empty {
            rust_i18n::t!("app.palette.empty.no_prompts").to_string()
        } else if modes && catalog_empty {
            rust_i18n::t!("app.palette.empty.modes_unavailable").to_string()
        } else if conversation_search && older_history_available {
            rust_i18n::t!("app.palette.empty.no_loaded_match").to_string()
        } else {
            rust_i18n::t!("app.palette.empty.no_results").to_string()
        };
        let empty_description = if models && catalog_empty {
            rust_i18n::t!("app.palette.empty.models_reload").to_string()
        } else if prompt_history && catalog_empty {
            rust_i18n::t!("app.palette.empty.prompts_hint").to_string()
        } else if modes && catalog_empty {
            rust_i18n::t!("app.palette.empty.modes_reload").to_string()
        } else if conversation_search && older_history_available {
            rust_i18n::t!("app.palette.empty.older_history").to_string()
        } else {
            rust_i18n::t!("app.palette.empty.try_different_search").to_string()
        };
        let mut command = Command::new(&state)
            .placeholder(placeholder)
            .max_h(px(360.))
            .when(models, |command| {
                let title = rust_i18n::t!(i18n::COMPOSER_CHOOSE_MODEL).to_string();
                command.header(move |_, _, _| div().px_3().py_2().font_semibold().child(title.clone()))
            })
            .when(conversation_search, |command| {
                let muted_foreground = colors.muted_foreground;
                command.header(move |_, _, _| {
                    div()
                        .px_3()
                        .py_2()
                        .text_xs()
                        .text_color(muted_foreground)
                        .child(rust_i18n::t!("app.palette.conversation_search_header").to_string())
                })
            })
            .when(prompt_history, |command| {
                let scope_weak = weak.clone();
                let palette_id = state.entity_id();
                let current = self.history_scope;
                let scopes = [HistoryScope::Everywhere, HistoryScope::Project, HistoryScope::Session];
                command.header(move |_, _, _| {
                    let weak = scope_weak.clone();
                    div().id("history-scope-header").px_3().py_2().child(
                        gpui_kit::component::button::ToggleGroup::new("history-scope")
                            .segmented()
                            .small()
                            .children(scopes.map(|scope| {
                                Toggle::new(format!("history-scope-{}", scope.label()))
                                    .label(scope.title())
                                    .checked(scope == current)
                            }))
                            .on_click(move |states, _, cx| {
                                let _updated = weak.update(cx, |view, cx| {
                                    if view.palette_kind != PaletteKind::PromptHistory
                                        || !view.palette_is_current(palette_id)
                                    {
                                        return;
                                    }
                                    // Segmented toggles flip independently: the
                                    // scope whose state differs from the render
                                    // is the click target.
                                    let clicked = states.iter().enumerate().position(|(index, on)| {
                                        *on != scopes.get(index).is_some_and(|scope| *scope == view.history_scope)
                                    });
                                    if let Some(scope) = clicked.and_then(|index| scopes.get(index).copied()) {
                                        view.set_history_scope(scope, cx);
                                    }
                                });
                            }),
                    )
                })
            })
            .empty(move |_, _, _| {
                if quick_model {
                    div().h(px(0.)).into_any_element()
                } else {
                    empty_state(empty_title.clone(), empty_description.clone(), IconName::Search).into_any_element()
                }
            });
        if inline_completion {
            command = command.searchable(false);
        }
        if prompt_history {
            // The prompt-history rows are already matched, ranked, deduped and
            // capped by the shared CLI ranker, so Kit must not re-filter them:
            // its substring test is exactly the divergence this replaced.
            command = command.filterable(false);
            // Reverse search is inline: the composer text is the filter, so
            // the panel shows rows only and keeps no search field of its own.
            command = command.searchable(false);
        }
        let show_history_control =
            conversation_search && (older_history_available || history_page_loading || history_page_failed);
        if show_history_control {
            let weak = weak.clone();
            let palette_id = state.entity_id();
            let session_id = self.selected_session_id.clone();
            let can_load_history =
                older_history_available && transcript_has_capacity && !history_page_loading && !self.running;
            let history_label = if history_page_loading {
                rust_i18n::t!(i18n::PALETTE_HISTORY_LOADING).to_string()
            } else if !transcript_has_capacity && older_history_available {
                rust_i18n::t!("app.palette.history.limit_reached").to_string()
            } else if history_page_failed {
                rust_i18n::t!("app.palette.history.retry").to_string()
            } else {
                rust_i18n::t!(i18n::PALETTE_HISTORY_LOAD).to_string()
            };
            let history_error = rust_i18n::t!("app.palette.history.retry_hint").to_string();
            command = command.footer(move |_, _, _| {
                let weak = weak.clone();
                let session_id = session_id.clone();
                let history_label = history_label.clone();
                let history_error = history_error.clone();
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .when(history_page_failed, |footer| {
                        footer.child(div().text_xs().text_color(colors.danger).child(history_error))
                    })
                    .child(
                        desktop_button("conversation-search-load-earlier")
                            .label(history_label)
                            .icon(IconName::ArrowUp)
                            .ghost()
                            .small()
                            .desktop_disabled(!can_load_history)
                            .on_click(move |_, _, cx| {
                                let _updated = weak.update(cx, |view, cx| {
                                    if view.palette_kind != PaletteKind::ConversationSearch
                                        || !view.palette_is_current(palette_id)
                                        || view.selected_session_id != session_id
                                    {
                                        return;
                                    }
                                    view.request_earlier_history(cx);
                                });
                            }),
                    )
            });
        }
        if quick_model {
            let slider = self.effort_slider.clone();
            let configuration = self.configuration.clone();
            let speed_toggle = configuration.as_ref().and_then(|configuration| {
                let options = configuration.session_options.as_ref()?;
                if !options.fast_mode_supported {
                    return None;
                }
                Some((
                    composer_controls::runtime_option_label(options.fast_mode_label.as_deref()?),
                    options.fast_mode_enabled,
                    configuration.provider.clone(),
                    configuration.model.clone(),
                    self.palette.as_ref().map(|state| state.entity_id()),
                ))
            });
            let speed_toggle_view = weak.clone();
            let label = configuration.as_ref().map(|config| {
                self.model_display_name(&config.provider, &config.model)
                    .map(str::to_owned)
                    .unwrap_or_else(|| config.model.clone())
            });
            let effort_focus = self.effort_focus.clone();
            let effort_levels = self.visible_effort_levels();
            let options_enabled = self.can_select_session_options();
            command = command.searchable(false).header(move |_, _, cx| {
                let speed_control = speed_toggle.as_ref().map(|(label, enabled, provider, model, palette_id)| {
                    let weak = speed_toggle_view.clone();
                    let label = label.clone();
                    let enabled = *enabled;
                    let provider = provider.clone();
                    let model = model.clone();
                    let palette_id = *palette_id;
                    Toggle::new("fast-mode-toggle")
                        .label(label.clone())
                        .icon(IconName::Activity)
                        .tooltip(if enabled {
                            rust_i18n::t!("app.palette.disable_processing", label = label.as_str()).to_string()
                        } else {
                            rust_i18n::t!("app.palette.enable_processing", label = label.as_str()).to_string()
                        })
                        .outline()
                        .small()
                        .checked(enabled)
                        .rounded_full()
                        .px_2()
                        .font_semibold()
                        .when(enabled && label == "Ultrafast", |toggle| {
                            toggle
                                .bg(colors.primary)
                                .text_color(colors.primary_foreground)
                                .border_color(colors.primary)
                        })
                        .disabled(!options_enabled)
                        .when(options_enabled, |toggle| toggle.cursor_pointer())
                        .on_click(move |checked, _window, cx| {
                            let _updated = weak.update(cx, |view, cx| {
                                if view.palette_kind != PaletteKind::ModelOptions
                                    || view.palette.as_ref().map(|state| state.entity_id()) != palette_id
                                    || !view.model_is_current(&provider, &model)
                                {
                                    return;
                                }
                                // Keep the model options panel open so processing-tier
                                // toggles do not dismiss the knob.
                                let _selected = view.select_session_options(
                                    protocol::SessionOptionsSelectParams {
                                        fast_mode: Some(*checked),
                                        ..Default::default()
                                    },
                                    cx,
                                );
                            });
                        })
                        .into_any_element()
                });
                composer_controls::model_options_header(
                    configuration.as_ref(),
                    &effort_levels,
                    slider.as_ref(),
                    &effort_focus,
                    label.as_deref(),
                    speed_control,
                    options_enabled,
                    cx,
                )
            });
            let search_all_view = weak.clone();
            let search_all_palette_id = state.entity_id();
            command = command.footer(move |_, _, _| {
                let view = search_all_view.clone();
                div().id("model-search-all-footer").px_3().py_2().child(
                    desktop_button("model-search-all")
                        .label(rust_i18n::t!("app.composer.search_all_models").to_string())
                        .icon(IconName::Search)
                        .ghost()
                        .small()
                        .w_full()
                        .on_click(move |_, window, cx| {
                            let _updated = view.update(cx, |view, cx| {
                                if view.palette_kind == PaletteKind::ModelOptions
                                    && view.palette_is_current(search_all_palette_id)
                                {
                                    view.open_model_palette(window, cx);
                                }
                            });
                        }),
                )
            });
        }
        let palette_query = self.palette_query.clone();
        for (label, entries) in &confirm_groups {
            let mut group = CommandGroup::new().label(label.clone());
            for entry in entries {
                let label = entry.label.clone();
                let description = entry.description.clone();
                let query = palette_query.clone();
                let selected = self.is_current_palette_action(&entry.action);
                let enabled = self.palette_action_available(&entry.action, cx);
                let id = palette_entry_element_id(&entry.action);
                let shortcut = self.palette_action_keystroke(&entry.action);
                let accessible_label = description
                    .as_ref()
                    .map(|details| format!("{label} · {details}"))
                    .unwrap_or_else(|| label.clone());
                let permission_icon = permission_palette_mode(self.palette_kind, &entry.action)
                    .map(|mode| composer_controls::permission_indicator(Some(mode), colors));
                // Commands rows always reserve the leading slot (even when a
                // future action maps to no icon) so labels share one spine.
                // Hoisted: the row closure below is `'static` and cannot borrow `self`.
                let is_commands = self.palette_kind == PaletteKind::Commands;
                let commands_icon = if is_commands {
                    palette_action_icon(&entry.action)
                } else {
                    None
                };
                group = group.item(
                    CommandItem::new()
                        .disabled(!enabled)
                        .label(label.clone())
                        .checked(selected)
                        .keywords(entry.keywords.clone())
                        .keywords(entry.shared_keywords.clone())
                        .child(move |_, _| {
                            div()
                                .id(id.clone())
                                .test_support()
                                .flex_1()
                                .min_w_0()
                                .when(enabled, |row| row.cursor_pointer())
                                .flex()
                                .items_center()
                                .gap_3()
                                .py_1()
                                .role(gpui_kit::Role::Label)
                                .aria_label(accessible_label.clone())
                                .when(is_commands, |row| {
                                    row.child(
                                        transcript_icon_slot(match commands_icon {
                                            Some(icon) => Icon::new(icon)
                                                .small()
                                                .text_color(colors.muted_foreground)
                                                .into_any_element(),
                                            None => div().into_any_element(),
                                        })
                                        .id(format!("{id}-icon"))
                                        .test_support(),
                                    )
                                })
                                .when_some(permission_icon, |row, (icon, color)| {
                                    row.child(Icon::new(icon).small().text_color(color))
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            gpui_kit::component::label::Label::new(label.clone())
                                                .highlights(query.clone())
                                                .font_medium()
                                                .truncate(),
                                        )
                                        .when_some(description.clone(), |row, details| {
                                            row.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.muted_foreground)
                                                    .truncate()
                                                    .child(details),
                                            )
                                        }),
                                )
                                .when_some(shortcut.clone(), |row, keystroke| {
                                    row.child(
                                        div()
                                            .id(format!("{id}-shortcut"))
                                            .test_support()
                                            .flex_none()
                                            .role(gpui_kit::Role::Label)
                                            .aria_label(
                                                rust_i18n::t!(
                                                    "app.shortcuts.badge_aria",
                                                    label = label.clone(),
                                                    value = Kbd::format(&keystroke)
                                                )
                                                .to_string(),
                                            )
                                            .child(Kbd::new(keystroke).outline()),
                                    )
                                })
                        }),
                );
            }
            command = command.group(group);
        }
        let confirm_weak = weak.clone();
        let cancel_weak = weak.clone();
        let query_weak = weak.clone();
        let palette_id = state.entity_id();
        let command = command
            .on_query(move |query, _, cx| {
                let _updated = query_weak.update(cx, |view, cx| {
                    if !view.palette_is_current(palette_id) {
                        return;
                    }
                    // No PromptHistory arm: the panel is rows-only
                    // (`searchable(false)`), so Kit never calls back — the
                    // draft drives refreshes instead.
                    view.palette_query = query.into();
                    cx.notify();
                });
            })
            .on_confirm(move |path, _window, cx: &mut App| {
                let Some(action) = confirm_groups
                    .get(path.section)
                    .and_then(|(_, entries)| entries.get(path.row))
                    .map(|entry| entry.action.clone())
                else {
                    return;
                };
                let _updated = confirm_weak.update(cx, |view, cx| {
                    if !view.palette_is_current(palette_id) {
                        return;
                    }
                    view.run_palette_action(action, _window, cx);
                });
            })
            .on_cancel(move |window, cx: &mut App| {
                let _updated = cancel_weak.update(cx, |view, cx| {
                    if view.palette_is_current(palette_id) {
                        view.close_palette(window, cx);
                    }
                });
            });

        if self.current_route() == Route::Settings && self.palette_kind == PaletteKind::SettingsSearch {
            // The embedded search keeps its own mouse-downs: the settings
            // panel dismisses the search on any other press inside it.
            return div()
                .id(COMMAND_PALETTE_ELEMENT_ID)
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label(title.clone())
                .w_full()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(command)
                .into_any_element();
        }

        let panel = div()
            .id(COMMAND_PALETTE_ELEMENT_ID)
            .test_support()
            .role(gpui_kit::Role::Dialog)
            .aria_label(title)
            .w(gpui_kit::rems(if quick_model {
                22.
            } else if models {
                24.5
            } else {
                35.
            }))
            .max_w(max_popup_width)
            .when(models || quick_model || inline_completion, |panel| {
                panel.rounded_xl().bg(colors.popover).overflow_hidden()
            })
            .on_mouse_down_out(move |_, window, cx| {
                let _updated = weak.update(cx, |view, cx| {
                    if view.palette_is_current(palette_id) {
                        view.close_palette(window, cx);
                    }
                });
            })
            .on_action(|_: &gpui_kit::base::actions::Cancel, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(command.with_animation(
                ("command-palette-content", state.entity_id()),
                Animation::new(cx.theme().motion_tokens().duration_fast).with_easing({
                    let easing = cx.theme().motion_tokens().easing_enter.clone();
                    move |progress| easing.sample(progress).clamp(0., 1.)
                }),
                |content, progress| content.opacity(progress),
            ))
            .focus_trap(COMMAND_PALETTE_ELEMENT_ID, &self.palette_focus);
        if composer_popup {
            return deferred(
                anchored()
                    .anchor(if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                        Anchor::BottomRight
                    } else {
                        Anchor::BottomLeft
                    })
                    .offset(point(px(0.), ui_font_size * -0.5))
                    .snap_to_window_with_margin(popup_margin)
                    .child(panel),
            )
            .priority(2)
            .into_any_element();
        }
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .pt(gpui_kit::rems(5.625))
            .bg(modal_scrim(&colors))
            .child(panel)
            .into_any_element()
    }

    /// Whether the live palette is still the popup that installed this
    /// callback. Guards against a superseded popup executing or dismissing
    /// its replacement.
    #[must_use]
    fn palette_is_current(&self, palette_id: EntityId) -> bool {
        self.palette.as_ref().is_some_and(|palette| palette.entity_id() == palette_id)
    }
}

impl DesktopView {
    /// Saved binding behind a palette action for the trailing badge viewer.
    /// Reads saved preferences (never the staged settings draft) and hides
    /// unbound or invalid values instead of badgeing them.
    pub(super) fn palette_action_keystroke(&self, action: &PaletteAction) -> Option<gpui_kit::Keystroke> {
        let id = palette_action_command_id(action)?;
        let command = shortcuts::COMMANDS.iter().find(|command| command.id == id)?;
        let staged = shortcuts::staged_value(&self.preferences.shortcuts, command);
        if staged.is_empty() {
            return None;
        }
        gpui_kit::Keystroke::parse_with_os(staged, shortcuts::host_operating_system()).ok()
    }
}

/// Configurable command behind a palette action, when the action runs the
/// command exactly. Near-miss neighbors (theme toggle vs theme sync, a
/// specific model vs the model picker, the shortcuts deeplink itself) stay
/// badge-free so the viewer never implies a chord that runs something else.
pub(super) fn palette_action_command_id(action: &PaletteAction) -> Option<shortcuts::CommandId> {
    match action {
        PaletteAction::NewSession => Some(shortcuts::CommandId::NewSession),
        PaletteAction::CloseSession => Some(shortcuts::CommandId::CloseSession),
        PaletteAction::OpenConversationSearch { .. } => Some(shortcuts::CommandId::ConversationSearch),
        PaletteAction::ChooseModel => Some(shortcuts::CommandId::Model),
        PaletteAction::ChooseWorkspace => Some(shortcuts::CommandId::Workspace),
        PaletteAction::ToggleInspector => Some(shortcuts::CommandId::Inspector),
        PaletteAction::ToggleZenMode => Some(shortcuts::CommandId::ZenMode),
        PaletteAction::OpenDeepLink(link) => match link.as_ref() {
            deeplinks::DeepLink::Command(id) => Some(*id),
            _ => None,
        },
        _ => None,
    }
}

/// Keep palette label identity attached to the command when search/filtering changes row order.
/// Hash payloads so file paths and draft text do not appear in rendered element IDs.
pub(super) fn palette_entry_element_id(action: &PaletteAction) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(action).hash(&mut hasher);
    match action {
        PaletteAction::SelectFont { target, family } => {
            match target {
                FontTarget::Ui => 0_u8,
                FontTarget::Code => 1_u8,
            }
            .hash(&mut hasher);
            family.hash(&mut hasher);
        }
        PaletteAction::EditConfigSetting(value)
        | PaletteAction::LoadSession(value)
        | PaletteAction::PreviewFile(value)
        | PaletteAction::SelectWorkspaceReviewFile(value)
        | PaletteAction::AttachFile(value)
        | PaletteAction::InsertFileMention(value)
        | PaletteAction::CompleteSlashCommand(value)
        | PaletteAction::InsertHistoryPrompt(value)
        | PaletteAction::InsertDraft(value) => value.hash(&mut hasher),
        PaletteAction::OpenConversationSearch { session_id } => session_id.hash(&mut hasher),
        PaletteAction::JumpToTranscriptItem { session_id, item_id, from_search } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
            from_search.hash(&mut hasher);
        }
        PaletteAction::ShowActivityOutput { session_id, item_id } => {
            session_id.hash(&mut hasher);
            item_id.hash(&mut hasher);
        }
        PaletteAction::SelectSettingsTab(tab) => std::mem::discriminant(tab).hash(&mut hasher),
        PaletteAction::SelectModel { provider, model } => {
            provider.hash(&mut hasher);
            model.hash(&mut hasher);
        }
        PaletteAction::SelectSessionOptions(options) => {
            match options.permission_mode {
                None => 0_u8,
                Some(protocol::PermissionMode::Ask) => 1,
                Some(protocol::PermissionMode::AutoApproveCommands) => 2,
                Some(protocol::PermissionMode::FullAccess) => 3,
            }
            .hash(&mut hasher);
            options.mode.hash(&mut hasher);
            options.reasoning_effort.hash(&mut hasher);
            options.fast_mode.hash(&mut hasher);
        }
        PaletteAction::OpenDeepLink(link) => link.path().unwrap_or_else(|| format!("{link:?}")).hash(&mut hasher),
        PaletteAction::NewSession
        | PaletteAction::CloseSession
        | PaletteAction::CopyThreadTitle
        | PaletteAction::CopyFilePath
        | PaletteAction::CloseOtherTabs
        | PaletteAction::OpenInCli
        | PaletteAction::CopyLatestMessage
        | PaletteAction::ChooseModel
        | PaletteAction::ChooseWorkspace
        | PaletteAction::RefreshSessions
        | PaletteAction::RefreshFiles
        | PaletteAction::BrowseThreads
        | PaletteAction::ToggleInspector
        | PaletteAction::ToggleZenMode
        | PaletteAction::ToggleTheme
        | PaletteAction::AttachFiles
        | PaletteAction::OpenGoTo => {}
    }
    format!("palette-label-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_shortcut_badges_cover_exact_commands_only() {
        use shortcuts::CommandId;
        // Mapped: the action runs the command exactly.
        assert_eq!(palette_action_command_id(&PaletteAction::NewSession), Some(CommandId::NewSession));
        assert_eq!(palette_action_command_id(&PaletteAction::CloseSession), Some(CommandId::CloseSession));
        assert_eq!(
            palette_action_command_id(&PaletteAction::OpenConversationSearch { session_id: "s".into() }),
            Some(CommandId::ConversationSearch)
        );
        assert_eq!(palette_action_command_id(&PaletteAction::ChooseModel), Some(CommandId::Model));
        assert_eq!(palette_action_command_id(&PaletteAction::ChooseWorkspace), Some(CommandId::Workspace));
        assert_eq!(palette_action_command_id(&PaletteAction::ToggleInspector), Some(CommandId::Inspector));
        assert_eq!(palette_action_command_id(&PaletteAction::ToggleZenMode), Some(CommandId::ZenMode));
        // Unmapped asymmetric sides: near-miss neighbors stay badge-free so no
        // badge implies a chord that runs something else.
        assert_eq!(palette_action_command_id(&PaletteAction::ToggleTheme), None);
        assert_eq!(palette_action_command_id(&PaletteAction::CopyLatestMessage), None);
        assert_eq!(palette_action_command_id(&PaletteAction::BrowseThreads), None);
        assert_eq!(palette_action_command_id(&PaletteAction::RefreshFiles), None);
        assert_eq!(
            palette_action_command_id(&PaletteAction::SelectModel { provider: "p".into(), model: "m".into() }),
            None
        );
        assert_eq!(
            palette_action_command_id(&PaletteAction::SelectSettingsTab(settings::SettingsTab::Shortcuts)),
            None
        );
        assert_eq!(palette_action_command_id(&PaletteAction::InsertHistoryPrompt("hi".into())), None);
    }
}
