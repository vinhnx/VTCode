//! Composer panel: draft input, attachments, model/policy actions, send/cancel.

use super::*;

impl DesktopView {
    pub(super) fn render_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        // Borderless on the transcript spine: the panel carries no horizontal
        // pad of its own, so the draft starts exactly where assistant body
        // text starts. The outer centering wrapper below owns the narrow-
        // window edge inset (mirroring the scroller's row wrapper).
        let mut panel = div()
            .w_full()
            .max_w(px(self.content_width()))
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .pb_4()
            .pt_2()
            // Zen tightens the bottom pad by the step the composer group
            // loosens below, so the bottom inset matches normal mode
            // instead of shrinking with the column.
            .when(self.zen_mode, |panel| panel.pb_3());
        if !self.attachments.is_empty() {
            panel = panel.child(div().w_full().flex().flex_wrap().gap_2().children(self.attachments.iter().map(
                |attachment| {
                    let path = attachment.path.clone();
                    let is_image = files::is_image_path(&attachment.path);
                    let thumbnail = self.attachment_thumbnails.get(&path).cloned();
                    Attachment::new()
                        .small()
                        .w(px(230.))
                        .media(AttachmentMedia::new().child({
                            match thumbnail {
                                Some(image) => {
                                    img(image).size(px(40.)).rounded_sm().overflow_hidden().into_any_element()
                                }
                                None => Icon::new(if is_image {
                                    IconName::FileImage
                                } else {
                                    IconName::FileCode
                                })
                                .size_6()
                                .into_any_element(),
                            }
                        }))
                        .content(
                            AttachmentContent::new()
                                .title(AttachmentTitle::new(attachment.path.clone()))
                                .description(AttachmentDescription::new(if is_image {
                                    rust_i18n::t!("app.composer.attachment_image").to_string()
                                } else {
                                    rust_i18n::t!("app.composer.attachment_file", bytes = attachment.text.len())
                                        .to_string()
                                })),
                        )
                        .actions(
                            AttachmentActions::new()
                                .child({
                                    let view_path = path.clone();
                                    desktop_button(opaque_path_element_id("view-attachment", &path))
                                        .icon(IconName::Eye)
                                        .accessibility_label(rust_i18n::t!("app.composer.view_attachment").to_string())
                                        .ghost()
                                        .xsmall()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_file_preview_overlay(view_path.clone(), window, cx);
                                        }))
                                })
                                .child({
                                    let open_path = path.clone();
                                    let editor = self
                                        .preferences
                                        .file_open_destination()
                                        .external_editor()
                                        .unwrap_or(files::ExternalEditor::System);
                                    desktop_button(opaque_path_element_id("open-attachment", &path))
                                        .icon(IconName::ExternalLink)
                                        .accessibility_label(rust_i18n::t!("app.composer.open_attachment").to_string())
                                        .ghost()
                                        .xsmall()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_file_in_editor(&open_path, editor, window, cx);
                                        }))
                                })
                                .child(
                                    desktop_button(opaque_path_element_id("remove-attachment", &path))
                                        .icon(IconName::X)
                                        .accessibility_label(
                                            rust_i18n::t!(
                                                "app.composer.remove_attachment",
                                                path = attachment.path.as_str()
                                            )
                                            .to_string(),
                                        )
                                        .ghost()
                                        .xsmall()
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| this.remove_attachment(path.clone(), cx)),
                                        ),
                                ),
                        )
                },
            )));
        }
        if self.first_submission.is_some() && self.session_request.is_none() {
            let waiting_label = rust_i18n::t!("app.session.waiting_to_send").to_string();
            panel = panel.child(
                div()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(themes::hairline(colors.popover, colors.foreground))
                    .bg(colors.popover)
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .id("startup-send-status")
                            .test_support()
                            .role(gpui_kit::Role::Status)
                            .aria_label(waiting_label.clone())
                            .child(waiting_label),
                    )
                    .child(
                        desktop_button("cancel-startup-send")
                            .label(rust_i18n::t!("app.session.keep_as_draft").to_string())
                            .ghost()
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.retained_submission = view.first_submission.take();
                                view.status = rust_i18n::t!(i18n::COMPOSER_MESSAGE_NOT_SUBMITTED).to_string();
                                cx.notify();
                            })),
                    ),
            );
        }
        let weak_view = cx.entity().downgrade();
        // One size owns the composer geometry. The borderless input carries
        // no horizontal inset: the draft starts on the transcript spine,
        // while the button frames below start on it too and their ghost
        // labels inset by button padding (matching transcript tool rows).
        let composer_size = gpui_kit::component::Size::default();
        let input = Textarea::new(&self.composer)
            .accessibility_id("composer")
            .aria_label(rust_i18n::t!("app.composer.input_label").to_string())
            // Kit insets multiline editor text by the size input token
            // with no override (`set_editor_paddings` is crate-private
            // and unconditional), so shift the element back by that same
            // public token: the draft starts on the transcript spine.
            // Pinned with Kit =0.8.1; recheck the formula on upgrade.
            .ml(-composer_size.input_px())
            .cursor_text()
            // Files copied in Finder arrive on the clipboard as
            // `ExternalPaths`; pasting them attaches instead of letting the
            // input insert raw paths. Plain text pastes fall through.
            .on_paste(move |item, _, app| {
                let paths: Vec<PathBuf> = item
                    .entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        gpui_kit::ClipboardEntry::ExternalPaths(paths) => Some(paths.0.to_vec()),
                        _ => None,
                    })
                    .flatten()
                    .collect();
                if paths.is_empty() {
                    return false;
                }
                if let Some(view) = weak_view.upgrade() {
                    view.update(app, |view, cx| view.attach_dropped_files(&paths, cx));
                }
                true
            })
            .min_h(px(32.));

        if let Some(draft) = self.retained_submission.as_ref() {
            let prompt = draft.prompt.clone();
            let explanation = if draft.delivery_uncertain {
                rust_i18n::t!("app.composer.delivery_unconfirmed").to_string()
            } else {
                rust_i18n::t!("app.composer.draft_not_submitted").to_string()
            };
            panel = panel.child(
                div()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(themes::hairline(colors.popover, colors.foreground))
                    .bg(colors.popover)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(row_title(explanation, &colors))
                    .child(rust_i18n::t!("app.composer.clear_draft_hint").to_string())
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                desktop_button("restore-submission")
                                    .label(rust_i18n::t!("app.composer.restore_draft").to_string())
                                    .desktop_disabled(
                                        !self.composer.read(cx).value().is_empty()
                                            || !self.attachments.is_empty()
                                            || self.attachments_picker_open
                                            || self.session_request.is_some(),
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| view.restore_submission(window, cx))),
                            )
                            .child(
                                desktop_button("copy-retained-prompt")
                                    .label(rust_i18n::t!("app.composer.copy_prompt").to_string())
                                    .ghost()
                                    .on_click(move |_, window, cx| {
                                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(prompt.clone()));
                                        super::super::notifications::copied_to_clipboard(window, cx);
                                    }),
                            )
                            .child(
                                desktop_button("dismiss-retained-prompt")
                                    .label(rust_i18n::t!("app.composer.dismiss").to_string())
                                    .ghost()
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.retained_submission = None;
                                        view.prune_attachment_thumbnails();
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }

        // Hide a card left over from a previous selection: reopening via the
        // button, `/cli`, or the palette refreshes it for the current thread.
        if let Some(handoff) = self.cli_handoff.as_ref()
            && self.selected_session_id.as_deref() == Some(handoff.session_id.as_str())
        {
            panel = panel.child(self.render_cli_handoff_card(handoff, cx));
        }
        if self.queue_panel_state() == QueuePanelState::Entries {
            panel = panel.child(self.render_queued_messages(cx));
        }
        if let Some(error) = self.error.as_ref() {
            panel = panel.child(error_alert("composer-error", error.clone(), &colors));
        }
        if let Some(interaction) = self.interaction.clone() {
            panel = panel.child(self.render_interaction(&interaction, window, cx));
        }

        let can_submit = self.session_change_idle()
            && self.workspace_permission_ready()
            && !self.attachments_picker_open
            && self.retained_submission.is_none()
            && !self.composer.read(cx).value().trim().is_empty();
        let buttons = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .when(self.zen_mode, |buttons| {
                // Zen keeps the same option commands as one quiet cluster:
                // model (with effort), mode, and policy stay reachable
                // opposite the send action while every other control hides.
                buttons.justify_between().child(
                    div()
                        .id("zen-composer-context")
                        .test_support()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .overflow_hidden()
                        .with_animation("zen-composer-context-fade", zen_fade_animation(cx.theme()), |el, delta| {
                            el.opacity(delta)
                        })
                        .child(self.render_model_button(cx).max_w(px(224.)))
                        .child(self.render_mode_button(cx))
                        .child(self.render_policy_button(cx)),
                )
            })
            .when(!self.zen_mode, |buttons| {
                buttons.child(
                    // One quiet option group: tighter than the gap to the
                    // trailing model/send cluster (separate groups).
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .min_w_0()
                        .with_animation("composer-secondary-fade", zen_fade_animation(cx.theme()), |el, delta| {
                            el.opacity(delta)
                        })
                        .child(
                            desktop_button("attach-files")
                                .icon(IconName::Plus)
                                .accessibility_label(rust_i18n::t!("app.composer.attach_files").to_string())
                                .tooltip(rust_i18n::t!("app.composer.attach_files").to_string())
                                .ghost()
                                .small()
                                .desktop_disabled(
                                    self.workspace.is_none()
                                        || self.attachments_picker_open
                                        || self.session_request.is_some(),
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_attachment_picker(window, cx);
                                })),
                        )
                        .child(self.render_policy_button(cx))
                        .child(self.render_mode_button(cx)),
                )
            });
        let mut model_actions =
            div()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .when(!self.zen_mode, |actions| {
                    actions.child(self.render_model_button(cx).max_w(px(336.)).with_animation(
                        "pick-model-fade",
                        zen_fade_animation(cx.theme()),
                        |el, delta| el.opacity(delta),
                    ))
                });
        // Running trio tiers: the frequent stop takes the primary fill (no
        // red anywhere in the cluster), background stays a quiet outline,
        // and force-cancel steps down to a neutral ghost, so one focal
        // action owns the emphasis.
        if self.running {
            model_actions = model_actions
                .child(
                    composer_run_geometry(desktop_tertiary_button("background-exec"))
                        .tooltip_with_action(
                            rust_i18n::t!("app.composer.background_operation_tooltip").to_string(),
                            &BackgroundOperation,
                            Some(key_context::DESKTOP),
                        )
                        .accessibility_label(rust_i18n::t!("app.composer.background_operation").to_string())
                        .icon(IconName::PanelBottomClose)
                        .desktop_elevated()
                        .on_click(cx.listener(|this, _, _, cx| this.background_foreground_session(cx))),
                )
                .child(
                    composer_run_geometry(desktop_button("force-cancel-exec").ghost())
                        .tooltip(rust_i18n::t!("app.composer.force_cancel_tooltip").to_string())
                        .accessibility_label(rust_i18n::t!("app.composer.force_cancel").to_string())
                        .icon(IconName::OctagonX)
                        .desktop_elevated()
                        .on_click(cx.listener(|this, _, _, cx| this.force_cancel_foreground_sessions(cx))),
                )
                .child(
                    composer_run_geometry(desktop_primary_button("cancel-turn"))
                        .tooltip_with_action(
                            rust_i18n::t!("app.composer.cancel_tooltip").to_string(),
                            &CancelRun,
                            Some(key_context::DESKTOP),
                        )
                        .accessibility_label(rust_i18n::t!("app.composer.cancel").to_string())
                        .icon(IconName::SquareStop)
                        .desktop_elevated()
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_turn(cx))),
                );
        } else {
            model_actions = model_actions.child(
                composer_run_geometry(desktop_primary_button("send-prompt"))
                    .accessibility_label(rust_i18n::t!("app.composer.send").to_string())
                    .tooltip(rust_i18n::t!("app.composer.send_tooltip").to_string())
                    .icon(IconName::ArrowUp)
                    .when(can_submit, |button| button.desktop_elevated())
                    .desktop_disabled(!can_submit)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_prompt(window, cx))),
            );
        }
        let buttons = buttons.child(model_actions);
        if self.palette_kind.is_composer_popup()
            && let Some(state) = self.palette.clone()
        {
            if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                // The panel carries no horizontal pad, so the zero-size
                // anchor docks flush at its trailing edge: the
                // BottomRight-anchored popup ends on the frame edge.
                panel = panel.child(
                    div()
                        .absolute()
                        .top(gpui_kit::rems(0.5))
                        .right_0()
                        .w_0()
                        .h_0()
                        .child(self.render_command_palette(state, window, cx)),
                );
            } else {
                panel = panel.child(self.render_command_palette(state, window, cx));
            }
        }
        // The attachment picker is a composer-anchored popover, sibling to the
        // palette popups so it docks above the input the same way.
        if self.attachment_picker.is_some() {
            panel = panel.child(self.render_attachment_picker(cx));
        }
        // Normal mode folds workspace, connection, and mode controls into the
        // sidebar, Settings, and the button row; only the zen no-workspace
        // state keeps this row, where the picker is the only visible path to
        // choose one (the zen empty state has no workspace link). The frame
        // carries no horizontal inset, so the row needs none either: its
        // button frames start on the input spine directly.
        if self.zen_mode && self.workspace.is_none() {
            panel = panel.child(
                div().w_full().child(
                    div()
                        .id("composer-context")
                        .test_support()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .py_1()
                        .with_animation("composer-context-fade", zen_fade_animation(cx.theme()), |el, delta| {
                            el.opacity(delta)
                        })
                        .child(self.render_workspace_picker(cx))
                        .child(
                            desktop_button("composer-cli-webmcp")
                                .icon(IconName::Terminal)
                                .label(if self.webmcp_status.as_ref().is_some_and(|status| status.running) {
                                    rust_i18n::t!("app.composer.webmcp_connected").to_string()
                                } else {
                                    rust_i18n::t!("app.composer.connect_webmcp").to_string()
                                })
                                .accessibility_label(rust_i18n::t!("app.composer.webmcp_accessibility").to_string())
                                .tooltip(rust_i18n::t!("app.composer.webmcp_tooltip").to_string())
                                .ghost()
                                .small()
                                .on_click(cx.listener(|view, _, window, cx| view.open_webmcp_settings(window, cx))),
                        ),
                ),
            );
        }
        div().w_full().flex().justify_center().px_3().child(
            panel.child(
                div()
                    .w_full()
                    .key_context(key_context::COMPOSER)
                    .on_action(
                        cx.listener(|view, action: &Enter, window, cx| view.handle_composer_enter(action, window, cx)),
                    )
                    .on_action(cx.listener(|view, _: &QueueBatched, window, cx| {
                        view.submit_batched_prompt(window, cx);
                    }))
                    .on_action(cx.listener(|view, _: &HistoryPrevious, window, cx| {
                        // Same suggestion-first priority as the arrow captures:
                        // history must not clobber the draft under an open picker.
                        if !view.move_composer_suggestion(false, window, cx) {
                            let _recalled = view.recall_history(true, false, window, cx);
                        }
                    }))
                    .on_action(cx.listener(|view, _: &HistoryNext, window, cx| {
                        if !view.move_composer_suggestion(true, window, cx) {
                            let _recalled = view.recall_history(false, false, window, cx);
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &MoveUp, window, cx| {
                        if view.move_composer_suggestion(false, window, cx)
                            || view.recall_history(true, true, window, cx)
                        {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &MoveDown, window, cx| {
                        if view.move_composer_suggestion(true, window, cx)
                            || view.recall_history(false, true, window, cx)
                        {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &IndentInline, window, cx| {
                        // The history palette cycles matches on Tab (CLI
                        // parity). File-mention and slash suggestions keep the
                        // accept-on-Tab convention.
                        if view.palette_kind == PaletteKind::PromptHistory && view.palette.is_some() {
                            let _cycled = view.move_composer_suggestion(true, window, cx);
                        } else if !view.accept_composer_suggestion(window, cx) {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|view, _: &OutdentInline, window, cx| {
                        if view.move_composer_suggestion(false, window, cx) {
                            cx.stop_propagation();
                        } else if view.composer.read(cx).value().contains('\n') {
                            // Multiline drafts keep Kit's native outdent; agent
                            // cycling is a single-line affordance (CLI parity).
                            cx.propagate();
                        } else {
                            view.cycle_primary_agent(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_key_up(cx.listener(|view, event: &KeyUpEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "left" | "right" | "up" | "down" | "home" | "end") {
                            view.refresh_composer_suggestions_after_input_navigation(window, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _: &MouseUpEvent, window, cx| {
                            view.refresh_composer_suggestions_after_input_navigation(window, cx);
                        }),
                    )
                    .child({
                        // True borderless: the group's own tint is cleared so
                        // the input sits directly on the canvas. The
                        // placeholder and caret carry the input affordance.
                        div().id("message-composer-frame").test_support().w_full().min_w_0().child(
                            InputGroup::new("message-composer")
                                .with_size(composer_size)
                                .aria_label(rust_i18n::t!("app.composer.input_group_label").to_string())
                                .w_full()
                                .border_0()
                                .bg(cx.theme().transparent)
                                .focus_ring(false)
                                .py_2()
                                .when(self.zen_mode, |group| group.py_3())
                                .input(input)
                                .addon(
                                    InputGroupAddon::new("composer-actions")
                                        .align(InputGroupAddonAlignment::BlockEnd)
                                        // Block addons inset by the size input
                                        // token; zero it so button frames start
                                        // on the transcript spine like the draft.
                                        .px_0()
                                        .child(buttons),
                                ),
                        )
                    }),
            ),
        )
    }

    /// Model picker button with the current effort and speed, shared by the
    /// normal composer and the zen option cluster. Call sites add their own
    /// width and fade: normal mode animates the button itself, while zen
    /// fades the whole cluster at once. Zen renders the speed as quiet text
    /// instead of the filled badge; effort is quiet in both modes.
    fn render_model_button(&self, cx: &Context<Self>) -> Button {
        let colors = cx.theme().colors;
        let current_model = self
            .configuration
            .as_ref()
            .map(|configuration| {
                self.model_display_name(&configuration.provider, &configuration.model)
                    .map(str::to_owned)
                    .unwrap_or_else(|| configuration.model.clone())
            })
            .unwrap_or_else(|| rust_i18n::t!(i18n::COMPOSER_CHOOSE_MODEL).to_string());
        let model_options = self.session_options();
        let selected_effort = model_options
            .filter(|options| options.supported_efforts.contains(&options.reasoning_effort))
            .map(|options| composer_controls::runtime_option_label(&options.reasoning_effort));
        let selected_speed = model_options
            .and_then(|options| options.speed.as_deref())
            .map(composer_controls::runtime_option_label);
        let model_summary = std::iter::once(current_model.as_str())
            .chain(selected_effort.as_deref())
            .chain(selected_speed.as_deref())
            .collect::<Vec<_>>()
            .join(display::MIDDOT);
        // One muted tone for both quiet suffixes: effort always, speed in zen.
        let muted = themes::muted_text(&colors);
        let model_label = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .child(div().min_w_0().max_w(px(176.)).truncate().child(current_model))
            .when_some(selected_effort, |row, effort| {
                row.child(composer_controls::quiet_option_suffix(
                    "composer-model-effort",
                    rust_i18n::t!(i18n::COMPOSER_REASONING_EFFORT, effort = effort).to_string(),
                    &effort,
                    muted,
                ))
            })
            .when_some(selected_speed, |row, speed| {
                if self.zen_mode {
                    // Zen reads the tier as quiet text: the filled badge
                    // would compete with the send action on a
                    // distraction-free surface. Identity, role, and
                    // accessible label stay put; only the paint changes.
                    row.child(composer_controls::quiet_option_suffix(
                        "composer-model-speed",
                        rust_i18n::t!("app.composer.speed_aria", label = speed).to_string(),
                        &speed,
                        muted,
                    ))
                } else {
                    row.child(composer_controls::processing_tier_badge(&speed, cx))
                }
            });
        desktop_button("pick-model")
            .accessibility_label(model_summary)
            .child(model_label)
            .dropdown_caret(true)
            .small()
            .overflow_hidden()
            .tooltip_with_action(
                rust_i18n::t!("app.composer.choose_model_tooltip").to_string(),
                &OpenModelPicker,
                Some(key_context::DESKTOP),
            )
            .ghost()
            .desktop_disabled(!self.session_change_allowed(None) || self.attachments_picker_open)
            .dispatches_desktop_action(OpenModelPicker, self.focus_handle.clone())
    }

    /// Execution-policy picker button shared by the normal composer and the
    /// zen option cluster.
    fn render_policy_button(&self, cx: &Context<Self>) -> Button {
        let permission_mode = self.session_options().map(|options| options.permission_mode);
        let (policy, policy_label) = match permission_mode {
            Some(protocol::PermissionMode::Ask) => {
                (rust_i18n::t!(i18n::COMPOSER_PERMISSION_ASK).to_string(), "Ask".to_owned())
            }
            Some(protocol::PermissionMode::AutoApproveCommands) => {
                (rust_i18n::t!(i18n::COMPOSER_PERMISSION_AUTO).to_string(), "Auto".to_owned())
            }
            Some(protocol::PermissionMode::FullAccess) => {
                (rust_i18n::t!(i18n::COMPOSER_PERMISSION_FULL).to_string(), "Full access".to_owned())
            }
            None => match self.configuration.as_ref().and_then(|config| config.sandbox.clone()) {
                Some(policy) => (policy.clone(), policy),
                None => (
                    rust_i18n::t!("app.composer.permissions_unavailable").to_string(),
                    rust_i18n::t!("app.composer.policy").to_string(),
                ),
            },
        };
        let (permission_icon, permission_icon_color) =
            composer_controls::permission_indicator(permission_mode, cx.theme().colors);
        desktop_button("composer-policy")
            .accessibility_label(rust_i18n::t!("app.composer.execution_policy", policy = policy).to_string())
            .tooltip_with_action(
                rust_i18n::t!("app.composer.command_permissions").to_string(),
                &OpenPermissions,
                Some(key_context::DESKTOP),
            )
            .icon(Icon::new(permission_icon).text_color(permission_icon_color))
            .child(
                div()
                    .id("composer-policy-label")
                    .test_support()
                    .truncate()
                    .child(policy_label.to_string()),
            )
            .dropdown_caret(true)
            .ghost()
            .small()
            .min_w_0()
            .max_w(px(112.))
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenPermissions, self.focus_handle.clone())
    }
}

/// Shared size and radius for the composer run cluster: the idle send and
/// every running-state button render from this one definition, so the cluster
/// reads as a single control across turn states. Elevation stays gated at
/// each call site: Kit replays the instance shadow inside its disabled arm.
fn composer_run_geometry(button: Button) -> Button {
    button.large().rounded(ButtonRounded::Large)
}
