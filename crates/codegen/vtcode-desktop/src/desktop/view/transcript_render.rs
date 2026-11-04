//! Transcript state and conversation rendering: rows, items, tool and diff cards.

use super::*;

/// Owned welcome-screen text shared by the default and Zen empty states.
struct WelcomeHeader {
    copy: WelcomeCopy,
    greeting: String,
    workspace_name: Option<String>,
    prompt: String,
}

impl DesktopView {
    /// Welcome prelude both empty states render from; single owner so their copy cannot drift.
    fn welcome_header(&self) -> WelcomeHeader {
        let copy = welcome_copy(self.welcome_copy_index);
        let greeting = welcome_greeting(self.welcome_period);
        let workspace_name = self.workspace_file_name();
        let prompt = welcome_prompt(&greeting, &copy.action, &copy.workspace_action, workspace_name.as_deref());
        WelcomeHeader { copy, greeting, workspace_name, prompt }
    }

    /// Shared greeting scaffold for the welcome and Zen empty states: one
    /// owner for the brand mark, title type, and gap, so the two greetings
    /// cannot drift apart in size. Callers pass their own title content
    /// (workspace link vs plain text) and wrap the scaffold in their own
    /// labelled group.
    fn greeting_empty_state(
        &self,
        title: impl IntoElement,
        colors: gpui_kit::component::theme::ThemeColor,
    ) -> impl IntoElement {
        EmptyState::new()
            .header(
                EmptyHeader::new()
                    .max_w(px(self.content_width()))
                    .gap_6()
                    .media(EmptyMedia::new().mb_0().child(welcome_brand_mark(36., colors.primary)))
                    .title(
                        EmptyTitle::new()
                            .text_size(gpui_kit::rems(1.75))
                            .font_weight(gpui_kit::FontWeight::NORMAL)
                            .text_color(colors.foreground)
                            .child(title),
                    ),
            )
            .size_full()
    }

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
        // One style build per frame; rows clone it instead of rebuilding it each.
        self.transcript_markdown_style = Some(markdown_style(cx));
        let empty_conversation = self.transcript.is_empty()
            && !self.history_has_earlier
            && !self.history_page_loading
            && self.history_page_error.is_none();
        let content = if self.pending_blank_transition() {
            let label = rust_i18n::t!(i18n::SESSION_PREPARING_NEW_THREAD).to_string();
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
                .child(shimmer_label("new-thread-pending-label", label))
                .into_any_element()
        } else if empty_conversation && !self.zen_mode {
            let colors = cx.theme().colors;
            let WelcomeHeader {
                copy: welcome_copy,
                greeting,
                workspace_name,
                prompt,
            } = self.welcome_header();
            let title = div()
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
                            .accessibility_label(
                                rust_i18n::t!("app.transcript.change_workspace", name = name.as_str()).to_string(),
                            )
                            .tooltip_with_action(
                                rust_i18n::t!("app.transcript.choose_workspace").to_string(),
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
                });
            div()
                .id("welcome")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label(rust_i18n::t!("app.transcript.welcome_label").to_string())
                .size_full()
                .child(self.greeting_empty_state(title, colors))
                .into_any_element()
        } else if empty_conversation {
            self.render_zen_empty_state(cx)
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
                                rust_i18n::t!(i18n::PALETTE_HISTORY_LOADING).to_string()
                            } else {
                                rust_i18n::t!(i18n::PALETTE_HISTORY_LOAD).to_string()
                            })
                            .small()
                            .ghost()
                            .tooltip(rust_i18n::t!("app.transcript.load_earlier_tooltip").to_string())
                            .desktop_disabled(self.history_page_loading || self.running)
                            .on_click(cx.listener(|view, _, _, cx| view.request_earlier_history(cx))),
                    );
                }
                if self.history_page_loading {
                    history_bar = history_bar.child(shimmer_label(
                        "history-loading-label",
                        rust_i18n::t!(i18n::PALETTE_HISTORY_LOADING).to_string(),
                    ));
                }
                transcript_content = transcript_content.child(history_bar);
            }
            // Relative column mirroring the original flex structure exactly: the
            // scroller keeps its flex_1 relationship, and the absolutely
            // positioned overlay sibling takes no layout space.
            let scroller = div()
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    MessageScroller::new(
                        "transcript-scroller",
                        self.message_scroller.clone(),
                        cx.processor(|view, index, _, cx| view.render_transcript_row(index, cx)),
                    )
                    .with_jump_button_label(rust_i18n::t!("app.transcript.jump_latest").to_string())
                    .with_jump_button_renderer(|button| {
                        button.accessibility_label(rust_i18n::t!("app.transcript.jump_latest").to_string())
                    })
                    // Row spacing is applied by render_transcript_row so collapsed tool
                    // placeholders do not each reserve the scroller's default 32 px gap.
                    .with_row_style(gpui_kit::StyleRefinement::default().pb_0())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .py_6(),
                )
                .children(self.render_transcript_overlays(cx))
                .into_any_element();
            transcript_content = transcript_content.child(scroller);
            if self.turn_awaiting_content() {
                transcript_content =
                    transcript_content.child(turn_pending_row(self.status.clone(), self.content_width(), cx));
            }
            transcript_content.into_any_element()
        };
        // The first transcript layout settles list readers; overlay
        // visibility waits for it. Set after content is built so this very
        // render still suppresses the overlay. One extra frame, guarded by
        // the flag so it can never loop.
        if !self.transcript_layout_settled && !self.pending_blank_transition() && !empty_conversation {
            self.transcript_layout_settled = true;
            cx.notify();
        }
        div()
            .id("transcript-scroll")
            .aria_label(rust_i18n::t!("app.transcript.conversation_label").to_string())
            .role(gpui_kit::Role::Region)
            .test_support()
            .track_focus(&self.transcript_focus)
            .when(self.transcript.is_empty(), |el| el.tab_stop(false))
            .key_context(key_context::TRANSCRIPT)
            .on_action(cx.listener(|view, _: &TranscriptPrevious, _, cx| view.move_transcript_focus(false, cx)))
            .on_action(cx.listener(|view, _: &TranscriptNext, _, cx| view.move_transcript_focus(true, cx)))
            .on_action(cx.listener(|view, _: &TranscriptFirst, _, cx| view.move_transcript_focus_edge(true, cx)))
            .on_action(cx.listener(|view, _: &TranscriptLast, _, cx| view.move_transcript_focus_edge(false, cx)))
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .child(content)
    }

    /// Minimal centered empty state for Zen mode. The brand mark is the single
    /// focal point, followed by one greeting line; the workspace name renders
    /// as plain text because Zen hides the title-bar identity. The workspace
    /// picker stays reachable through the composer context row when none is set.
    /// Mark, title, and gap come from the shared greeting scaffold, so
    /// toggling modes never resizes the greeting.
    fn render_zen_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let WelcomeHeader { prompt, .. } = self.welcome_header();
        div()
            .id("zen-welcome")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.transcript.zen_start_label").to_string())
            .size_full()
            .child(
                self.greeting_empty_state(
                    div()
                        .id("zen-welcome-prompt")
                        .test_support()
                        .role(gpui_kit::Role::Heading)
                        .aria_label(prompt.clone())
                        .child(prompt),
                    colors,
                ),
            )
            .into_any_element()
    }

    pub(super) fn pending_blank_transition(&self) -> bool {
        self.session_request == Some(protocol::method::SESSION_CLOSE)
            && (self.pending_new_thread || self.pending_navigation.is_some())
    }

    /// Whether the active turn is still waiting for its first content.
    pub(super) fn turn_awaiting_content(&self) -> bool {
        self.running && !self.turn_content_seen
    }

    /// Compact loading status shown above the transcript while a history
    /// navigation is in flight. Blank transitions already replace the
    /// transcript with their own status, so this covers session loads and
    /// workspace switches. Visible in Zen mode, where the toolbar buttons
    /// and thread header are hidden.
    pub(super) fn render_navigation_loading(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = match self.pending_navigation_direction() {
            Some(NavigationDirection::Back) => rust_i18n::t!("app.transcript.navigating_back").to_string(),
            Some(NavigationDirection::Forward) => rust_i18n::t!("app.transcript.navigating_forward").to_string(),
            None => rust_i18n::t!("app.transcript.navigating").to_string(),
        };
        div()
            .id("navigation-loading")
            .test_support()
            .role(gpui_kit::Role::Status)
            .aria_label(label.clone())
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(Spinner::new().small().color(cx.theme().colors.muted_foreground))
            .child(shimmer_label("navigation-loading-label", label))
    }

    fn render_transcript_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        // Resolve the tail before borrowing the row for render: the cache
        // needs `&mut self` while the row borrows the transcript.
        let tool_tail = self.expanded_tool_tail(index);
        let Some(item) = self.transcript.get(index) else {
            return div().into_any_element();
        };
        if !Self::transcript_row_visible(item) {
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
            .when(show_item, |column| column.child(self.render_transcript_item(item, tool_tail.clone(), cx)));
        let message_id = item.id.clone();
        let clicked_id = item.id.clone();
        let keyboard_focused = self.keyboard_focused_message.as_deref() == Some(item.id.as_str());
        // Active-descendant announcements need a role, so user/assistant rows
        // (deliberately role-less) stay silent.
        let announceable = item.is_tool() || item.is_diff();
        div()
            .id(format!("transcript-row-{}", item.id))
            .when(announceable, |row| row.role(gpui_kit::Role::ListItem))
            .when(keyboard_focused && announceable, |row| row.aria_active_descendant())
            .test_support()
            .w_full()
            .flex()
            .justify_center()
            // Clicking a row anchors the keyboard highlight as position
            // memory (and reveals its actions); the row is already visible,
            // so no scroll is needed. Child controls keep their own clicks.
            .on_click(cx.listener(move |view, _, _, cx| {
                view.keyboard_focused_message = Some(clicked_id.clone());
                cx.notify();
            }))
            // Absolutely positioned so the highlight never reflows the row.
            .when(keyboard_focused, |row| {
                row.child(
                    div()
                        .id(format!("transcript-focus-rail-{}", item.id))
                        .test_support()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(cx.theme().colors.accent),
                )
            })
            // MessageScroller owns the shared horizontal row inset. The bottom
            // tiers encode the relationship: conversation turns (user +
            // assistant) breathe at the section tier while metadata clusters
            // (tools tight, diffs/notices grouped) stay tucked beneath them.
            .when(item.is_tool(), |row| row.pb_1())
            .when(item.is_user() || item.is_assistant(), |row| row.pb_6())
            .when(!item.is_tool() && !item.is_user() && !item.is_assistant(), |row| row.pb_3())
            // Hover tracking covers every row with revealable actions:
            // assistant copy plus the group, diff, and tool-row jumps.
            .when(item.is_assistant() || item.is_tool() || item.is_diff(), |row| {
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

    /// Whether a transcript row's secondary actions are currently revealed:
    /// mouse hover or keyboard highlight (row clicks, Up/Down navigation, and
    /// palette jumps all anchor it). Toggles, labels, and status always
    /// render; only inspector-navigation jumps recede, and every destination
    /// stays reachable through the inspector toggle plus tab keys.
    pub(super) fn transcript_row_revealed(&self, id: &str) -> bool {
        self.hovered_message.as_deref() == Some(id) || self.keyboard_focused_message.as_deref() == Some(id)
    }

    fn render_transcript_item(
        &self,
        item: &TranscriptItem,
        tool_tail: Option<gpui_kit::SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if item.is_notice() {
            return self.render_notice_item(item, cx);
        }
        if item.is_tool() {
            return self.render_tool_item(item, tool_tail, cx);
        }
        if item.is_diff() {
            return self.render_diff_item(item, cx);
        }

        let colors = cx.theme().colors;
        let user = item.is_user();
        let label = if user {
            rust_i18n::t!(i18n::TRANSCRIPT_YOU).to_string()
        } else {
            item.title.to_string()
        };
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
                        // Lists report marker-only intrinsic width (Kit lays item
                        // text in a basis-zero `flex_1` column), so a shrink-to-fit
                        // bubble collapses to a ~23px pill. Give list content the
                        // definite width it needs; the 80% max keeps it trailing
                        // and short non-list messages keep their pills.
                        .when(user && item.has_list, |bubble| bubble.w_full())
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
                                        // The kit bubble pins text_sm (14px); the
                                        // message body is the primary content and
                                        // reads at the user's full base size, which
                                        // also rasterizes more smoothly at weight
                                        // 400 on dark surfaces. Markdown inherits
                                        // this ambient size; code keeps its own.
                                        .text_base()
                                        .child(render_markdown_with_style(
                                            format!("markdown-{}", item.id),
                                            item.markdown.clone(),
                                            item.is_assistant(),
                                            self.transcript_markdown_style
                                                .clone()
                                                .unwrap_or_else(|| markdown_style(cx)),
                                        )),
                                ),
                        ),
                ),
            )
            .when(!user, |message| {
                // Message actions reveal on hover or keyboard highlight; the
                // slot keeps the xsmall button height so rows never reflow.
                // Keyboard and screen-reader users can also copy through the
                // palette command.
                let revealed = self.transcript_row_revealed(item.id.as_str());
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
                                        .accessibility_label(rust_i18n::t!("app.transcript.copy_message").to_string())
                                        .tooltip(rust_i18n::t!("app.transcript.copy_message").to_string())
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

    fn render_tool_item(
        &self,
        item: &TranscriptItem,
        tool_tail: Option<gpui_kit::SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let id = item.id.clone();
        let status = if item.has_activity_status {
            item.text.lines().next().unwrap_or_default().to_string()
        } else {
            rust_i18n::t!("app.transcript.archived_output").to_string()
        };
        let state = ActivityState::from_status(&status);
        let status_color = Self::activity_status_color(state, colors);
        let mut panel = div().w_full().max_w(px(self.content_width())).flex().flex_col().gap_1().child(
            desktop_button(format!("tool-{}", item.id))
                .accessibility_label(
                    rust_i18n::t!(
                        "app.transcript.details_label",
                        toggle = if item.expanded {
                            rust_i18n::t!("app.transcript.hide_details").to_string()
                        } else {
                            rust_i18n::t!("app.transcript.show_details").to_string()
                        },
                        title = item.title.as_str(),
                        status = status.as_str()
                    )
                    .to_string(),
                )
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
                        .child(Self::render_activity_status_indicator("tool-indicator", &item.id, state, status_color))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_color(Self::tool_row_title_color(state, colors))
                                .child(item.title.clone()),
                        )
                        // The state icon already signals completion, so
                        // completed rows drop the redundant label; other
                        // states keep theirs and the chevron stays docked.
                        .when(state != ActivityState::Completed, |row| {
                            row.child(
                                div()
                                    .id(format!("tool-status-{}", item.id))
                                    .test_support()
                                    .flex_shrink_0()
                                    // Bound so backend strings ellipsize instead of
                                    // pushing the title and chevron out of the row.
                                    .max_w(px(320.))
                                    .truncate()
                                    .text_sm()
                                    .text_color(themes::muted_text(&colors))
                                    .child(status.to_string()),
                            )
                        })
                        .child(
                            Icon::new(if item.expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .small()
                            .flex_shrink_0()
                            .text_color(themes::muted_text(&colors)),
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
            // Pre-resolved by the row through the revision-keyed tail cache;
            // the direct computation only runs if a future caller passes `None`.
            let output_excerpt = tool_tail.unwrap_or_else(|| inspector::output_tail(&output));
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_1()
                            .child(
                                desktop_button(format!("copy-tool-output-{}", item.id))
                                    .icon(IconName::Copy)
                                    .accessibility_label(
                                        rust_i18n::t!("app.transcript.copy_output_for", title = item.title.to_string())
                                            .to_string(),
                                    )
                                    .tooltip(rust_i18n::t!("app.transcript.copy_full_output").to_string())
                                    .ghost()
                                    .xsmall()
                                    .on_click(move |_, window, app| {
                                        app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                            full_output.to_string(),
                                        ));
                                        super::super::notifications::copied_to_clipboard(window, app);
                                    }),
                            )
                            .when(item.has_output(), |row| {
                                row.child(Self::tool_output_cta(
                                    item,
                                    self.transcript_row_revealed(item.id.as_str()),
                                    cx,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .id(format!("tool-output-{}", item.id))
                            .role(gpui_kit::Role::Label)
                            .aria_label(output_excerpt.clone())
                            .test_support()
                            // Align under the title: status icon plus the row gap.
                            .ml_6()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.secondary)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .child(render_output_sections(
                                format!("tool-output-{}", item.id),
                                &output_excerpt,
                                &colors,
                                self.transcript_markdown_style.clone().unwrap_or_else(|| markdown_style(cx)),
                            )),
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
            rust_i18n::t!("app.transcript.patch_failed").to_string()
        } else if diff.incomplete {
            rust_i18n::t!("app.transcript.preview_incomplete").to_string()
        } else {
            rust_i18n::t!("app.transcript.applied").to_string()
        };
        let toggle_id = id.clone();
        let mut stats = div().flex().items_center().gap_2().text_sm().flex_shrink_0();
        if let Some(additions) = diff.additions {
            stats = stats.child(div().text_color(colors.success).child(format!("+{additions}")));
        }
        if let Some(deletions) = diff.deletions {
            stats = stats.child(div().text_color(colors.danger).child(format!("−{deletions}")));
        }

        // Trailing actions share one shrink-proof group (see panel_header):
        // the toggle and Changes jump must never overflow the card, so the
        // leading title truncates instead when the conversation narrows. Copy
        // patch lives in the expanded body beside the patch it copies.
        let actions = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                desktop_button(format!("diff-{}", item.id))
                    .label(if item.expanded {
                        rust_i18n::t!("app.transcript.hide_diff").to_string()
                    } else {
                        rust_i18n::t!("app.transcript.show_diff").to_string()
                    })
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_diff(&toggle_id);
                        cx.notify();
                    })),
            )
            .child(Self::diff_changes_cta(&item.id, self.transcript_row_revealed(item.id.as_str()), cx));

        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_2()
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .flex_1()
                    .child(div().font_medium().min_w_0().truncate().child(item.title.clone()))
                    .child(stats)
                    .child(
                        div()
                            .text_sm()
                            .flex_shrink_0()
                            .text_color(if diff.failed {
                                colors.danger
                            } else {
                                themes::muted_text(&colors)
                            })
                            .child(status_label),
                    ),
            )
            .child(actions);

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
                    .child(rust_i18n::t!("app.transcript.previews_unavailable").to_string()),
            );
        }

        if item.expanded && !diff.patch.is_empty() {
            // Export sits with its object, mirroring the tool-row actions
            // above their output block; the header keeps only the toggle and
            // the Changes jump.
            let copy_id = id.clone();
            body = body.child(
                div().flex().justify_end().child(
                    desktop_button(format!("copy-patch-{}", item.id))
                        .label(rust_i18n::t!("app.transcript.copy_patch").to_string())
                        .ghost()
                        .xsmall()
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
                ),
            );
            let mut lines = ClassifiedDiffLines::new(&diff.patch);
            let mut diff_view = div()
                .id(format!("diff-lines-{}", item.id))
                .test_support()
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
                    // Hunk markers orient multi-hunk patches but earn no band:
                    // muted text on the base surface, like context lines.
                    DiffLineKind::Hunk => (colors.muted_foreground, colors.background, None),
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
                        .child(rust_i18n::t!("app.transcript.diff_truncated").to_string()),
                );
            }
            body = body.child(diff_view);
        }
        panel = panel.child(body);
        panel.into_any_element()
    }
}
