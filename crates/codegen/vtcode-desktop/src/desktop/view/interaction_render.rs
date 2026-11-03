//! Runtime interaction prompts: question stepper, freeform answers, and choice rows.
use super::*;
use gpui_kit::component::kbd::Kbd;

/// Highest 1-based digit shortcut. Panels carry at most a handful of rows
/// (tool-loop grants show four); rows past 9 stay clickable and Tab-reachable.
const MAX_DIGIT_SHORTCUT: usize = 9;

/// Clamped wizard step for the current interaction state.
fn visible_step_index(interaction: &InteractionRequest, step_index: usize) -> usize {
    step_index.min(interaction.steps.len().saturating_sub(1))
}

/// Rows the user actually sees: the active wizard step's options for
/// multi-step questions, else the first step's (or top-level) choice list.
/// Render, the digit shortcuts, and the immediate Confirm path share this so
/// numbers, highlight, and submission always agree on the same rows.
pub(super) fn visible_choices(interaction: &InteractionRequest, step_index: usize) -> &[InteractionChoice] {
    if interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty() {
        let step_index = visible_step_index(interaction, step_index);
        match interaction.steps.get(step_index) {
            Some(step) => &step.choices,
            None => &[],
        }
    } else if let Some(step) = interaction.steps.first() {
        &step.choices
    } else {
        &interaction.choices
    }
}

/// Multi-step question prompts render the wizard (stepper + Previous/Next);
/// every other prompt is an immediate select-then-confirm panel.
pub(super) fn is_wizard_interaction(interaction: &InteractionRequest) -> bool {
    interaction.kind == protocol::InteractionKind::Question && !interaction.steps.is_empty()
}

/// 1-based digit chip for rows inside the shortcut range.
fn digit_shortcut(choice_index: usize) -> Option<usize> {
    (choice_index < MAX_DIGIT_SHORTCUT).then_some(choice_index + 1)
}

/// One selectable option card: label, inline description, digit chip.
struct ChoiceCard<'a> {
    element_id: String,
    choice: &'a InteractionChoice,
    /// 1-based position for assistive technology.
    position: usize,
    total: usize,
    checked: bool,
    /// 1-based digit shortcut chip; `None` past [`MAX_DIGIT_SHORTCUT`] or
    /// while a freeform answer owns the panel's keys.
    digit: Option<usize>,
    step_index: usize,
}

