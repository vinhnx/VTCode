//! Composer panel: draft input, attachments, model/policy actions, send/cancel.

use super::*;

impl DesktopView {
    pub(super) fn render_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mut panel = div()
            .w_full()
            .max_w(px(self.content_width()))
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .pb_4()
            .pt_2()
            .when(self.zen_mode, |panel| panel.px_3().pb_3());
        if !self.attachments.is_empty() {
            panel = panel.child(div().w_full().flex().flex_wrap().gap_2().children(self.attachments.iter().map(
                |attachment| {
                    let path = attachment.path.clone();
                    Attachment::new()
                        .small()
                        .w(px(230.))
                        .media(AttachmentMedia::new().child(Icon::new(IconName::FileCode)))
                        .content(
                            AttachmentContent::new()
                                .title(AttachmentTitle::new(attachment.path.clone()))
                                .description(AttachmentDescription::new(format!(
                                    "{} bytes · workspace file",
                                    attachment.text.len()
                                ))),
                        )
                        .actions(
                            AttachmentActions::new().child(
                                desktop_button(opaque_path_element_id("remove-attachment", &path))
                                    .icon(IconName::X)
                                    .accessibility_label(format!("Remove {}", attachment.path))
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
                                view.status = "Message not submitted".into();
                                cx.notify();
                            })),
                    ),
            );
        }
        let input = Textarea::new(&self.composer)
            .accessibility_id("composer")
            .aria_label(rust_i18n::t!("app.composer.input_label").to_string())
            .cursor_text()
            .min_h(px(32.));

        if let Some(draft) = self.retained_submission.as_ref() {
            let prompt = draft.prompt.clone();
            let explanation = if draft.delivery_uncertain {
                "Delivery was not confirmed. Check session history before resending. Restore this draft or copy it before dismissing."
            } else {
                "This message was not submitted. Restore this draft or copy it before dismissing."
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(explanation)
                    .child("Clear the current draft and attachments to restore.")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                desktop_button("restore-submission")
                                    .label("Restore draft")
                                    .desktop_disabled(
                                        !self.composer.read(cx).value().is_empty()
                                            || !self.attachments.is_empty()
                                            || self.attachments_picker_open
                                            || self.session_request.is_some(),
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| view.restore_submission(window, cx))),
                            )
                            .child(desktop_button("copy-retained-prompt").label("Copy prompt").ghost().on_click(
                                move |_, window, cx| {
                                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(prompt.clone()));
                                    super::super::notifications::copied_to_clipboard(window, cx);
                                },
                            ))
                            .child(desktop_button("dismiss-retained-prompt").label("Dismiss").ghost().on_click(
                                cx.listener(|view, _, _, cx| {
                                    view.retained_submission = None;
                                    cx.notify();
                                }),
                            )),
                    ),
            );
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
        let current_model = self
            .configuration
            .as_ref()
            .map(|configuration| {
                self.models
                    .iter()
                    .find(|model| model.provider == configuration.provider && model.id == configuration.model)
                    .map(|model| model.display_name.clone())
                    .unwrap_or_else(|| configuration.model.clone())
            })
            .unwrap_or_else(|| rust_i18n::t!("app.composer.choose_model").to_string());
        let permission_mode = self.session_options().map(|options| options.permission_mode);
        let (policy, policy_label) = match permission_mode {
            Some(protocol::PermissionMode::Ask) => {
                (rust_i18n::t!("app.composer.permission_ask").to_string(), "Ask".to_owned())
            }
            Some(protocol::PermissionMode::AutoApproveCommands) => {
                (rust_i18n::t!("app.composer.permission_auto").to_string(), "Auto".to_owned())
            }
            Some(protocol::PermissionMode::FullAccess) => {
                (rust_i18n::t!("app.composer.permission_full").to_string(), "Full access".to_owned())
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
        let buttons = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .when(self.zen_mode, |buttons| {
                // Zen replaces the control cluster with one quiet status line:
                // the send action keeps the trailing slot, the model/policy
                // context fills the leading space. Changes stay on the command
                // palette keyboard path, so this text is descriptive, not a control.
                let context = format!("{current_model} · {policy_label}");
                buttons.justify_between().child(
                    div()
                        .id("zen-composer-context")
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(context.clone())
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_left()
                        .text_xs()
                        .text_color(themes::muted_text(&colors))
                        .child(context),
                )
            })
            .when(!self.zen_mode, |buttons| {
                buttons.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .with_animation("composer-secondary-fade", zen_fade_animation(cx.theme()), |el, delta| {
                            el.opacity(delta)
                        })
                        .child(
                            desktop_button("attach-files")
                                .icon(IconName::Plus)
                                .accessibility_label(rust_i18n::t!("app.composer.add_context").to_string())
                                .tooltip_with_action(
                                    rust_i18n::t!("app.composer.add_context").to_string(),
                                    &OpenAddContext,
                                    Some(key_context::DESKTOP),
                                )
                                .ghost()
                                .small()
                                .desktop_disabled(
                                    self.workspace.is_none()
                                        || self.attachments_picker_open
                                        || self.session_request.is_some(),
                                )
                                .dispatches_desktop_action(OpenAddContext, self.focus_handle.clone()),
                        )
                        .child(
                            desktop_button("composer-policy")
                                .accessibility_label(
                                    rust_i18n::t!("app.composer.execution_policy", policy = policy).to_string(),
                                )
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
                                .dispatches_desktop_action(OpenPermissions, self.focus_handle.clone()),
                        ),
                )
            });
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
            .join(" · ");
        let model_label = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .child(div().min_w_0().max_w(px(176.)).truncate().child(current_model))
            .when_some(selected_effort, |row, effort| {
                row.child(
                    div()
                        .id("composer-model-effort")
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(rust_i18n::t!("app.composer.reasoning_effort", effort = effort).to_string())
                        .flex_shrink_0()
                        .text_color(themes::muted_text(&colors))
                        .child(format!("· {effort}")),
                )
            })
            .when_some(selected_speed, |row, speed| row.child(composer_controls::processing_tier_badge(&speed, cx)));
        let mut model_actions =
            div()
                .min_w_0()
                .max_w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .when(!self.zen_mode, |actions| {
                    actions.child(
                        desktop_button("pick-model")
                            .accessibility_label(model_summary)
                            .child(model_label)
                            .dropdown_caret(true)
                            .small()
                            .max_w(px(336.))
                            .overflow_hidden()
                            .tooltip_with_action(
                                rust_i18n::t!("app.composer.choose_model_tooltip").to_string(),
                                &OpenModelPicker,
                                Some(key_context::DESKTOP),
                            )
                            .ghost()
                            .desktop_disabled(!self.session_change_allowed(None) || self.attachments_picker_open)
                            .dispatches_desktop_action(OpenModelPicker, self.focus_handle.clone())
                            .with_animation("pick-model-fade", zen_fade_animation(cx.theme()), |el, delta| {
                                el.opacity(delta)
                            }),
                    )
                });
        if self.running {
            model_actions = model_actions.child(
                desktop_button("cancel-turn")
                    .tooltip_with_action(
                        rust_i18n::t!("app.composer.cancel_tooltip").to_string(),
                        &CancelRun,
                        Some(key_context::DESKTOP),
                    )
                    .accessibility_label(rust_i18n::t!("app.composer.cancel").to_string())
                    .icon(IconName::Square)
                    .rounded_full()
                    .danger()
                    .when(self.zen_mode, |button| button.small())
                    .dispatches_desktop_action(CancelRun, self.focus_handle.clone()),
            );
        } else {
            model_actions = model_actions.child(
                desktop_button("send-prompt")
                    .accessibility_label(rust_i18n::t!("app.composer.send").to_string())
                    .tooltip(rust_i18n::t!("app.composer.send_tooltip").to_string())
                    .icon(IconName::ArrowUp)
                    .rounded_full()
                    .primary()
                    .when(self.zen_mode, |button| button.small())
                    .desktop_disabled(!can_submit)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_prompt(window, cx))),
            );
        }
        let buttons = buttons.child(model_actions);
        if self.palette_kind.is_composer_popup()
            && let Some(state) = self.palette.clone()
        {
            if matches!(self.palette_kind, PaletteKind::Models | PaletteKind::ModelOptions) {
                panel = panel.child(
                    div()
                        .absolute()
                        .top(gpui_kit::rems(0.5))
                        .right(gpui_kit::rems(1.))
                        .w_0()
                        .h_0()
                        .child(self.render_command_palette(state, window, cx)),
                );
            } else {
                panel = panel.child(self.render_command_palette(state, window, cx));
            }
        }
        // Zen hides this row, except when no workspace is set yet: the picker is
        // the only visible path to choose one (the zen empty state has no workspace link).
        if !self.zen_mode || self.workspace.is_none() {
            panel = panel.child(
                div()
                    .id("composer-context")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_1()
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
                    )
                    .child(self.render_mode_button(cx)),
            );
        }
        // Zen restyles the card while the column narrows (see content_width):
        // a defined input surface with a hairline border, roomier padding,
        // and a small send action. The border lives on the frame only; the
        // inner group stays borderless with no focus ring, so the card never
        // shifts its boundary when the draft gains focus — the caret marks
        // the focused field.
        let frame_radius = px(if self.zen_mode { 16. } else { 22. });
        div().w_full().flex().justify_center().child(
            panel.child(
                div()
                    .w_full()
                    .on_action(cx.listener(|view, _: &Enter, window, cx| view.handle_composer_enter(window, cx)))
                    .capture_action(cx.listener(|view, _: &MoveUp, window, cx| {
                        if view.move_composer_suggestion(false, window, cx) {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &MoveDown, window, cx| {
                        if view.move_composer_suggestion(true, window, cx) {
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    }))
                    .capture_action(cx.listener(|view, _: &IndentInline, window, cx| {
                        if !view.accept_composer_suggestion(window, cx) {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
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
                    .child(
                        div()
                            .id("message-composer-frame")
                            .test_support()
                            .w_full()
                            .min_w_0()
                            .rounded(frame_radius)
                            .bg(colors.popover)
                            .when(self.zen_mode, |frame| frame.border_1().border_color(colors.border))
                            .child(
                                InputGroup::new("message-composer")
                                    .aria_label(rust_i18n::t!("app.composer.input_group_label").to_string())
                                    .w_full()
                                    .rounded(frame_radius)
                                    .border_0()
                                    .focus_ring(false)
                                    .p_2()
                                    .when(self.zen_mode, |group| group.p_3())
                                    .bg(colors.popover)
                                    .input(input)
                                    .addon(
                                        InputGroupAddon::new("composer-actions")
                                            .align(InputGroupAddonAlignment::BlockEnd)
                                            .child(buttons),
                                    ),
                            ),
                    ),
            ),
        )
    }
}
