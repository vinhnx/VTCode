use super::*;

#[gpui_kit::test]
fn desktop_view_uses_the_supplied_startup_preferences(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut preferences = crate::desktop::preferences::Preferences::default();
    preferences.set_ui_language(crate::desktop::localization::UiLanguage::Vietnamese);
    let (view, cx) = cx.add_window_view(move |window, cx| {
        DesktopView::new_with_preferences(window, cx, ThemeMode::Light, preferences)
    });

    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert_eq!(view.preferences.ui_language(), crate::desktop::localization::UiLanguage::Vietnamese);
    });
}

#[gpui_kit::test]
fn disconnected_config_catalog_cannot_be_edited(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, _| {
        view.workspace = Some("/workspace".into());
        view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
            settings: Vec::new(),
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        });
        assert!(!view.can_edit_cli_settings());
        view.backend = Some(BackendHandle { commands });
        assert!(view.can_edit_cli_settings());
        view.backend = None;
        assert!(!view.can_edit_cli_settings());
    });
}

#[gpui_kit::test]
fn interrupted_config_write_unlocks_editor_and_reports_uncertain_outcome(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    for event in [
        BackendEvent::Exited { status: Some(1), authentication_required: false },
        BackendEvent::Exited { status: Some(1), authentication_required: true },
        BackendEvent::LaunchFailed("service failed".into()),
    ] {
        view.update(cx, |view, cx| {
            view.config_write_pending = true;
            view.config_editor_error = None;
            view.handle_backend_event(event, cx);
            assert!(!view.config_write_pending);
            assert!(
                view.config_editor_error
                    .as_deref()
                    .is_some_and(|error| error.contains("outcome is unknown"))
            );
            assert!(view.error.as_deref().is_some_and(|error| error.contains("outcome is unknown")));
        });
    }
}

#[gpui_kit::test]
fn language_selection_updates_settings_and_persists(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-420.))), cx);
    })
    .unwrap();

    for (locale, title, new_session, language) in [
        ("es", "Configuración", None, crate::desktop::localization::UiLanguage::Spanish),
        (
            "pt-BR",
            "Configurações",
            Some("Nova conversa"),
            crate::desktop::localization::UiLanguage::BrazilianPortuguese,
        ),
        ("id", "Pengaturan", Some("Percakapan baru"), crate::desktop::localization::UiLanguage::Indonesian),
        ("ru", "Настройки", Some("Новый диалог"), crate::desktop::localization::UiLanguage::Russian),
        ("zh-TW", "設定", Some("新對話"), crate::desktop::localization::UiLanguage::TraditionalChineseTaiwan),
        (
            "zh-HK",
            "設定",
            Some("新對話"),
            crate::desktop::localization::UiLanguage::TraditionalChineseHongKong,
        ),
        ("tr", "Ayarlar", Some("Yeni sohbet"), crate::desktop::localization::UiLanguage::Turkish),
        ("pl", "Ustawienia", Some("Nowy wątek"), crate::desktop::localization::UiLanguage::Polish),
        ("hi", "सेटिंग्स", Some("नया थ्रेड"), crate::desktop::localization::UiLanguage::Hindi),
        ("uk", "Налаштування", Some("Нова розмова"), crate::desktop::localization::UiLanguage::Ukrainian),
        ("sv", "Inställningar", Some("Ny tråd"), crate::desktop::localization::UiLanguage::Swedish),
    ] {
        cx.update_window(window, |_, window, cx| {
            window.within("ui-language-select").click("input", cx);
            window.input(locale, cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.press("enter", cx)).unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(view.read(cx).preferences.ui_language(), language);
            assert_eq!(window.find("settings-panel").label(), Some(title));
            assert_eq!(window.find("ui-language-select").value(), Some(language.label()));
        })
        .unwrap();
        if let Some(new_session) = new_session {
            // Routes unmount the workspace, so go back to observe its relocalized chrome,
            // then reopen settings for the next locale (scroll offset is retained).
            cx.update_window(window, |_, window, cx| {
                window.press("escape", cx);
                window.render_frame(cx);
                assert_eq!(window.find("new-session").label(), Some(new_session));
                view.update(cx, |view, cx| view.open_settings(window, cx));
                window.render_frame(cx);
            })
            .unwrap();
        }
        assert_eq!(crate::desktop::preferences::Preferences::load().ui_language(), language);
    }

    cx.update_window(window, |_, _, _| {
        crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::English);
    })
    .unwrap();
}