impl DesktopView {
    pub(super) fn render_interaction(
        &self,
        interaction: &InteractionRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let submitting = self.pending_interaction_response.as_deref() == Some(interaction.interaction_id.as_str());
        let is_question = interaction.kind == protocol::InteractionKind::Question;
        let is_wizard = is_wizard_interaction(interaction);
        let step_index = visible_step_index(interaction, self.interaction_step_index);
        // A visible freeform answer owns every keystroke: typing digits must
        // never commit a row, so panel shortcuts stay off while it renders.
        let freeform_visible = is_question && interaction.steps.get(step_index).is_some_and(|step| step.allow_freeform);
        // Approvals carry the warning treatment; plain choices stay neutral so
        // the tone keeps meaning "review carefully".
        let is_approval =
            matches!(interaction.kind, protocol::InteractionKind::Approval | protocol::InteractionKind::PlanApproval);
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
            .border_color(if is_approval { colors.warning } else { colors.border })
            .rounded_md()
            .p_4()
            .when(is_approval, |panel| panel.bg(colors.warning.opacity(0.08)))
            .child(div().font_semibold().child(interaction.title.clone()));
        // The first detail line carries the decision facts; the rest recede as
        // supporting metadata.
        for (detail_index, detail) in interaction.details.iter().enumerate() {
            panel = panel.child(
                div()
                    .text_sm()
                    .text_color(if detail_index == 0 {
                        colors.foreground
                    } else {
                        themes::muted_text(&colors)
                    })
                    .child(detail.clone()),
            );
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
        if !freeform_visible {
            panel = panel.on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let modifiers = &event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                let Some(interaction) = this.interaction.clone() else {
                    return;
                };
                if this.pending_interaction_response.as_deref() == Some(interaction.interaction_id.as_str()) {
                    return;
                }
                if event.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    this.submit_interaction(Vec::new(), true, window, cx);
                    return;
                }
                let key = event.keystroke.key.as_str();
                if key.len() != 1 {
                    return;
                }
                let Some(digit @ b'1'..=b'9') = key.as_bytes().first().copied() else {
                    return;
                };
                let choice_index = (digit - b'1') as usize;
                let step_index = visible_step_index(&interaction, this.interaction_step_index);
                if let Some(choice) = visible_choices(&interaction, step_index).get(choice_index) {
                    let choice_id = choice.id.clone();
                    cx.stop_propagation();
                    if is_wizard_interaction(&interaction) {
                        this.choose_interaction_option(step_index, choice_id, window, cx);
                    } else {
                        // Digits keep their immediate-commit contract; only
                        // mouse clicks became select-then-confirm.
                        this.submit_interaction(vec![choice_id], false, window, cx);
                    }
                }
            }));
        }

        let step_choices = visible_choices(interaction, step_index);
        // A one-item stepper restates the title and counter without aiding
        // navigation, so progress renders only for genuine multi-step flows.
        if is_wizard && interaction.steps.len() > 1 {
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

        if is_wizard {
            if let Some(step) = interaction.steps.get(step_index) {
                panel = panel.child(self.render_question_step(step, step_index, interaction.steps.len(), window, cx));
            }
        } else {
            let choices = step_choices;
            // The highlight is the mouse selection when set, else the service
            // default — the same row Confirm submits.
            let effective_id = self.effective_immediate_choice_id();
            let effective_index = effective_id
                .as_deref()
                .and_then(|id| choices.iter().position(|choice| choice.id == id));
            let total = choices.len();
            let mut options = div()
                .id("interaction-choice-options")
                .test_support()
                .role(gpui_kit::Role::RadioGroup)
                .aria_label(interaction.title.clone())
                .w_full()
                .flex()
                .flex_col()
                .gap_2();
            for (choice_index, choice) in choices.iter().enumerate() {
                options = options.child(self.render_choice(choice, choice_index, total, effective_index, cx));
            }
            panel = panel.child(options);
        }

        // The hint promises digit shortcuts, so it renders only where rows
        // exist to choose and no freeform answer owns the keys.
        if !freeform_visible && !step_choices.is_empty() {
            panel = panel.child(
                div()
                    .id("interaction-keyboard-hint")
                    .test_support()
                    .text_xs()
                    .text_color(themes::muted_text(&colors))
                    .child(rust_i18n::t!("app.interaction.choice_hint").to_string()),
            );
        }

        panel.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                // The decision row is its own group, not another option:
                // one extra step over the panel gap lands it on the lg rung.
                .mt_2()
                .child(
                    desktop_button("cancel-interaction")
                        .label(rust_i18n::t!("app.interaction.cancel_request").to_string())
                        .ghost()
                        .desktop_disabled(submitting)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.submit_interaction(Vec::new(), true, window, cx)),
                        ),
                )
                .when(!is_wizard && !step_choices.is_empty(), |row| {
                    // Immediate choices commit nothing on their own: activating
                    // the checked card is a no-op, so the highlighted row
                    // needs an explicit default commitment next to Cancel.
                    row.child(
                        desktop_button("confirm-interaction")
                            .label(rust_i18n::t!("app.interaction.confirm").to_string())
                            .primary()
                            .desktop_disabled(submitting)
                            .on_click(cx.listener(|this, _, window, cx| this.confirm_immediate_choice(window, cx))),
                    )
                })
                .when(is_wizard, |row| {
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
                                        cx.listener(|this, _, window, cx| this.advance_interaction_step(window, cx)),
                                    ),
                            ),
                    )
                }),
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
            self.render_choice_card(
                ChoiceCard {
                    element_id: format!("interaction-question-choice-{choice_index}"),
                    choice,
                    position: choice_index + 1,
                    total: step.choices.len(),
                    checked: selected_choice_index == Some(choice_index),
                    digit: if step.allow_freeform {
                        None
                    } else {
                        digit_shortcut(choice_index)
                    },
                    step_index: index,
                },
                cx,
            )
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
        let mut content = div().w_full().flex().flex_col().gap_2().when(total > 1, |content| {
            // "Question 1 of 1" states nothing the title and question do not
            // already say; the counter earns its line only with siblings.
            content.child(
                div()
                    .id("question-step-counter")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.interaction.step_counter", n = index + 1, total = total).to_string())
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.interaction.step_counter", n = index + 1, total = total).to_string()),
            )
        });
        content = content.child(div().font_medium().child(step.question.clone()));
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
                        .h(gpui_kit::rems(4.5)),
                ),
            );
        }
        content.into_any_element()
    }

    /// One selectable option card shared by question steps and immediate
    /// choices: bordered card, inline description, trailing digit chip.
    fn render_choice_card(&self, card: ChoiceCard<'_>, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let label = if card.choice.recommended {
            rust_i18n::t!("app.interaction.recommended", label = card.choice.label.as_str()).to_string()
        } else {
            card.choice.label.clone()
        };
        let disabled = self.pending_interaction_response.is_some();
        let mut radio = Radio::new(card.element_id)
            .label(label.clone())
            .accessibility_label(label)
            .checked(card.checked)
            .disabled(disabled)
            .w_full()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(if card.checked {
                colors.primary.opacity(0.6)
            } else {
                colors.border
            })
            .bg(if card.checked {
                colors.primary.opacity(0.08)
            } else {
                colors.popover
            })
            // Kit Radio owns no hover state, so unchecked cards borrow a
            // lighter cut of the checked border: rest, hover, and checked
            // still read as three distinct rungs. Disabled and checked
            // cards stay put — selection and submission already speak.
            .when(!disabled && !card.checked, |radio| {
                radio.hover(|hover| hover.border_color(colors.primary.opacity(0.35)))
            });
        let choice_id = card.choice.id.clone();
        let step_index = card.step_index;
        radio = radio
            .aria_position_in_set(card.position)
            .aria_size_of_set(card.total)
            .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                if *checked {
                    this.choose_interaction_option(step_index, choice_id.clone(), window, cx);
                }
            }));
        // Decision-critical descriptions stay fully visible (never truncated,
        // never tooltip-only); the digit chip trails in its own lane.
        if card.choice.description.is_some() || card.digit.is_some() {
            let mut row = div()
                .id(format!("choice-desc-{}", card.choice.id))
                .test_support()
                .w_full()
                .flex()
                .items_start()
                .gap_2();
            match card.choice.description.as_ref() {
                Some(description) => {
                    row = row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(themes::muted_text(&colors))
                            .child(description.clone()),
                    );
                }
                None => {
                    row = row.child(div().flex_1());
                }
            }
            if let Some(digit) = card.digit
                && let Ok(keystroke) = gpui_kit::Keystroke::parse_with_os(&digit.to_string(), cx.platform_info().os)
            {
                row = row.child(Kbd::new(keystroke).outline());
            }
            radio = radio.child(row);
        }
        radio.into_any_element()
    }

    fn render_choice(
        &self,
        choice: &InteractionChoice,
        choice_index: usize,
        total_choices: usize,
        selected_index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_choice_card(
            ChoiceCard {
                element_id: format!("choice-{}", choice.id),
                choice,
                position: choice_index + 1,
                total: total_choices,
                checked: selected_index == Some(choice_index),
                digit: digit_shortcut(choice_index),
                step_index: 0,
            },
            cx,
        )
    }
}
