use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_tui::{InlineEvent, InlineHandle};

use crate::agent::runloop::unified::state::CtrlCState;

pub(super) async fn prompt_session_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
) -> Result<Option<usize>> {
    use vtcode_tui::{InlineListItem, InlineListSelection};

    let description_lines = vec![
        format!("Session tool limit reached: {}", max_limit),
        "Would you like to increase the limit to continue?".to_string(),
        "".to_string(),
        "Use ↑↓ or Tab to navigate • Enter to select • Esc to deny".to_string(),
    ];

    let options = vec![
        InlineListItem {
            title: "+100 tool calls".to_string(),
            subtitle: Some("Increase the session limit by 100".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SessionLimitIncrease(100)),
            search_value: Some("increase 100 hundred plus more".to_string()),
        },
        InlineListItem {
            title: "+50 tool calls".to_string(),
            subtitle: Some("Increase the session limit by 50".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SessionLimitIncrease(50)),
            search_value: Some("increase 50 fifty plus more".to_string()),
        },
        InlineListItem {
            title: "".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: None,
        },
        InlineListItem {
            title: "Deny".to_string(),
            subtitle: Some("Do not increase limit (stops tool execution)".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no exit stop cancel".to_string()),
        },
    ];

    prompt_limit_increase_modal(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        "Session Limit Reached".to_string(),
        description_lines,
        options,
        100,
    )
    .await
}

pub(super) async fn prompt_tool_loop_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
) -> Result<Option<usize>> {
    use vtcode_tui::{InlineListItem, InlineListSelection};

    let description_lines = vec![
        format!("Maximum tool loops reached: {}", max_limit),
        "Would you like to continue with more tool loops?".to_string(),
        "".to_string(),
        "Use ↑↓ or Tab to navigate • Enter to select • Esc to stop".to_string(),
    ];

    let options = vec![
        InlineListItem {
            title: "+50 tool loops".to_string(),
            subtitle: Some("Continue with 50 more tool loops".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SessionLimitIncrease(50)),
            search_value: Some("increase 50 fifty plus more continue".to_string()),
        },
        InlineListItem {
            title: "+20 tool loops".to_string(),
            subtitle: Some("Continue with 20 more tool loops".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SessionLimitIncrease(20)),
            search_value: Some("increase 20 twenty plus more continue".to_string()),
        },
        InlineListItem {
            title: "+10 tool loops".to_string(),
            subtitle: Some("Continue with 10 more tool loops".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SessionLimitIncrease(10)),
            search_value: Some("increase 10 ten plus more continue".to_string()),
        },
        InlineListItem {
            title: "".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: None,
        },
        InlineListItem {
            title: "Stop".to_string(),
            subtitle: Some("Stop the current turn and wait for input".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("stop no exit cancel done".to_string()),
        },
    ];

    prompt_limit_increase_modal(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        "Tool Loop Limit Reached".to_string(),
        description_lines,
        options,
        20,
    )
    .await
}

async fn prompt_limit_increase_modal<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    title: String,
    description_lines: Vec<String>,
    options: Vec<vtcode_tui::InlineListItem>,
    default_increment: usize,
) -> Result<Option<usize>> {
    use vtcode_tui::InlineListSelection;

    handle.show_list_modal(
        title,
        description_lines,
        options.clone(),
        Some(InlineListSelection::SessionLimitIncrease(default_increment)),
        None,
    );

    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_modal();
            handle.force_redraw();
            return Ok(None);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_modal();
            handle.force_redraw();
            return Ok(None);
        };

        match event {
            InlineEvent::ListModalSubmit(selection) => {
                handle.close_modal();
                handle.force_redraw();
                match selection {
                    InlineListSelection::SessionLimitIncrease(inc) => return Ok(Some(inc)),
                    _ => return Ok(None),
                }
            }
            InlineEvent::ListModalCancel | InlineEvent::Cancel | InlineEvent::Exit => {
                handle.close_modal();
                handle.force_redraw();
                return Ok(None);
            }
            InlineEvent::Interrupt => {
                let _signal = ctrl_c_state.register_signal();
                ctrl_c_notify.notify_waiters();
                handle.close_modal();
                handle.force_redraw();
                return Ok(None);
            }
            _ => continue,
        }
    }
}