#[gpui_kit::test]
fn webmcp_status_discards_snapshots_superseded_by_session_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.webmcp_status = Some(connected_webmcp_status());
            view.selected_session_id = Some("session-old".into());
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                result: Some(json!({ "closed": true })), error: None,
            }, cx);
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                result: Some(json!({ "sessionId": "new", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None,
            }, cx);
            assert!(view.webmcp_status_dirty);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                result: Some(serde_json::to_value(connected_webmcp_status()).unwrap()), error: None,
            }, cx);
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            let mut fresh = connected_webmcp_status();
            fresh.session_id = Some("new".into());
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                result: Some(serde_json::to_value(fresh.clone()).unwrap()), error: None,
            }, cx);
            assert_eq!(view.webmcp_status, Some(fresh));
            assert!(!view.webmcp_status_dirty);
            view.handle_backend_event(BackendEvent::TurnStatus(json!({ "sessionId": "new", "turnId": "browser-turn", "status": "running" })), cx);
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                result: Some(json!({})), error: None,
            }, cx);
            assert!(view.webmcp_status_dirty, "an invalid refresh cannot validate the old snapshot");
            assert!(view.webmcp_notice.as_ref().unwrap().text().contains("invalid WebMCP status"));
            view.request_webmcp_status();
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                result: Some(serde_json::to_value(connected_webmcp_status()).unwrap()), error: None,
            }, cx);
            assert!(!view.webmcp_status_dirty);
            assert!(view.webmcp_notice.is_none());
        });
    }).unwrap();
    for _ in 0..4 {
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("status request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    }
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn webmcp_notice_tracks_the_selected_language_and_typed_refresh_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, _cx| {
            view.preferences
                .set_ui_language(crate::desktop::localization::UiLanguage::Turkish);
            crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::Turkish);
            view.webmcp_notice = Some(super::super::webmcp::WebmcpNotice::Disconnected);
        });
        let turkish = rust_i18n::t!("app.webmcp_notice.disconnected", locale = "tr").to_string();
        assert_eq!(view.read(cx).webmcp_notice.as_ref().unwrap().text(), turkish);

        view.update(cx, |view, _cx| {
            view.preferences
                .set_ui_language(crate::desktop::localization::UiLanguage::Polish);
            crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::Polish);
        });
        let polish = rust_i18n::t!("app.webmcp_notice.disconnected", locale = "pl").to_string();
        assert_eq!(view.read(cx).webmcp_notice.as_ref().unwrap().text(), polish);

        view.update(cx, |view, cx| {
            assert_eq!(view.webmcp_notice, Some(super::super::webmcp::WebmcpNotice::Disconnected));
            assert!(view.handle_webmcp_response(
                vtcode_desktop_protocol::method::WEBMCP_STATUS,
                serde_json::to_value(connected_webmcp_status()).unwrap(),
                cx
            ));
            assert!(view.webmcp_notice.is_none(), "a successful refresh clears typed connection failures");

            view.webmcp_notice = Some(super::super::webmcp::WebmcpNotice::PairingCreated);
            assert!(view.handle_webmcp_response(
                vtcode_desktop_protocol::method::WEBMCP_STATUS,
                serde_json::to_value(connected_webmcp_status()).unwrap(),
                cx
            ));
            assert_eq!(
                view.webmcp_notice,
                Some(super::super::webmcp::WebmcpNotice::PairingCreated),
                "a status refresh preserves informational pairing feedback"
            );
        });
        crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::English);
    })
    .unwrap();
}

