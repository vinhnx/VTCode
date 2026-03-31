use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::task;
use vtcode_tui::app::{InlineHandle, InlineSession, TransientSubmission};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::url_guard::{
    UrlGuardDecision, UrlGuardPrompt, open_external_url, url_guard_decision,
};

pub(crate) struct ExternalUrlGuardContext<'a> {
    handle: &'a InlineHandle,
    session: &'a mut InlineSession,
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<Notify>,
}

impl<'a> ExternalUrlGuardContext<'a> {
    pub(crate) fn new(
        handle: &'a InlineHandle,
        session: &'a mut InlineSession,
        ctrl_c_state: &'a Arc<CtrlCState>,
        ctrl_c_notify: &'a Arc<Notify>,
    ) -> Self {
        Self {
            handle,
            session,
            ctrl_c_state,
            ctrl_c_notify,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExternalUrlGuardOutcome {
    Approved,
    Cancelled,
    Exit,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExternalUrlOpenOutcome {
    Opened,
    OpenFailed(String),
    Cancelled,
    Exit,
    Unsupported,
}

pub(crate) async fn request_external_url_guard(
    ctx: ExternalUrlGuardContext<'_>,
    url: &str,
) -> Result<ExternalUrlGuardOutcome> {
    let Some(prompt) = UrlGuardPrompt::parse(url.to_string()) else {
        return Ok(ExternalUrlGuardOutcome::Unsupported);
    };

    let outcome = show_overlay_and_wait(
        ctx.handle,
        ctx.session,
        prompt.request(),
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(selection) => url_guard_decision(&selection),
            _ => None,
        },
    )
    .await?;

    close_guard_modal(ctx.handle).await;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(UrlGuardDecision::Approve) => {
            ExternalUrlGuardOutcome::Approved
        }
        OverlayWaitOutcome::Submitted(UrlGuardDecision::Deny)
        | OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted => ExternalUrlGuardOutcome::Cancelled,
        OverlayWaitOutcome::Exit => ExternalUrlGuardOutcome::Exit,
    })
}

pub(crate) async fn request_external_url_open(
    ctx: ExternalUrlGuardContext<'_>,
    url: &str,
) -> Result<ExternalUrlOpenOutcome> {
    Ok(match request_external_url_guard(ctx, url).await? {
        ExternalUrlGuardOutcome::Approved => {
            if let Err(err) = open_external_url(url) {
                ExternalUrlOpenOutcome::OpenFailed(err.to_string())
            } else {
                ExternalUrlOpenOutcome::Opened
            }
        }
        ExternalUrlGuardOutcome::Cancelled => ExternalUrlOpenOutcome::Cancelled,
        ExternalUrlGuardOutcome::Exit => ExternalUrlOpenOutcome::Exit,
        ExternalUrlGuardOutcome::Unsupported => ExternalUrlOpenOutcome::Unsupported,
    })
}

async fn close_guard_modal(handle: &InlineHandle) {
    handle.close_modal();
    handle.force_redraw();
    task::yield_now().await;
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalUrlGuardContext, ExternalUrlGuardOutcome, ExternalUrlOpenOutcome,
        request_external_url_guard, request_external_url_open,
    };
    use crate::agent::runloop::unified::state::CtrlCState;
    use crate::agent::runloop::unified::url_guard::URL_GUARD_TITLE;
    use std::sync::Arc;
    use tokio::sync::Notify;
    use tokio::sync::mpsc;
    use vtcode_tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineSession,
        TransientEvent, TransientRequest, TransientSubmission,
    };

    fn session_with_channels() -> (
        InlineHandle,
        mpsc::UnboundedReceiver<InlineCommand>,
        mpsc::UnboundedSender<InlineEvent>,
        InlineSession,
    ) {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let session = InlineSession {
            handle: handle.clone(),
            events: event_rx,
        };

        (handle, command_rx, event_tx, session)
    }

    #[tokio::test]
    async fn request_external_url_guard_shows_modal_and_accepts_approval() {
        let (handle, mut command_rx, event_tx, mut session) = session_with_channels();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let task = tokio::spawn({
            let handle = handle.clone();
            let ctrl_c_state = ctrl_c_state.clone();
            let ctrl_c_notify = ctrl_c_notify.clone();
            async move {
                request_external_url_guard(
                    ExternalUrlGuardContext::new(
                        &handle,
                        &mut session,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    ),
                    "https://example.com/docs",
                )
                .await
            }
        });

        let command = command_rx.recv().await.expect("show transient command");
        match command {
            InlineCommand::ShowTransient { request } => match *request {
                TransientRequest::List(request) => assert_eq!(request.title, URL_GUARD_TITLE),
                other => panic!("expected list request, got {other:?}"),
            },
            _ => panic!("expected transient command"),
        }

        event_tx
            .send(InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::Selection(InlineListSelection::ConfigAction(
                    "url_guard:approve".to_string(),
                )),
            )))
            .expect("send approval selection");

        let outcome = task.await.expect("join guard task").expect("guard result");
        assert_eq!(outcome, ExternalUrlGuardOutcome::Approved);
    }

    #[tokio::test]
    async fn request_external_url_guard_returns_unsupported_for_non_http_targets() {
        let (handle, mut command_rx, _event_tx, mut session) = session_with_channels();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let outcome = request_external_url_guard(
            ExternalUrlGuardContext::new(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify),
            "mailto:test@example.com",
        )
        .await
        .expect("unsupported guard result");

        assert_eq!(outcome, ExternalUrlGuardOutcome::Unsupported);
        assert!(command_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn request_external_url_open_returns_unsupported_without_opening_browser() {
        let (handle, mut command_rx, _event_tx, mut session) = session_with_channels();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let outcome = request_external_url_open(
            ExternalUrlGuardContext::new(&handle, &mut session, &ctrl_c_state, &ctrl_c_notify),
            "mailto:test@example.com",
        )
        .await
        .expect("unsupported open result");

        assert_eq!(outcome, ExternalUrlOpenOutcome::Unsupported);
        assert!(command_rx.try_recv().is_err());
    }
}
