//! Transcript state and conversation rendering: rows, items, tool and diff cards.

use super::*;

impl DesktopView {
    pub(super) fn append_or_update_item(&mut self, id: String, role: &str, title: &str, text: &str, expandable: bool) {
        self.transcript.upsert(TranscriptItem::new(
            id,
            if expandable {
                role.to_string()
            } else {
                TranscriptRole::Assistant.as_str().to_string()
            },
            title.into(),
            text.to_string(),
        ));
    }

    pub(super) fn load_history(&mut self, history: Vec<TranscriptItem>) {
        self.reset_session_state();
        self.transcript.replace_prepared(history);
        self.transcript.follow_latest();
    }

    fn sync_transcript_layout(&mut self, cx: &mut Context<Self>) {
        use super::super::transcript::TranscriptLayoutChange;

        let changes = self.transcript.take_layout_changes();
        if changes.is_empty() {
            return;
        }

        self.message_scroller.update(cx, |state, cx| {
            for change in changes {
                match change {
                    TranscriptLayoutChange::Reset(item_count) => state.reset(item_count, cx),
                    TranscriptLayoutChange::Prepend(count) => {
                        let applied = state.prepend(count, cx);
                        debug_assert!(applied, "transcript and message scroller row counts diverged");
                    }
                    TranscriptLayoutChange::Splice { range, count } => {
                        let applied = state.splice(range, count, cx);
                        debug_assert!(applied, "transcript and message scroller row counts diverged");
                    }
                    TranscriptLayoutChange::Remeasure => state.remeasure(cx),
                    TranscriptLayoutChange::RemeasureItems(range) => {
                        let applied = state.remeasure_items(range, cx);
                        debug_assert!(applied, "transcript remeasurement range is out of bounds");
                    }
                    TranscriptLayoutChange::PauseFollowingTail => {
                        // Kit exposes scrolling but no pause-only operation. Scroll to the
                        // current row only when tail-following is active; a reader who has
                        // already scrolled away keeps the exact existing item offset.
                        if state.item_count() > 0 && state.is_following_tail() {
                            let anchor = state.logical_scroll_top();
                            let _scrolled = state.scroll_to_item(anchor.item_ix, cx);
                        }
                    }
                    TranscriptLayoutChange::FollowLatest => state.scroll_to_end(cx),
                }
            }
        });
    }