#[gpui_kit::test]
fn context_boundary_events_notice_and_reset_stale_usage(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{
        CompactionMode, CompactionTrigger, ContextResetEvent, ContextResetTrigger, ThreadCompactBoundaryEvent,
        ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent,
    };

    let boundary = |session_id: &str| {
        BackendEvent::ThreadEvent(ThreadEventNotification {
            session_id: session_id.into(),
            turn_id: None,
            event: VersionedThreadEvent::new(ThreadEvent::ThreadCompactBoundary(Box::new(
                ThreadCompactBoundaryEvent {
                    thread_id: session_id.into(),
                    trigger: CompactionTrigger::Auto,
                    mode: CompactionMode::Local,
                    original_message_count: 120,
                    compacted_message_count: 8,
                    history_artifact_path: None,
                    previous_segment_id: None,
                    new_segment_id: None,
                    previous_prefix_hash: None,
                    new_prefix_hash: None,
                    previous_catalog_hash: None,
                    new_catalog_hash: None,
                },
            ))),
        })
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.last_usage = Some(Usage {
                input_tokens: 90_000,
                output_tokens: 10_000,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
            });

            view.handle_backend_event(boundary("other-session"), cx);
            assert!(view.transcript.is_empty(), "stale sessions stay silent");
            assert!(view.last_usage.is_some(), "stale sessions keep their usage");

            view.handle_backend_event(boundary("session"), cx);
            assert!(view.last_usage.is_none(), "compaction clears stale usage");
            assert_eq!(view.transcript.len(), 1);
            let notice = view.transcript.get(0).unwrap();
            assert_eq!(notice.role, "notice");
            assert!(!notice.title.is_empty());
            for expected in ["120", "8", "auto", "local"] {
                assert!(notice.text.contains(expected), "notice dropped {expected}");
            }

            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: None,
                event: VersionedThreadEvent::new(ThreadEvent::ContextReset(ContextResetEvent {
                    thread_id: "session".into(),
                    turn_id: "turn-1".into(),
                    trigger: ContextResetTrigger::PlanApproval,
                    plan_preserved: true,
                    previous_context_usage_percent: 87,
                    tool_budget_reset: true,
                })),
            });
            assert_eq!(view.transcript.len(), 2);
            let notice = view.transcript.get(1).unwrap();
            assert_eq!(notice.role, "notice");
            for expected in ["plan approval", "87"] {
                assert!(notice.text.contains(expected), "notice dropped {expected}");
            }

            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("turn-1".into()),
                event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    final_response_item_id: None,
                    completed_at: None,
                    usage: Usage {
                        input_tokens: 5_000,
                        output_tokens: 500,
                        cached_input_tokens: 0,
                        cache_creation_tokens: 0,
                    },
                    in_progress_exec_sessions: Vec::new(),
                })),
            });
            assert_eq!(view.last_usage.as_ref().unwrap().input_tokens, 5_000);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn sandbox_denied_event_pushes_warning_notice(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{AppSandboxDeniedEvent, ThreadEvent, VersionedThreadEvent};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _cx| {
            view.selected_session_id = Some("session".into());
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: None,
                event: VersionedThreadEvent::new(ThreadEvent::AppSandboxDenied(Box::new(AppSandboxDeniedEvent {
                    tool: "mcp::demo::get_time".into(),
                    tier: "off".into(),
                    uris: vec!["ui://demo/app.html".into(), "ui://demo/other.html".into()],
                }))),
            });
            assert_eq!(view.transcript.len(), 1);
            let notice = view.transcript.get(0).unwrap();
            assert_eq!(notice.role, "notice");
            assert_eq!(notice.title.as_str(), "App UI refused");
            for expected in [
                "mcp::demo::get_time",
                "'off'",
                "ui://demo/app.html",
                "ui://demo/other.html",
            ] {
                assert!(notice.text.contains(expected), "notice dropped {expected}");
            }
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_notices_render_severity_without_message_chrome(cx: &mut TestAppContext) {
    use crate::desktop::transcript::NoticeSeverity;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, severity, title, text) in [
                ("notice-error", NoticeSeverity::Error, "Runtime error", "Boom `code` here"),
                ("notice-warning", NoticeSeverity::Warning, "Run status", "Turn ended early"),
                ("notice-info", NoticeSeverity::Info, "Compaction", "120 into 8 messages"),
            ] {
                let mut item = TranscriptItem::new(id.into(), "notice".into(), title.into(), text.into());
                item.notice_severity = severity;
                view.transcript.push(item);
            }
            cx.notify();
        });
        window.render_frame(cx);
        // Severity roles: errors announce as alerts, the rest as status.
        assert_eq!(window.find("notice-notice-error").role(), Some(gpui_kit::Role::Alert));
        assert_eq!(window.find("notice-notice-warning").role(), Some(gpui_kit::Role::Status));
        assert_eq!(window.find("notice-notice-info").role(), Some(gpui_kit::Role::Status));
        // No message chrome: notices carry no copy action.
        for id in ["notice-error", "notice-warning", "notice-info"] {
            assert!(window.try_find(format!("copy-message-{id}")).is_none(), "{id} must not offer message copy");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn message_actions_reveal_on_hover_without_moving_rows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "hover-me".into(),
                "assistant".into(),
                "VT Code".into(),
                "Hover body".into(),
            ));
        });
        window.render_frame(cx);
        assert!(window.try_find("copy-message-hover-me").is_none(), "actions hide at rest");
        let slot = window.find("message-actions-hover-me").bounds();
        assert!(slot.size.height > px(0.), "the slot reserves layout while empty");
        let row = window.find("transcript-row-hover-me").bounds();
        window.hover("transcript-row-hover-me", cx);
        window.render_frame(cx);
        assert!(window.try_find("copy-message-hover-me").is_some(), "hover reveals actions");
        assert_eq!(window.find("transcript-row-hover-me").bounds(), row, "reveal must not reflow the row");
        assert_eq!(view.read(cx).hovered_message.as_deref(), Some("hover-me"));
        window.hover("transcript-scroll", cx);
        window.render_frame(cx);
        assert!(window.try_find("copy-message-hover-me").is_none(), "leaving hides actions again");
        assert!(view.read(cx).hovered_message.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_copy_latest_message_copies_newest_raw_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.latest_copyable_message().is_none(), "empty transcript has nothing to copy");
            assert!(!view.palette_action_available(&super::super::PaletteAction::CopyLatestMessage));
            view.run_palette_action(super::super::PaletteAction::CopyLatestMessage, window, cx);
        });
        window.render_frame(cx);
        assert!(cx.read_from_clipboard().is_none(), "empty copy writes nothing");
        assert!(window.notifications(cx).is_empty(), "empty copy notifies nothing");

        view.update(cx, |view, cx| {
            for (id, role, text) in [
                ("first", "user", "first prompt"),
                ("second", "assistant", "**second** reply"),
                ("tool-row", "tool", "Finished successfully"),
                ("third", "user", "third prompt"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), "Title".into(), text.into()));
            }
            assert!(view.palette_action_available(&super::super::PaletteAction::CopyLatestMessage));
            view.run_palette_action(super::super::PaletteAction::CopyLatestMessage, window, cx);
        });
        window.render_frame(cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("third prompt"));
        assert_eq!(window.notifications(cx).len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn tool_status_slot_only_renders_for_unfinished_states(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // An assistant row between the tools keeps both ungrouped.
            for (id, role, title, text) in [
                ("done-tool", "tool", "cargo check", "completed\noutput"),
                ("filler", "assistant", "VT Code", "Between."),
                ("failed-tool", "tool", "cargo check", "failed (exit 1)\ndetails"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), title.into(), text.into()));
            }
        });
        window.render_frame(cx);
        // Completed rows drop the redundant label (the state icon already
        // signals it); other states keep theirs. Both rows span the full
        // content width, so their chevrons stay docked on one edge.
        assert!(window.try_find("tool-status-done-tool").is_none(), "completed rows omit the status label");
        assert!(window.try_find("tool-status-failed-tool").is_some(), "failed rows keep the status label");
        let done = window.find("tool-done-tool").bounds();
        let failed = window.find("tool-failed-tool").bounds();
        assert_eq!(done.right(), failed.right(), "chevrons share one trailing edge");
        assert_eq!(done.size.height, failed.size.height, "rows hold their shape as states change");
    })
    .unwrap();
}

