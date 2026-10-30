//! Runtime interaction prompts: question stepper, freeform answers, and choice rows.
use super::*;

impl DesktopView {
    pub(super) fn render_interaction(
        &self,
        interaction: &InteractionRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let submitting = self.pending_interaction_response.as_deref() == Some(interaction.interaction_id.as_str());
        let mut panel = div()
            .id("runtime-interaction")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(interaction.title.clone())
            .track_focus(&self.interaction_focus)
            .w_full()
            .max_w(px(self.content_width()))
            .flex()
            .flex_col()
            .gap_2()
            .border_1()
            .border_color(colors.warning)
            .rounded_md()
            .p_4()
            .bg(colors.warning.opacity(0.08))
            .child(div().font_semibold().child(interaction.title.clone()));
        for detail in &interaction.details {
            panel = panel.child(div().text_sm().child(detail.clone()));
        }
        if submitting {
            panel = panel.child(
                div()
                    .id("interaction-response-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.interaction.sending_aria").to_string())
                    .text_sm()
                    .child(rust_i18n::t!(i18n::COMPOSER_SENDING_DECISION).to_string()),
            );
        }

        if interaction.kind == protocol::InteractionKind::Question && interaction.steps.len() > 1 {
            let step_index = self.interaction_step_index.min(interaction.steps.len().saturating_sub(1));
            let steps = interaction.steps.iter().enumerate().map(|(index, step)| {
                let title = if step.title.trim().is_empty() {
                    rust_i18n::t!("app.interaction.step_fallback", n = index + 1).to_string()
                } else {
                    step.title.clone()
                };
                StepperItem::new().child(div().min_w_0().max_w_full().truncate().child(title))
            });
            panel = panel.child(
                div()
                    .id("interaction-question-progress")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.interaction.progress_aria").to_string())
                    .w_full()
                    .child(
                        div()
                            .id("interaction-question-stepper")
                            .test_support()
                            .role(gpui_kit::Role::List)
                            .aria_label(rust_i18n::t!("app.interaction.steps_aria").to_string())
                            .w_full()
                            .child(
                                Stepper::new("interaction-question-steps")
                                    .small()
                                    .w_full()
                                    .selected_index(step_index)
                                    .disabled(true)
                                    .items(steps),
                            ),
                    ),
            );
        }

