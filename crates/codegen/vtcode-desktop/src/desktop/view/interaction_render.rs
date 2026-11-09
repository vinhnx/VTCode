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

/// A detail that is exactly one `` `$ ...` `` span is the tool call under
/// review: return the shell source so it renders highlighted. Anything else
/// (prose, mixed spans) stays on its own path.
fn shell_command_span(detail: &str) -> Option<&str> {
    let inner = detail.trim().strip_prefix("`$")?.strip_suffix('`')?;
    let command = inner.trim_start();
    (!command.is_empty()).then_some(command)
}

/// One supporting line under the title.
///
/// Prose keeps the plain themed paragraph (first-line hierarchy intact); a
/// lone `` `$ ...` `` span renders as a highlighted `sh` block and any other
/// backtick content as inline Markdown, so tool calls never show raw ticks.
/// Parsing runs per render over a handful of short strings (unlike the
/// transcript, which prepares hundreds of streaming rows at ingestion);
/// memoize at the call site if profiling ever implicates this.
/// Every wrapper clamps to the panel width: `min_w_0` breaks the flex
/// min-content chain that otherwise shoves trailing content past the card
/// edge, and `overflow_hidden` clips unbreakable paint instead of the layout.
fn render_interaction_detail(detail: &str, detail_index: usize, first: bool, cx: &App) -> AnyElement {
    let colors = cx.theme().colors;
    let wrapper = div()
        .id(format!("interaction-detail-{detail_index}"))
        .test_support()
        .w_full()
        .min_w_0()
        .max_w_full()
        .overflow_hidden();
    if let Some(command) = shell_command_span(detail) {
        let mut fenced = String::with_capacity(command.len().saturating_add(8));
        fenced.push_str("```sh\n");
        fenced.push_str(command);
        fenced.push_str("\n```");
        let prepared = prepare_markdown(&gpui_kit::SharedString::from(fenced));
        return wrapper
            .role(gpui_kit::Role::Label)
            .aria_label(format!("Command: {}", prepared.accessible_text))
            .child(render_markdown_with_style(
                format!("interaction-detail-code-{detail_index}"),
                prepared.markdown,
                false,
                markdown_style(cx),
            ))
            .into_any_element();
    }
    if detail.contains('`') {
        let prepared = prepare_markdown(&gpui_kit::SharedString::from(detail));
        return wrapper
            .role(gpui_kit::Role::Label)
            .aria_label(prepared.accessible_text.to_string())
            .child(render_markdown_with_style(
                format!("interaction-detail-md-{detail_index}"),
                prepared.markdown,
                false,
                markdown_style(cx),
            ))
            .into_any_element();
    }
    wrapper
        .text_sm()
        .text_color(if first {
            colors.foreground
        } else {
            themes::muted_text(&colors)
        })
        .child(detail.to_string())
        .into_any_element()
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
        // Approvals keep the marker icon; plain choices stay neutral so the
        // marker keeps meaning "review carefully".
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
            .rounded_lg()
            .bg(colors.popover)
            .border_1()
            .border_color(colors.border)
            .p_4();
        // Questionnaire order: progress eyebrow, title, description. The
        // panel is one framed card; approvals keep the marker icon so the
        // severity never rests on color alone, but no longer wash the
        // whole surface in warning tint. Choice rows stay borderless
        // inside the frame (no nested cards): selection reads through
        // the checked wash. A one-item counter restates the title
        // without aiding navigation, so the eyebrow renders only for
        // genuine multi-step flows.
        if is_wizard && interaction.steps.len() > 1 {
            let counter =
                rust_i18n::t!("app.interaction.step_counter", n = step_index + 1, total = interaction.steps.len())
                    .to_string();
            panel = panel.child(
                div()
                    .id("question-step-counter")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(counter.clone())
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(counter),
            );
        }
        // No title-row dismiss: Cancel request and Esc own cancellation,
        // matching the consent modals (full-access, running-switch).
        panel = panel.child(
            div()
                .id("interaction-title-row")
                .test_support()
                .flex()
                .items_center()
                .gap_2()
                .when(is_approval, |row| {
                    row.child(
                        div()
                            .id("interaction-approval-marker")
                            .test_support()
                            .flex_shrink_0()
                            .child(Icon::new(IconName::TriangleAlert).text_color(colors.warning)),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_lg()
                        .font_semibold()
                        .text_color(colors.foreground)
                        .child(interaction.title.clone()),
                ),
        );
        // The first detail line carries the decision facts; the rest recede as
        // supporting metadata.
        for (detail_index, detail) in interaction.details.iter().enumerate() {
            panel = panel.child(render_interaction_detail(detail, detail_index, detail_index == 0, cx));
        }
        // The details are the collapsed preview; the toggle below reveals the
        // full proposal in the shared viewer before the choice rows, Edit plan
        // signals the service to open its editor, and the icon button opens
        // the same viewer in the inspector side panel.
        if has_viewable_plan_markdown(interaction.plan_markdown.as_deref()) {
            let expanded = self.interaction_plan_expanded;
            let toggle_label = if expanded {
                rust_i18n::t!("app.interaction.hide_full_plan").to_string()
            } else {
                rust_i18n::t!("app.interaction.show_full_plan").to_string()
            };
            let open_label = rust_i18n::t!("app.interaction.open_in_inspector").to_string();
            let edit_label = rust_i18n::t!("app.interaction.edit_plan").to_string();
            panel = panel.child(
                div()
                    .id("interaction-plan-row")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        desktop_button("interaction-plan-toggle")
                            .label(toggle_label.clone())
                            .accessibility_label(toggle_label)
                            .toggled(expanded)
                            .child(transcript_chevron_lane(expanded))
                            .ghost()
                            .small()
                            .desktop_disabled(submitting)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.interaction_plan_expanded = !view.interaction_plan_expanded;
                                cx.notify();
                            })),
                    )
                    .child(
                        desktop_button("interaction-plan-edit")
                            .label(edit_label.clone())
                            .accessibility_label(edit_label)
                            .icon(IconName::Pencil)
                            .ghost()
                            .small()
                            .desktop_disabled(submitting)
                            .on_click(cx.listener(|view, _, _, cx| view.submit_interaction_hotkey(cx))),
                    )
                    .child(
                        desktop_button("interaction-plan-open-inspector")
                            .icon(IconName::PanelRight)
                            .accessibility_label(open_label.clone())
                            .tooltip(open_label)
                            .ghost()
                            .small()
                            .desktop_disabled(submitting)
                            .on_click(cx.listener(|view, _, _, cx| view.open_inspector_tab(InspectorTab::Plan, cx))),
                    ),
            );
            if expanded && let Some(markdown) = interaction.plan_markdown.as_deref() {
                panel = panel.child(render_markdown_view(
                    "interaction-plan-proposal".to_string(),
                    markdown.into(),
                    MarkdownViewHeight::Panel,
                    cx,
                ));
            }
        }
        if submitting {
            let (status_aria, status_text) = if self.interaction_edit_pending {
                (
                    rust_i18n::t!("app.interaction.editing_plan_aria").to_string(),
                    rust_i18n::t!("app.interaction.editing_plan").to_string(),
                )
            } else {
                (
                    rust_i18n::t!("app.interaction.sending_aria").to_string(),
                    rust_i18n::t!(i18n::COMPOSER_SENDING_DECISION).to_string(),
                )
            };
            panel = panel.child(
                div()
                    .id("interaction-response-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(status_aria)
                    .text_sm()
                    .child(status_text),
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
                panel = panel.child(self.render_question_step(step, step_index, window, cx));
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
                    desktop_default_button("cancel-interaction")
                        .label(rust_i18n::t!("app.interaction.cancel_request").to_string())
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
                        desktop_glass_popover_primary_button("confirm-interaction", &colors, submitting)
                            .label(rust_i18n::t!("app.interaction.confirm").to_string())
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
                                    desktop_default_button("previous-question")
                                        .label(rust_i18n::t!("app.interaction.previous").to_string())
                                        .icon(IconName::ChevronLeft)
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
                                desktop_glass_popover_primary_button("continue-question", &colors, submitting)
                                    .label(if self.interaction_step_index + 1 == interaction.steps.len() {
                                        rust_i18n::t!("app.interaction.submit_answers").to_string()
                                    } else {
                                        rust_i18n::t!("app.interaction.next_question").to_string()
                                    })
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
        // The step counter lives in the panel header eyebrow now; the step
        // itself starts at its question.
        let mut content = div().w_full().flex().flex_col().gap_2();
        content = content.child(div().font_medium().child(step.question.clone()));
        if !step.choices.is_empty() {
            content = content.child(options);
        }
        if step.allow_freeform {
            // The panel owns the popover surface, so the answer box drops to
            // the base background to stay distinct. The wrapper stays
            // borderless: the Textarea already paints its own input border,
            // and disabling that would also drop its focus ring.
            content = content.child(
                div().w_full().rounded_md().bg(colors.background).p_2().child(
                    Textarea::new(&self.question_answer)
                        .accessibility_id("question-answer")
                        .aria_label(rust_i18n::t!("app.interaction.your_answer").to_string())
                        .cursor_text()
                        .disabled(self.pending_interaction_response.is_some())
                        .bg(colors.background)
                        .h(gpui_kit::rems(4.5)),
                ),
            );
        }
        content.into_any_element()
    }

    /// One selectable option row shared by question steps and immediate
    /// choices: plain label, Recommended pill, inline description,
    /// trailing digit chip.
    fn render_choice_card(&self, card: ChoiceCard<'_>, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        // The visual label stays plain: the recommendation becomes a pill
        // below it, while screen readers keep hearing the combined
        // announcement on the row itself.
        let accessible_label = if card.choice.recommended {
            rust_i18n::t!("app.interaction.recommended", label = card.choice.label.as_str()).to_string()
        } else {
            card.choice.label.clone()
        };
        let disabled = self.pending_interaction_response.is_some();
        let mut radio = Radio::new(card.element_id)
            .label(card.choice.label.clone())
            .accessibility_label(accessible_label)
            .checked(card.checked)
            .disabled(disabled)
            .w_full()
            .p_3()
            .rounded_md()
            // Borderless rows inside the framed panel: rest is transparent
            // and checked keeps the primary wash. Kit Radio owns no hover
            // state, so unchecked rows borrow a lighter cut of that wash:
            // rest, hover, and checked still read as three distinct rungs.
            // Disabled and checked rows stay put — selection and
            // submission already speak.
            .when(card.checked, |radio| radio.bg(colors.primary.opacity(0.08)))
            .when(!disabled && !card.checked, |radio| radio.hover(|hover| hover.bg(colors.primary.opacity(0.04))));
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
        if card.choice.recommended {
            // Tag is RenderOnce and cannot carry `.test_support()`, so the
            // wrapper owns the test identity (domain-derived choice id).
            // The pill sits on the checked/hover wash seats, where the
            // outline default (`muted_foreground`) drops below AA — like
            // every other row copy, it uses `muted_text` instead (pinned
            // by recommended_pill_holds_contrast_on_washed_rows).
            radio = radio.child(
                div()
                    .id(format!("choice-recommended-{}", card.choice.id))
                    .test_support()
                    .w_full()
                    .flex()
                    .items_center()
                    .child(
                        Tag::secondary()
                            .outline()
                            .small()
                            .text_color(themes::muted_text(&colors))
                            .child(rust_i18n::t!("app.interaction.recommended_badge").to_string()),
                    ),
            );
        }
        // Decision-critical descriptions stay fully visible (never truncated,
        // never tooltip-only); the digit chip trails in its own lane. The row
        // breaks the flex min-content chain (`min_w_0`) so a long command in
        // the description cannot shove the chip past the card edge; the chip
        // itself never shrinks and unbreakable paint clips inside the text
        // instead of escaping the layout.
        if card.choice.description.is_some() || card.digit.is_some() {
            let mut row = div()
                .id(format!("choice-desc-{}", card.choice.id))
                .test_support()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .gap_2();
            match card.choice.description.as_ref() {
                Some(description) => {
                    row = row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w_full()
                            .overflow_hidden()
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
                row = row.child(
                    div()
                        .id(format!("choice-digit-{}", card.choice.id))
                        .test_support()
                        .flex_shrink_0()
                        .child(Kbd::new(keystroke).outline()),
                );
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

#[cfg(test)]
mod tests {
    use super::shell_command_span;

    #[test]
    fn shell_span_detects_only_lone_dollar_spans() {
        // Both sides of every boundary: spacing, position, emptiness.
        assert_eq!(shell_command_span("`$ python3 -c 'print(1)'`"), Some("python3 -c 'print(1)'"));
        assert_eq!(shell_command_span("`$cmd`"), Some("cmd"));
        assert_eq!(shell_command_span("  `$  ls`  "), Some("ls"));
        assert_eq!(shell_command_span("Run `$ foo` now"), None);
        assert_eq!(shell_command_span("`code`"), None);
        assert_eq!(shell_command_span("$ bare"), None);
        assert_eq!(shell_command_span("`$ `"), None);
        assert_eq!(shell_command_span("``"), None);
        assert_eq!(shell_command_span(""), None);
    }
}