#[gpui_kit::test]
fn grouped_tool_rows_nest_under_their_header(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, title, text) in [
                ("intro", "assistant", "VT Code", "I will inspect the build."),
                ("read", "tool", "Read Cargo.toml", "completed\nmanifest"),
                ("build", "tool", "cargo check", "failed (exit 1)\nstructured details"),
                ("comment", "assistant", "VT Code", "The build needs a fix."),
                ("retry", "tool", "cargo check", "in progress\nretrying"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), title.into(), text.into()));
            }
        });
        window.render_frame(cx);
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        // Nesting and flush are text contracts: the toggles carry different
        // leading chrome (the group slot sits beside its toggle), so button
        // boxes no longer encode them. Titles share one spine instead.
        let header_x = window.find("activity-group-label-read").bounds().origin.x;
        let child_x = window.find("tool-title-build").bounds().origin.x;
        let single_x = window.find("tool-title-retry").bounds().origin.x;
        assert!(child_x > header_x, "grouped titles nest under their header label");
        assert_eq!(single_x, header_x, "single tool titles share the header spine");
    })
    .unwrap();
}

#[gpui_kit::test]
fn user_turns_render_without_dividers(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, text) in [
                ("first-turn", "user", "hello"),
                ("answer", "assistant", "Hi there."),
                ("second-turn", "user", "and then?"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), "Title".into(), text.into()));
            }
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-divider-first-turn").is_none(), "no divider above the first turn");
        assert!(window.try_find("turn-divider-second-turn").is_none(), "later turns have no divider");
    })
    .unwrap();
}