    pub(super) fn render_transcript(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_transcript_layout(cx);
        let content = if self.pending_blank_transition() {
            let label = rust_i18n::t!("app.session.preparing_new_thread").to_string();
            div()
                .id("new-thread-pending")
                .test_support()
                .role(gpui_kit::Role::Status)
                .aria_label(label.clone())
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().colors.foreground)
                .child(label)
                .into_any_element()
        } else if self.transcript.is_empty()
            && !self.history_has_earlier
            && !self.history_page_loading
            && self.history_page_error.is_none()
            && !self.zen_mode
        {
            let colors = cx.theme().colors;
            let welcome_copy = welcome_copy(self.welcome_copy_index);
            let greeting = welcome_greeting(self.welcome_period);
            let workspace_name = self
                .workspace
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned());
            let prompt = welcome_prompt(
                &greeting,
                &welcome_copy.action,
                &welcome_copy.workspace_action,
                workspace_name.as_deref(),
            );
            let title = EmptyTitle::new()
                .text_size(gpui_kit::rems(1.75))
                .font_weight(gpui_kit::FontWeight::NORMAL)
                .text_color(colors.foreground)
                .child(
                    div()
                        .id("welcome-prompt")
                        .test_support()
                        .role(gpui_kit::Role::Heading)
                        .aria_label(prompt.clone())
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(if workspace_name.is_some() {
                            format!("{greeting}. {}", welcome_copy.workspace_action)
                        } else {
                            prompt
                        })
                        .when_some(workspace_name, |title, name| {
                            title.child(
                                desktop_button("welcome-workspace")
                                    .accessibility_label(format!("Change workspace, current: {name}"))
                                    .tooltip_with_action(
                                        "Choose workspace",
                                        &ChooseWorkspace,
                                        Some(key_context::DESKTOP),
                                    )
                                    .text()
                                    .text_color(colors.foreground)
                                    .h_auto()
                                    .p_0()
                                    .min_w_0()
                                    .max_w_full()
                                    .desktop_disabled(
                                        !self.workspace_switch_idle()
                                            || self.workspace_picker_open
                                            || self.retained_submission.is_some(),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(gpui_kit::rems(1.75))
                                            .font_weight(gpui_kit::FontWeight::NORMAL)
                                            .border_b_1()
                                            .border_dashed()
                                            .border_color(colors.muted_foreground)
                                            .child(format!("{name}?")),
                                    )
                                    .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone()),
                            )
                        }),
                );
            div()
                .id("welcome")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label("VT Code welcome")
                .size_full()
                .child(
                    EmptyState::new()
                        .header(
                            EmptyHeader::new()
                                .max_w(px(self.content_width()))
                                .gap_6()
                                .media(EmptyMedia::new().mb_0().child(welcome_brand_mark(36., colors.primary)))
                                .title(title),
                        )
                        .size_full()
                        .when(self.workspace.is_none(), |welcome| {
                            welcome.content(
                                EmptyContent::new().child(
                                    desktop_button("open-first-workspace")
                                        .label(welcome_copy.call_to_action.clone())
                                        .accessibility_label(welcome_copy.call_to_action)
                                        .primary()
                                        .desktop_disabled(
                                            self.workspace_picker_open
                                                || !self.workspace_switch_idle()
                                                || self.retained_submission.is_some(),
                                        )
                                        .on_click(cx.listener(|view, _, window, cx| view.choose_workspace(window, cx))),
                                ),
                            )
                        }),
                )
                .into_any_element()
        } else {
            let remaining_history_capacity = self.transcript.remaining_capacity();
            let mut transcript_content = div().size_full().min_h_0().flex().flex_col();
            if self.history_has_earlier || self.history_page_loading || self.history_page_error.is_some() {
                let mut history_bar = div().w_full().flex().flex_col().items_center().gap_1().px_3().py_1();
                if let Some(error) = &self.history_page_error {
                    history_bar =
                        history_bar.child(warning_alert("history-page-error", error.clone(), &cx.theme().colors));
                }
                if self.history_has_earlier && remaining_history_capacity == 0 {
                    history_bar = history_bar.child(
                        div()
                            .id("history-page-limit")
                            .role(gpui_kit::Role::Status)
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(rust_i18n::t!("app.transcript.history_limit_reached").to_string()),
                    );
                } else if self.history_has_earlier {
                    history_bar = history_bar.child(
                        desktop_button("load-earlier-history")
                            .label(if self.history_page_loading {
                                rust_i18n::t!("app.palette.history.loading").to_string()
                            } else {
                                rust_i18n::t!("app.palette.history.load").to_string()
                            })
                            .small()
                            .ghost()
                            .tooltip(rust_i18n::t!("app.transcript.load_earlier_tooltip").to_string())
                            .desktop_disabled(self.history_page_loading || self.running)
                            .on_click(cx.listener(|view, _, _, cx| view.request_earlier_history(cx))),
                    );
                }
                transcript_content = transcript_content.child(history_bar);
            }
            let scroller = MessageScroller::new(
                "transcript-scroller",
                self.message_scroller.clone(),
                cx.processor(|view, index, _, cx| view.render_transcript_row(index, cx)),
            )
            .with_jump_button_label("Jump to latest message")
            .with_jump_button_renderer(|button| button.accessibility_label("Jump to latest message"))
            // Row spacing is applied by render_transcript_row so collapsed tool
            // placeholders do not each reserve the scroller's default 32 px gap.
            .with_row_style(gpui_kit::StyleRefinement::default().pb_0())
            .w_full()
            .flex_1()
            .min_h_0()
            .py_6()
            .into_any_element();
            transcript_content.child(scroller).into_any_element()
        };
        div()
            .id("transcript-scroll")
            .aria_label("Conversation")
            .role(gpui_kit::Role::Region)
            .test_support()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(content)
    }

    pub(super) fn pending_blank_transition(&self) -> bool {
        self.session_request == Some(protocol::method::SESSION_CLOSE)
            && (self.pending_new_thread || self.pending_navigation.is_some())
    }

    /// Compact loading status shown above the transcript while a history
    /// navigation is in flight. Blank transitions already replace the
    /// transcript with their own status, so this covers session loads and
    /// workspace switches. Visible in Zen mode, where the toolbar buttons
    /// and thread header are hidden.
    pub(super) fn render_navigation_loading(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match self.pending_navigation_direction() {
            Some(NavigationDirection::Back) => "Navigating back…",
            Some(NavigationDirection::Forward) => "Navigating forward…",
            None => "Navigating…",
        };
        div()
            .id("navigation-loading")
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(label)
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(Spinner::new().small().color(cx.theme().colors.muted_foreground))
            .child(label)
    }

    fn render_transcript_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.transcript.get(index) else {
            return div().into_any_element();
        };
        if item.activity_grouped && !item.activity_group_head && !item.activity_expanded {
            return div().into_any_element();
        }
        let group_header = item
            .activity_group_head
            .then(|| self.transcript.activity_group(index))
            .flatten()
            .map(|range| self.render_activity_group(range, cx));
        let show_item = !item.activity_grouped || item.activity_expanded;
        let content = div()
            .id(format!("transcript-content-{}", item.id))
            .test_support()
            .w_full()
            .max_w(px(self.content_width()))
            .min_w_0()
            .flex()
            .flex_col()
            .children(group_header)
            .when(item.is_user() && index > 0, |column| {
                column.child(
                    div()
                        .id(format!("turn-divider-{}", item.id))
                        .test_support()
                        .child(turn_divider(&cx.theme().colors)),
                )
            })
            .when(show_item, |column| column.child(self.render_transcript_item(item, cx)));
        let message_id = item.id.clone();
        div()
            .id(format!("transcript-row-{}", item.id))
            .when(item.is_tool() || item.is_diff(), |row| row.role(gpui_kit::Role::ListItem))
            .test_support()
            .w_full()
            .flex()
            .justify_center()
            // MessageScroller owns the shared horizontal row inset.
            .when(item.is_tool(), |row| row.pb_1())
            .when(item.is_user(), |row| row.pb_6())
            .when(!item.is_tool() && !item.is_user(), |row| row.pb_3())
            .when(item.is_assistant(), |row| {
                row.on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
                    let next = hovered.then(|| message_id.clone());
                    if view.hovered_message != next {
                        view.hovered_message = next;
                        cx.notify();
                    }
                }))
            })
            .child(content)
            .into_any_element()
    }

    fn render_transcript_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        if item.is_notice() {
            return self.render_notice_item(item, cx);
        }
        if item.is_tool() {
            return self.render_tool_item(item, cx);
        }
        if item.is_diff() {
            return self.render_diff_item(item, cx);
        }

        let colors = cx.theme().colors;
        let user = item.is_user();
        let label = if user { USER_DISPLAY_NAME } else { item.title.as_str() };
        let copy_text = item.text.clone();
        Message::new()
            .id(format!("message-{}", item.id))
            .role(gpui_kit::Role::ListItem)
            .alignment(if user {
                MessageAlignment::End
            } else {
                MessageAlignment::Start
            })
            .w_full()
            .max_w(px(self.content_width()))
            .gap_1()
            .content(
                MessageContent::new().bubble(
                    Bubble::new()
                        .with_variant(if user {
                            BubbleVariant::Secondary
                        } else {
                            BubbleVariant::Ghost
                        })
                        .content(
                            BubbleContent::new()
                                .rounded(px(18.))
                                .when(user, |surface| {
                                    surface.bg(colors.secondary).text_color(colors.foreground).px_4().py_2()
                                })
                                .child(
                                    div()
                                        .id(format!("message-body-{}", item.id))
                                        .role(gpui_kit::Role::Label)
                                        .aria_label(format!("{label}: {}", item.accessibility_text))
                                        .test_support()
                                        .child(render_markdown(
                                            format!("markdown-{}", item.id),
                                            item.markdown.clone(),
                                            item.is_assistant(),
                                            cx,
                                        )),
                                ),
                        ),
                ),
            )
            .when(!user, |message| {
                // Message actions reveal on hover only; the slot keeps the
                // xsmall button height so rows never reflow. Keyboard and
                // screen-reader users copy through the palette command.
                let revealed = self.hovered_message.as_deref() == Some(item.id.as_str());
                message.footer(
                    MessageFooter::new().child(
                        div()
                            .id(format!("message-actions-{}", item.id))
                            .test_support()
                            .min_h_5()
                            .flex()
                            .items_center()
                            .when(revealed, |slot| {
                                slot.child(
                                    desktop_button(format!("copy-message-{}", item.id))
                                        .icon(IconName::Copy)
                                        .accessibility_label("Copy message")
                                        .tooltip("Copy message")
                                        .ghost()
                                        .xsmall()
                                        .tab_stop(false)
                                        .on_click(move |_, window, app| {
                                            app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                                copy_text.to_string(),
                                            ));
                                            super::super::notifications::copied_to_clipboard(window, app);
                                        }),
                                )
                            }),
                    ),
                )
            })
            .into_any_element()
    }

    /// Severity rows for runtime errors, compaction/reset notices, and run
    /// status: a compact icon/title/body row in severity colors, with no
    /// message bubble, Markdown, or copy action.
    fn render_notice_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let (icon, tone, role) = match item.notice_severity {
            NoticeSeverity::Error => (IconName::CircleAlert, colors.danger, gpui_kit::Role::Alert),
            NoticeSeverity::Warning => (IconName::TriangleAlert, colors.warning, gpui_kit::Role::Status),
            NoticeSeverity::Info => (IconName::Info, themes::muted_text(&colors), gpui_kit::Role::Status),
        };
        div()
            .id(format!("notice-{}", item.id))
            .test_support()
            .role(role)
            .aria_label(format!("{}: {}", item.title, item.accessibility_text))
            .w_full()
            .max_w(px(self.content_width()))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(icon).small().text_color(tone))
                    .child(div().text_sm().font_semibold().text_color(tone).child(item.title.clone())),
            )
            .child(div().text_sm().text_color(colors.foreground).child(item.text.clone()))
            .into_any_element()
    }

    fn render_activity_group(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> AnyElement {
        let Some(head) = self.transcript.get(range.start) else {
            return div().into_any_element();
        };
        let id = head.id.clone();
        let summary = head.activity_group_summary;
        let failed = summary.failed;
        let running = summary.running;
        let colors = cx.theme().colors;
        let status = if failed > 0 {
            ActivityState::Failed
        } else if running > 0 {
            ActivityState::Running
        } else {
            ActivityState::Other
        };
        let status_color = Self::activity_status_color(status, colors);
        let mut label = format!("{} tool actions", summary.total);
        if running > 0 {
            label.push_str(&format!(" · {running} running"));
        }
        if failed > 0 {
            label.push_str(&format!(" · {failed} failed"));
        }
        desktop_button(format!("activity-group-{}", head.id))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_start()
                    .gap_2()
                    .child(Icon::new(IconName::Wrench).small())
                    .when(running > 0, |row| {
                        row.child(
                            div()
                                .id(format!("activity-group-running-indicator-{}", head.id))
                                .test_support()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Spinner::new()
                                        .small()
                                        .color(Self::activity_status_color(ActivityState::Running, colors)),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id(format!("activity-group-label-{}", head.id))
                            .test_support()
                            .min_w_0()
                            .truncate()
                            .child(label.clone()),
                    )
                    .child(
                        Icon::new(if head.activity_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small(),
                    ),
            )
            .accessibility_label(format!("{} {label}", if head.activity_expanded { "Collapse" } else { "Expand" }))
            .toggled(head.activity_expanded)
            .ghost()
            .small()
            .max_w_full()
            .justify_start()
            .text_color(status_color)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.transcript.toggle_activity_group(&id);
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_tool_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let id = item.id.clone();
        let status = if item.has_activity_status {
            item.text.lines().next().unwrap_or_default()
        } else {
            "Archived output"
        };
        let state = ActivityState::from_status(status);
        let status_color = Self::activity_status_color(state, colors);
        let mut panel = div().w_full().max_w(px(self.content_width())).flex().flex_col().gap_1().child(
            desktop_button(format!("tool-{}", item.id))
                .accessibility_label(format!(
                    "{} for {}, status: {status}",
                    if item.expanded { "Hide details" } else { "Show details" },
                    item.title
                ))
                .tooltip(format!("{} · {status}", item.title))
                .ghost()
                .small()
                .w_full()
                .justify_start()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .w_full()
                        .text_color(status_color)
                        .child(Self::render_activity_status_indicator("tool-indicator", &item.id, state, status_color))
                        .child(div().min_w_0().truncate().child(item.title.clone()))
                        // The status slot always renders so rows keep their
                        // shape as states change; completed rows paint it
                        // transparent instead of collapsing the layout.
                        .child(
                            div()
                                .id(format!("tool-status-{}", item.id))
                                .test_support()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .text_color(if state == ActivityState::Completed {
                                    status_color.opacity(0.)
                                } else {
                                    status_color
                                })
                                .child(status.to_string()),
                        )
                        .child(
                            Icon::new(if item.expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .small(),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.transcript.toggle_tool(&id);
                    // The fetch notifies when it queues; this covers the
                    // tier-off and already-loaded paths too.
                    this.request_mcp_app_previews(&id, cx);
                    cx.notify();
                })),
        );
        // Grouped rows nest under their shared header; single tool rows stay flush.
        panel = panel.when(item.activity_grouped, |panel| panel.pl_6());
        if item.expanded {
            let output = item
                .output
                .as_ref()
                .filter(|output| !output.is_empty())
                .cloned()
                .unwrap_or_else(|| item.text.clone());
            let full_output = output.clone();
            let output_excerpt = inspector::output_tail(&output);
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().flex().justify_end().child(
                            desktop_button(format!("copy-tool-output-{}", item.id))
                                .icon(IconName::Copy)
                                .accessibility_label(format!("Copy full output for {}", item.title))
                                .tooltip("Copy full captured output")
                                .ghost()
                                .xsmall()
                                .on_click(move |_, window, app| {
                                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                        full_output.to_string(),
                                    ));
                                    super::super::notifications::copied_to_clipboard(window, app);
                                }),
                        ),
                    )
                    .child(
                        div()
                            .id(format!("tool-output-{}", item.id))
                            .role(gpui_kit::Role::Label)
                            .aria_label(output_excerpt.clone())
                            .test_support()
                            .ml_4()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.secondary)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .child(output_excerpt),
                    )
                    .child(self.render_mcp_app_previews(item, cx)),
            );
        }
        panel.into_any_element()
    }

    fn render_diff_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let diff_foreground =
            themes::diff_text_foreground(colors.background, colors.foreground, colors.success, colors.danger);
        let mono_font = cx.theme().mono_font_family.clone();
        let Some(diff) = item.diff.as_ref() else {
            return div().into_any_element();
        };
        let id = item.id.clone();
        let status_label = if diff.failed {
            "patch failed"
        } else if diff.incomplete {
            "preview incomplete"
        } else {
            "applied"
        };
        let toggle_id = id.clone();
        let mut stats = div().flex().items_center().gap_2().text_sm();
        if let Some(additions) = diff.additions {
            stats = stats.child(div().text_color(colors.success).child(format!("+{additions}")));
        }
        if let Some(deletions) = diff.deletions {
            stats = stats.child(div().text_color(colors.danger).child(format!("−{deletions}")));
        }

        let mut header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().font_medium().child(item.title.clone()))
                    .child(stats)
                    .child(
                        div()
                            .text_sm()
                            .text_color(if diff.failed {
                                colors.danger
                            } else {
                                themes::muted_text(&colors)
                            })
                            .child(status_label),
                    ),
            )
            .child(
                desktop_button(format!("diff-{}", item.id))
                    .label(if item.expanded { "Hide diff" } else { "Show diff" })
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_diff(&toggle_id);
                        cx.notify();
                    })),
            );

        if !diff.patch.is_empty() {
            let copy_id = id.clone();
            header = header.child(
                desktop_button(format!("copy-patch-{}", item.id))
                    .label("Copy patch")
                    .ghost()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        let Some(patch) = view
                            .transcript
                            .items_newest_first()
                            .find(|candidate| candidate.id == copy_id)
                            .and_then(|candidate| candidate.diff.as_ref())
                            .map(|diff| diff.patch.clone())
                            .filter(|patch| !patch.is_empty())
                        else {
                            return;
                        };
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(patch));
                        super::super::notifications::copied_to_clipboard(window, cx);
                    })),
            );
        }

        let mut panel = div()
            .relative()
            .w_full()
            .max_w(px(self.content_width()))
            .rounded_md()
            .overflow_hidden()
            .child(header);

        let mut body = div().w_full().flex().flex_col().px_3().py_2().gap_1();
        for file in &diff.files {
            let (kind_label, kind_color) = match file.kind {
                DiffFileKind::Add => ("added", colors.success),
                DiffFileKind::Delete => ("deleted", colors.danger),
                DiffFileKind::Update => ("modified", colors.muted_foreground),
            };
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().font_family(mono_font.clone()).child(file.path.clone()))
                    .child(div().text_xs().text_color(kind_color).child(kind_label)),
            );
        }
        if diff.incomplete {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("Some file previews were unavailable for this change set."),
            );
        }

        if item.expanded && !diff.patch.is_empty() {
            let mut lines = ClassifiedDiffLines::new(&diff.patch);
            let mut diff_view = div()
                .id(format!("diff-lines-{}", item.id))
                .w_full()
                .flex()
                .flex_col()
                .mt_2()
                .rounded_md()
                .overflow_hidden()
                .overflow_x_scroll()
                .bg(colors.background);
            for line in &mut lines {
                let (line_color, line_bg, marker_color) = match line.kind {
                    DiffLineKind::Added => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.success),
                        Some(("+", "+", colors.success)),
                    ),
                    DiffLineKind::Removed => (
                        diff_foreground,
                        themes::diff_line_background(colors.background, diff_foreground, colors.danger),
                        Some(("-", "−", colors.danger)),
                    ),
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.secondary, None),
                    DiffLineKind::Header | DiffLineKind::Context => (colors.foreground, colors.background, None),
                };
                let mut row = div()
                    .w_full()
                    .px_2()
                    .text_xs()
                    .font_family(mono_font.clone())
                    .whitespace_nowrap()
                    .bg(line_bg);
                if let Some((source_marker, marker, marker_color)) = marker_color {
                    let content = line.text.strip_prefix(source_marker).unwrap_or_default();
                    row = row
                        .flex()
                        .child(
                            div()
                                .w_4()
                                .flex_shrink_0()
                                .font_semibold()
                                .text_color(marker_color)
                                .child(marker),
                        )
                        .child(div().text_color(line_color).child(content.to_string()));
                } else {
                    row = row.text_color(line_color).child(line.text.to_string());
                }
                diff_view = diff_view.child(row);
            }
            if lines.truncated {
                diff_view = diff_view.child(
                    div()
                        .w_full()
                        .px_2()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("Diff truncated after 2,000 lines. Use Copy patch for the full change."),
                );
            }
            body = body.child(diff_view);
        }
        panel = panel.child(body);
        panel.into_any_element()
    }
}