        if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
            let step_index = self.interaction_step_index.min(interaction.steps.len().saturating_sub(1));
            if let Some(step) = interaction.steps.get(step_index) {
                panel = panel.child(self.render_question_step(step, step_index, interaction.steps.len(), window, cx));
            }
        } else {
            let choices = interaction
                .steps
                .first()
                .map(|step| step.choices.as_slice())
                .unwrap_or(interaction.choices.as_slice());
            for choice in choices {
                panel = panel.child(self.render_choice(choice, 0, window, cx));
            }
        }

        panel.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    desktop_button("cancel-interaction")
                        .label(rust_i18n::t!("app.interaction.cancel_request").to_string())
                        .ghost()
                        .desktop_disabled(submitting)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.submit_interaction(Vec::new(), true, window, cx)),
                        ),
                )
                .when(
                    interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty(),
                    |row| {
                        row.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .when(self.interaction_step_index > 0, |row| {
                                    row.child(
                                        desktop_button("previous-question")
                                            .label(rust_i18n::t!("app.interaction.previous").to_string())
                                            .icon(IconName::ChevronLeft)
                                            .ghost()
                                            .desktop_disabled(submitting)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_interaction_step(
                                                    this.interaction_step_index.saturating_sub(1),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                })
                                .child(
                                    desktop_button("continue-question")
                                        .label(if self.interaction_step_index + 1 == interaction.steps.len() {
                                            rust_i18n::t!("app.interaction.submit_answers").to_string()
                                        } else {
                                            rust_i18n::t!("app.interaction.next_question").to_string()
                                        })
                                        .primary()
                                        .desktop_disabled(submitting)
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.advance_interaction_step(window, cx)
                                            }),
                                        ),
                                ),
                        )
                    },
                ),
        )
    }

    fn render_question_step(
        &self,
        step: &InteractionStep,
        index: usize,
        total: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let selected_choice_id = self.interaction_choice_ids.get(index).and_then(Option::as_deref);
        let selected_choice_index =
            selected_choice_id.and_then(|selected| step.choices.iter().position(|choice| choice.id == selected));
        let choice_radios = step.choices.iter().enumerate().map(|(choice_index, choice)| {
            let label = if choice.recommended {
                rust_i18n::t!("app.interaction.recommended", label = choice.label.as_str()).to_string()
            } else {
                choice.label.clone()
            };
            let selected = selected_choice_index == Some(choice_index);
            let mut radio = Radio::new(format!("interaction-question-choice-{choice_index}"))
                .label(label.clone())
                .accessibility_label(label)
                .checked(selected)
                .disabled(self.pending_interaction_response.is_some())
                .w_full()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(if selected {
                    colors.primary.opacity(0.6)
                } else {
                    colors.border
                })
                .bg(if selected {
                    colors.primary.opacity(0.08)
                } else {
                    colors.popover
                });
            let choice_id = choice.id.clone();
            radio = radio
                .aria_position_in_set(choice_index + 1)
                .aria_size_of_set(step.choices.len())
                .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                    if *checked {
                        this.choose_interaction_option(index, choice_id.clone(), window, cx);
                    }
                }));
            if let Some(description) = choice.description.as_ref() {
                radio = radio.child(div().text_sm().text_color(colors.muted_foreground).child(description.clone()));
            }
            radio
        });
        let options = div()
            .id("interaction-question-options")
            .test_support()
            .role(gpui_kit::Role::RadioGroup)
            .aria_label(step.question.clone())
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .children(choice_radios);
        let mut content = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("question-step-counter")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.interaction.step_counter", n = index + 1, total = total).to_string())
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.interaction.step_counter", n = index + 1, total = total).to_string()),
            )
            .child(div().font_medium().child(step.question.clone()));
        if !step.choices.is_empty() {
            content = content.child(options);
        }
        if step.allow_freeform {
            content = content.child(
                div().w_full().rounded_sm().bg(colors.popover).p_2().child(
                    Textarea::new(&self.question_answer)
                        .accessibility_id("question-answer")
                        .aria_label(rust_i18n::t!("app.interaction.your_answer").to_string())
                        .cursor_text()
                        .disabled(self.pending_interaction_response.is_some())
                        .bg(colors.popover)
                        .h(px(72.)),
                ),
            );
        }
        content.into_any_element()
    }

    fn render_choice(
        &self,
        choice: &InteractionChoice,
        step_index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let choice_id = choice.id.clone();
        let label = if choice.recommended {
            rust_i18n::t!("app.interaction.recommended", label = choice.label.as_str()).to_string()
        } else {
            choice.label.clone()
        };
        let selected = self
            .interaction
            .as_ref()
            .is_some_and(|interaction| interaction.kind == protocol::InteractionKind::Question)
            && self.interaction_choice_ids.get(step_index).and_then(Option::as_deref) == Some(choice.id.as_str());
        let mut button = desktop_button(format!("choice-{}", choice.id))
            .accessibility_label(if selected {
                rust_i18n::t!("app.interaction.selected_label", label = label.as_str()).to_string()
            } else {
                label.clone()
            })
            .label(label)
            .w_full()
            .ghost()
            .desktop_disabled(self.pending_interaction_response.is_some())
            .selected(selected)
            .when(selected, |button| button.icon(IconName::Check));
        if let Some(description) = choice.description.as_ref() {
            button = button.tooltip(description.clone());
        }
        div().w_full().child(button.on_click(cx.listener(move |this, _, window, cx| {
            this.choose_interaction_option(step_index, choice_id.clone(), window, cx);
        })))
    }
}