#[gpui_kit::test]
fn expanded_tool_output_paints_on_the_panel_fill(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "explain".into(),
                "assistant".into(),
                "VT Code".into(),
                "Checking.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "check".into(),
                "tool".into(),
                "cargo check".into(),
                "completed\nall targets".into(),
            ));
        });
        let panel: gpui_kit::Background = cx.theme().colors.secondary.into();
        window.render_frame(cx);
        let collapsed_panels = window.painted_quads().iter().filter(|quad| quad.background == panel).count();
        window.click("tool-check", cx);
        window.render_frame(cx);
        assert!(window.try_find("tool-output-check").is_some());
        let expanded_panels = window.painted_quads().iter().filter(|quad| quad.background == panel).count();
        assert!(expanded_panels > collapsed_panels, "expanding output adds a panel fill");
    })
    .unwrap();
}

#[gpui_kit::test]
fn context_boundary_hides_and_restores_rendered_context_usage(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{
        CompactionMode, CompactionTrigger, ThreadCompactBoundaryEvent, ThreadEvent, TurnCompletedEvent, Usage,
        VersionedThreadEvent,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.preferences.show_status_bar = true;
            let mut configuration = runtime_options_config();
            configuration.context_window = Some(200_000);
            view.configuration = Some(configuration);
            view.last_usage = Some(Usage {
                input_tokens: 90_000,
                output_tokens: 10_000,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
            });
        });
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_some());
        assert!(window.try_find("context-usage").is_some(), "usage renders before the boundary");

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::ThreadEvent(ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: None,
                    event: VersionedThreadEvent::new(ThreadEvent::ThreadCompactBoundary(Box::new(
                        ThreadCompactBoundaryEvent {
                            thread_id: "session".into(),
                            trigger: CompactionTrigger::Auto,
                            mode: CompactionMode::Local,
                            original_message_count: 120,
                            compacted_message_count: 8,
                            history_artifact_path: None,
                            previous_segment_id: None,
                            new_segment_id: None,
                            previous_prefix_hash: None,
                            new_prefix_hash: None,
                            previous_catalog_hash: None,
                            new_catalog_hash: None,
                        },
                    ))),
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_some());
        assert!(window.try_find("context-usage").is_none(), "boundary hides Context %");

        view.update(cx, |view, _| {
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("turn-1".into()),
                event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    final_response_item_id: None,
                    completed_at: None,
                    usage: Usage {
                        input_tokens: 5_000,
                        output_tokens: 500,
                        cached_input_tokens: 0,
                        cache_creation_tokens: 0,
                    },
                    in_progress_exec_sessions: Vec::new(),
                })),
            });
        });
        window.render_frame(cx);
        assert!(window.try_find("context-usage").is_some(), "next turn restores Context %");
    })
    .unwrap();
}

