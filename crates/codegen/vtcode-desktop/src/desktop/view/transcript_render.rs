//! Transcript state and conversation rendering: rows, items, tool and diff cards.

use super::*;
use welcome::{WelcomeHeader, greeting_empty_state, welcome_header};

mod rows;
mod welcome;

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
        self.transcript.group_archived_prefix(usize::MAX);
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
                        // Freeze the viewport exactly where it is: disengage
                        // tail-following without re-anchoring, so an interaction
                        // during streaming neither shifts the view by the row
                        // offset nor strands a short transcript at its top row.
                        // Tail mode is retained, so following resumes once the
                        // reader returns to the bottom (or jump-to-latest).
                        state.pause_following_tail(cx);
                    }
                    TranscriptLayoutChange::FollowLatest => state.scroll_to_end(cx),
                }
            }
        });
    }

    pub(super) fn render_transcript(
        &mut self,
        route_changed: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.sync_transcript_layout(cx);
        // One style build per frame; rows clone it instead of rebuilding it each.
        self.transcript_markdown_style = Some(markdown_style(cx));
        let empty_conversation = self.transcript.is_empty()
            && !self.history_has_earlier
            && !self.history_page_loading
            && self.history_page_error.is_none();
        let connecting = self.backend.is_some() && self.session_request == Some(protocol::method::INITIALIZE);
        let content = if self.pending_blank_transition() {
            let label = rust_i18n::t!(i18n::SESSION_PREPARING_NEW_THREAD).to_string();
            let colors = cx.theme().colors;
            div()
                .id("new-thread-pending")
                .test_support()
                .role(gpui_kit::Role::Status)
                .aria_label(label.clone())
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.foreground)
                .child(shimmer_label("new-thread-pending-label", label, colors.foreground, cx))
                .into_any_element()
        } else if (empty_conversation || connecting) && !self.zen_mode {
            let colors = cx.theme().colors;
            let WelcomeHeader {
                copy: welcome_copy,
                greeting,
                workspace_name,
                prompt,
            } = welcome_header(self);
            let title = div()
                .id(if connecting {
                    "welcome-title-layout"
                } else {
                    "welcome-prompt"
                })
                .test_support()
                .when(!connecting, |title| title.role(gpui_kit::Role::Heading).aria_label(prompt.clone()))
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
                    let name_text = div()
                        .min_w_0()
                        .truncate()
                        .text_size(gpui_kit::rems(1.75))
                        .font_weight(gpui_kit::FontWeight::NORMAL)
                        .border_b_1()
                        .border_dashed()
                        .border_color(colors.muted_foreground)
                        .child(format!("{name}?"));
                    let workspace = desktop_secondary_button(if connecting {
                        "welcome-workspace-layout"
                    } else {
                        "welcome-workspace"
                    })
                    .text()
                    .text_color(colors.foreground)
                    .h_auto()
                    .p_0()
                    .min_w_0()
                    .max_w_full()
                    .child(name_text);
                    title.child(if connecting {
                        workspace.role(None::<gpui_kit::Role>).tab_stop(false).desktop_disabled(true)
                    } else {
                        workspace
                            .accessibility_label(
                                rust_i18n::t!("app.transcript.change_workspace", name = name.as_str()).to_string(),
                            )
                            .tooltip_with_action(
                                rust_i18n::t!("app.transcript.choose_workspace").to_string(),
                                &ChooseWorkspace,
                                Some(key_context::DESKTOP),
                            )
                            .desktop_disabled(
                                !self.workspace_switch_idle()
                                    || self.workspace_picker_open
                                    || self.retained_submission.is_some(),
                            )
                            .dispatches_desktop_action(ChooseWorkspace, self.focus_handle.clone())
                    })
                });
            div()
                .id("welcome")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label(rust_i18n::t!("app.transcript.welcome_label").to_string())
                .size_full()
                .child(greeting_empty_state(self, title, connecting, colors, cx))
                .into_any_element()
        } else if empty_conversation || connecting {
            self.render_zen_empty_state(connecting, cx)
        } else {
            let remaining_history_capacity = self.transcript.remaining_capacity();
            let colors = cx.theme().colors;
            let mut transcript_content = div().size_full().min_h_0().flex().flex_col();
            if self.history_has_earlier || self.history_page_loading || self.history_page_error.is_some() {
                let mut history_bar = div().w_full().flex().flex_col().items_center().gap_1().px_3().py_1();
                if let Some(error) = &self.history_page_error {
                    history_bar = history_bar.child(warning_alert("history-page-error", error.clone(), &colors));
                }
                if self.history_has_earlier && remaining_history_capacity == 0 {
                    history_bar = history_bar.child(
                        div()
                            .id("history-page-limit")
                            .test_support()
                            .role(gpui_kit::Role::Status)
                            .w_full()
                            .child(
                                Marker::new()
                                    .with_variant(MarkerVariant::Separator)
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .content(
                                        MarkerContent::new()
                                            .text(rust_i18n::t!("app.transcript.history_limit_reached").to_string()),
                                    ),
                            ),
                    );
                } else if self.history_has_earlier {
                    history_bar = history_bar.child(
                        desktop_ghost_button("load-earlier-history")
                            .label(if self.history_page_loading {
                                rust_i18n::t!(i18n::PALETTE_HISTORY_LOADING).to_string()
                            } else {
                                rust_i18n::t!(i18n::PALETTE_HISTORY_LOAD).to_string()
                            })
                            .small()
                            .tooltip(rust_i18n::t!("app.transcript.load_earlier_tooltip").to_string())
                            .desktop_disabled(self.history_page_loading || self.running)
                            .on_click(cx.listener(|view, _, _, cx| view.request_earlier_history(cx))),
                    );
                }
                if self.history_page_loading {
                    // The old label inherited the 16px root foreground (no
                    // ancestor sets a size); the marker normalizes it to
                    // text_sm muted like every sibling loading row.
                    history_bar = history_bar.child(loading_marker_row(
                        "history-loading-label",
                        rust_i18n::t!(i18n::PALETTE_HISTORY_LOADING).to_string(),
                        colors.muted_foreground,
                        cx,
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
            if self.turn_activity_visible() {
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
        let destination_pending = self.pending_blank_transition()
            || self.pending_navigation.is_some()
            || self.session_request == Some(protocol::method::SESSION_LOAD);
        let content = if !destination_pending {
            let direction = self.navigation_history.take_committed_direction();
            // Session creation can select a blank session before its first
            // echo. Keep the welcome identity until conversation content exists.
            let session_id = if empty_conversation {
                None
            } else {
                self.selected_session_id.clone()
            };
            let destination = (session_id, empty_conversation);
            // A fast Browser load can commit before the first Workspace frame.
            // Only a return to the same conversation inherits the route's easing.
            let returning = direction.map_or(
                empty_conversation || (route_changed && self.chat_motion.is_current(&destination)),
                |direction| direction == NavigationDirection::Back,
            );
            let _changed = self.chat_motion.update(
                destination,
                returning,
                route_changed || direction.is_some(),
                cx.background_executor().now(),
            );
            self.chat_motion
                .fade(div().size_full().flex().flex_col().child(content), false, window, cx)
                .into_any_element()
        } else {
            content
        };
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
    fn render_zen_empty_state(&self, connecting: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let WelcomeHeader { prompt, .. } = welcome_header(self);
        div()
            .id("zen-welcome")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.transcript.zen_start_label").to_string())
            .size_full()
            .child(greeting_empty_state(
                self,
                div()
                    .id(if connecting {
                        "welcome-title-layout"
                    } else {
                        "zen-welcome-prompt"
                    })
                    .test_support()
                    .when(!connecting, |title| title.role(gpui_kit::Role::Heading).aria_label(prompt.clone()))
                    .child(prompt),
                connecting,
                colors,
                cx,
            ))
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

    /// Whether the turn activity shimmer shows in the thread: any running
    /// turn keeps its live status label up, and idle hides it. The label
    /// tracks the newest started item (see `turn_activity_status`), so it
    /// stays meaningful for the whole turn instead of clearing at the
    /// first content event.
    pub(super) fn turn_activity_visible(&self) -> bool {
        self.running
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
        let muted = cx.theme().colors.muted_foreground;
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
            .text_color(muted)
            .child(Spinner::new().small().color(muted))
            .child(shimmer_label("navigation-loading-label", label, muted, cx))
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
        let colors = cx.theme().colors;
        let group_header = item
            .activity_group_head
            .then(|| self.transcript.activity_group(index))
            .flatten()
            .map(|range| self.render_activity_group(range, cx));
        let show_turn_item = !item.turn_grouped || item.turn_group_expanded;
        let show_item = show_turn_item && (!item.activity_grouped || item.activity_expanded);
        let turn_header = if item.turn_group_head {
            item.turn_id.as_ref().map(|turn_id| {
                let turn_id = turn_id.to_string();
                let expanded = item.turn_group_expanded;
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(themes::layout::gap::TITLE)
                    .child(transcript_icon_slot(div()))
                    .child(
                        desktop_ghost_button(format!("turn-work-{turn_id}"))
                            .accessibility_label(format!(
                                "{} {}",
                                if expanded {
                                    rust_i18n::t!("app.transcript.collapse")
                                } else {
                                    rust_i18n::t!("app.transcript.expand")
                                },
                                InspectorTab::Activity.label()
                            ))
                            .toggled(expanded)
                            .child(
                                div()
                                    .id(format!("turn-work-label-{turn_id}"))
                                    .test_support()
                                    .w_full()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().min_w_0().flex_1().child(InspectorTab::Activity.label()))
                                    .child(transcript_chevron_lane(expanded)),
                            )
                            .small()
                            .flex_1()
                            .min_w_0()
                            .justify_start()
                            .text_color(themes::muted_text(&colors))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.transcript.set_turn_group_expanded(&turn_id, !expanded);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
        } else {
            None
        };
        let content = div()
            .id(format!("transcript-content-{}", item.id))
            .test_support()
            .w_full()
            .max_w(px(self.content_width()))
            .min_w_0()
            .flex()
            .flex_col()
            .children(turn_header)
            .when(show_turn_item, |column| column.children(group_header))
            .when(show_item, |column| column.child(self.render_transcript_item(item, tool_tail.clone(), cx)))
            .when(!item.turn_files.is_empty(), |column| {
                column.child(
                    div()
                        .px(themes::layout::padding::TITLE_SPINE)
                        .when(show_item, |slot| slot.pt_4())
                        .child(transcript_changes_card::render_turn_files_card(self, item, cx)),
                )
            });
        let message_id = item.id.clone();
        let clicked_id = item.id.clone();
        let keyboard_focused = self.keyboard_focused_message.as_deref() == Some(item.id.as_str());
        let flashed = self.transcript_row_flashed(item.id.as_str());
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
            // so no scroll is needed. Child buttons keep their own cursors,
            // so the row's control-free area advertises the pointing hand.
            .cursor_pointer()
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
                        .bg(colors.accent),
                )
            })
            // Transient search/deeplink tint: absolute like the rail, so the
            // flash never reflows the row; an earlier child than the content,
            // so the wash sits below the text and it stays crisp.
            .when(flashed, |row| {
                row.child(
                    div()
                        .id(format!("transcript-flash-{}", item.id))
                        .test_support()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .bg(colors.accent.opacity(0.12)),
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

    /// Whether this row carries the transient search/deeplink tint. Named
    /// predicate shared by the row renderer and its tests.
    pub(super) fn transcript_row_flashed(&self, id: &str) -> bool {
        self.transcript_flash.as_ref().is_some_and(|flash| flash.item_id == id)
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
            .min_w_0()
            .gap_1()
            // Assistant prose insets to the header title spine: leading
            // slot + gap + small-button padding (32px). Symmetric so the
            // column centering contracts keep holding. User pills stay
            // trailing and keep their own shape.
            .when(!user, |message| message.px(themes::layout::padding::TITLE_SPINE))
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
                                .child({
                                    // Cards first: the call reborrows `cx` mutably, which must
                                    // end before the body closure below captures it.
                                    let cards = if item.user_attachments.is_empty() {
                                        None
                                    } else {
                                        Some(self.user_attachment_cards(&item.id, &item.user_attachments, cx))
                                    };
                                    // One builder for the prompt body: attachment echoes stack
                                    // the file cards above it, while text-only rows keep the
                                    // previous single-child shape exactly.
                                    let body = || {
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
                                            ))
                                    };
                                    match cards {
                                        None => body().into_any_element(),
                                        Some(cards) => div()
                                            .flex()
                                            .flex_col()
                                            .min_w_0()
                                            .gap_2()
                                            .child(cards)
                                            .child(body())
                                            .into_any_element(),
                                    }
                                }),
                        ),
                ),
            )
            .when(!user, |message| {
                // Message actions reveal on hover or keyboard highlight; the
                // slot keeps the xsmall button height so rows never reflow.
                // Only the final turn response offers inline copy; intermediate
                // commentary stays copyable through the palette command.
                // The copy action docks at the trailing edge so timestamps
                // keep the leading spine.
                let revealed = self.transcript_row_revealed(item.id.as_str());
                let copyable = revealed && self.transcript.is_final_turn_response(item.id.as_str());
                message.footer(
                    MessageFooter::new().child(
                        div()
                            .id(format!("message-actions-{}", item.id))
                            .test_support()
                            .min_h_5()
                            .flex()
                            .items_center()
                            .children(item.completed_at.as_ref().map(|time| {
                                div()
                                    .text_xs()
                                    .text_color(themes::muted_text(&colors))
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(time.clone())
                                    .when_some(item.completed_elapsed.as_ref(), |slot, elapsed| {
                                        slot.child(format!("{} {elapsed}", display::MIDDOT))
                                    })
                            }))
                            .when(copyable, |slot| {
                                slot.child(
                                    desktop_ghost_button(format!("copy-message-{}", item.id))
                                        .ml_auto()
                                        .icon(IconName::Copy)
                                        .accessibility_label(rust_i18n::t!("app.transcript.copy_message").to_string())
                                        .tooltip(rust_i18n::t!("app.transcript.copy_message").to_string())
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
}