#[test]
fn item_update_coalescing_requires_the_same_session_turn_and_item() {
    let first = agent_message_update_event("session-a", "turn-a", "item-a", "first snapshot");
    let next = agent_message_update_event("session-a", "turn-a", "item-a", "latest snapshot");
    assert!(super::super::can_coalesce_item_updates(&first, &next));
    assert!(!super::super::can_coalesce_item_updates(
        &first,
        &agent_message_update_event("session-b", "turn-a", "item-a", "other session")
    ));
    assert!(!super::super::can_coalesce_item_updates(
        &first,
        &agent_message_update_event("session-a", "turn-b", "item-a", "other turn")
    ));
    assert!(!super::super::can_coalesce_item_updates(
        &first,
        &agent_message_update_event("session-a", "turn-a", "item-b", "other item")
    ));
}

#[gpui_kit::test]
fn event_drain_projects_the_latest_adjacent_item_snapshot(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn".into());
            view.running = true;
            view.event_tx
                .send(agent_message_update_event("session", "turn", "message", "Earlier snapshot"))
                .unwrap();
            view.event_tx
                .send(agent_message_update_event("session", "turn", "message", "Latest snapshot"))
                .unwrap();

            assert!(!view.drain_events(window, cx));
            let item = view.transcript.get(0).expect("the updated message is projected");
            assert_eq!(item.text.as_ref(), "Latest snapshot");
            assert_eq!(item.markdown.as_ref(), "Latest snapshot");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn plan_delta_chunks_accumulate_and_empty_recovery_checkpoints_stay_hidden(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ItemCompletedEvent, PlanItem, ThreadEvent, ThreadItem, ThreadItemDetails};

    let (_, view) = open_desktop(cx);
    cx.update_entity(&view, |view, _| {
        view.handle_thread_event(plan_delta_notification("session", "turn", "plan", "", true));
        assert!(view.transcript.is_empty(), "a checkpoint without text is not a visible plan row");

        view.handle_thread_event(plan_delta_notification("session", "turn", "plan", "Plan: ", false));
        view.handle_thread_event(plan_delta_notification("session", "turn", "plan", "inspect the change.", false));
        assert_eq!(view.transcript.len(), 1);
        assert_eq!(view.transcript.get(0).unwrap().text.as_ref(), "Plan: inspect the change.");

        view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "session".into(),
            turn_id: Some("turn".into()),
            event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    context: None,
                    id: "plan".into(),
                    details: ThreadItemDetails::Plan(PlanItem { text: "Final plan snapshot".into() }),
                },
            })),
        });
        assert_eq!(view.transcript.get(0).unwrap().text.as_ref(), "Final plan snapshot");
    });
}

#[gpui_kit::test]
fn rejected_plan_draft_events_render_a_visible_transcript_row(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ItemCompletedEvent, PlanItem, ThreadEvent, ThreadItem, ThreadItemDetails};

    // Terminal plan rejections publish the rejected draft as `{turn}-rejected-plan`
    // thread events because host runtimes never forward renderer transcript lines.
    let (_, view) = open_desktop(cx);
    cx.update_entity(&view, |view, _| {
        view.handle_thread_event(plan_delta_notification(
            "session",
            "turn",
            "turn-rejected-plan",
            "## Rejected draft",
            false,
        ));
        view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "session".into(),
            turn_id: Some("turn".into()),
            event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    context: None,
                    id: "turn-rejected-plan".into(),
                    details: ThreadItemDetails::Plan(PlanItem { text: "## Rejected draft".into() }),
                },
            })),
        });
        assert_eq!(view.transcript.len(), 1);
        assert_eq!(view.transcript.get(0).unwrap().text.as_ref(), "## Rejected draft");
    });
}

#[test]
fn desktop_buttons_use_pointer_when_enabled_and_arrow_when_disabled() {
    let mut enabled = super::super::desktop_button("native-cursor-enabled");
    assert_eq!(enabled.style().mouse_cursor, Some(CursorStyle::PointingHand));

    let mut disabled = super::super::desktop_button("native-cursor-disabled").desktop_disabled(true);
    assert_eq!(disabled.style().mouse_cursor, Some(CursorStyle::Arrow));

    let mut reenabled = super::super::desktop_button("native-cursor-reenabled").desktop_disabled(false);
    assert_eq!(reenabled.style().mouse_cursor, Some(CursorStyle::PointingHand));
}

#[test]
fn activity_details_omit_tool_output_from_the_metadata_copy() {
    use vtcode_exec_events::{
        CommandExecutionItem, CommandExecutionStatus, ThreadItemDetails, ToolCallStatus, ToolOutputItem,
    };

    let details = ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
        is_delta: None,
        call_id: "call-1".into(),
        tool_call_id: Some("provider-call-1".into()),
        spool_path: Some(".vtcode/tool-outputs/call-1".into()),
        output: "large captured result".repeat(32),
        exit_code: Some(0),
        status: ToolCallStatus::Completed,
        app_previews: Vec::new(),
    }));
    let payload = super::super::activity_details_payload(&details);

    assert!(payload.contains("call-1"), "tool identity remains available in metadata");
    assert!(!payload.contains("large captured result"), "the separate output field owns captured text");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        json!({
            "type": "tool_output",
            "call_id": "call-1",
            "tool_call_id": "provider-call-1",
            "spool_path": ".vtcode/tool-outputs/call-1",
            "exit_code": 0,
            "status": "completed"
        })
    );

    let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
        command: "cargo check".into(),
        arguments: Some(json!({ "cwd": "/workspace" })),
        aggregated_output: "discarded command output".repeat(64),
        exit_code: None,
        status: CommandExecutionStatus::InProgress,
    }));
    let payload = super::super::activity_details_payload(&command);
    assert!(!payload.contains("discarded command output"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        json!({
            "type": "command_execution",
            "command": "cargo check",
            "arguments": { "cwd": "/workspace" },
            "status": "in_progress"
        })
    );
}

#[test]
fn palette_entry_ids_follow_action_identity_without_exposing_payloads() {
    let path = "private/workspace/src/main.rs";
    let file_action = super::super::PaletteAction::PreviewFile(path.into());
    let file_id = palette_action_id(&file_action);
    assert_eq!(file_id, palette_action_id(&file_action));
    assert!(!file_id.contains(path), "workspace paths stay out of element identifiers");
    assert_ne!(
        file_id,
        palette_action_id(&super::super::PaletteAction::PreviewFile("private/workspace/src/lib.rs".into()))
    );
}

#[gpui_kit::test]
fn turn_pending_row_shows_until_first_content_then_clears(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = view.read(cx).status.clone();
        let pending = window.find("turn-pending");
        assert_eq!(pending.role(), Some(gpui_kit::Role::Status));
        assert_eq!(pending.label(), Some(status.as_str()));
        assert!(window.find("turn-pending-label").bounds().size.height > px(0.));

        // Lifecycle events carry no content: TurnStarted keeps the row up.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_started_event("session", "turn-1"), cx);
            assert!(view.running);
            assert!(!view.turn_content_seen);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_some());

        // First content clears the row while the turn keeps running.
        view.update(cx, |view, cx| {
            view.handle_backend_event(agent_message_update_event("session", "turn-1", "item-1", "hello back"), cx);
            assert!(view.running);
            assert!(view.turn_content_seen);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none());
        assert!(window.try_find("transcript-row-item-1").is_some());

        // Turn end keeps it cleared.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
            assert!(!view.running);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn turn_pending_row_clears_on_turn_end_without_content(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_some());

        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
            assert!(!view.running);
            assert!(!view.turn_content_seen, "no content arrived for this turn");
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none(), "idle view shows no pending row");
    })
    .unwrap();
}

#[gpui_kit::test]
fn turn_started_resets_content_seen_for_new_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    view.update(cx, |view, cx| {
        view.handle_backend_event(turn_started_event("session", "turn-1"), cx);
        view.handle_backend_event(agent_message_update_event("session", "turn-1", "item-1", "first"), cx);
        assert!(view.turn_content_seen);
        view.handle_backend_event(turn_completed_event("session", "turn-1"), cx);
        assert!(!view.running);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_none());

        // A new turn starts: the pending row returns until its content arrives.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_started_event("session", "turn-2"), cx);
            assert!(view.running);
            assert!(!view.turn_content_seen);
        });
        window.render_frame(cx);
        assert!(window.try_find("turn-pending").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_loading_shimmer_shows_while_page_loads(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    view.update(cx, |view, _| {
        view.history_has_earlier = true;
        view.history_page_loading = true;
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let loading = rust_i18n::t!("app.palette.history.loading").to_string();
        assert_eq!(window.find("load-earlier-history").label(), Some(loading.as_str()));
        assert!(window.find("history-loading-label").bounds().size.height > px(0.));

        view.update(cx, |view, _| {
            view.history_page_loading = false;
            view.history_has_earlier = false;
        });
        window.render_frame(cx);
        assert!(window.try_find("history-loading-label").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn loading_shimmer_labels_render_under_reduced_motion(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    running_turn_without_content(cx, &view);
    view.update(cx, |view, _| {
        view.pending_navigation = Some(PendingNavigation { target_index: 0, workspace_switch: false });
    });
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Static text stays readable with motion disabled; the Kit shimmer and
        // the shimmer takes its non-animated branch.
        assert!(window.try_find("turn-pending-label").is_some());
        assert!(window.try_find("navigation-loading-label").is_some());
        assert!(window.find("turn-pending-label").bounds().size.height > px(0.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn restarted_failure_status_shows_localized_recovery_copy(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".into());
        view.handle_backend_event(
            BackendEvent::TurnStatus(
                json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed", "restarted": true }),
            ),
            cx,
        );
        assert_eq!(view.status, rust_i18n::t!("app.service.turn_recovered").to_string());
        // A plain failure without the escalation marker keeps the raw label.
        view.handle_backend_event(
            BackendEvent::TurnStatus(json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed" })),
            cx,
        );
        assert_eq!(view.status, "failed");
    });
}

#[gpui_kit::test]
fn loading_updates_require_the_selected_session_and_reset_on_session_change(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.handle_backend_event(BackendEvent::UiProgress(json!({"active": true})), cx);
        assert!(!view.ui_progress_active);
        view.selected_session_id = Some("current".into());
        view.handle_backend_event(BackendEvent::UiProgress(json!({"sessionId": "other", "active": true})), cx);
        assert!(!view.ui_progress_active);
        view.handle_backend_event(BackendEvent::UiProgress(json!({"sessionId": "current", "active": true})), cx);
        assert!(view.ui_progress_active);
        view.reset_session_state();
        assert!(!view.ui_progress_active);
    });
}
