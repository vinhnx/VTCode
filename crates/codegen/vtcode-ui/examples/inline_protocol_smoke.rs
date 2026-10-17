//! Interactive visual fixture for both public inline protocols; no provider or tools.
//! Run `cargo run --locked -p vtcode-ui --example inline_protocol_smoke -- app`
//! or replace `app` with `core`. See `docs/development/ui-message-protocol.md`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use vtcode_ui::tui::core_tui::{InlineLinkRange, InlineLinkTarget, InlineMessageKind, InlineSegment, InlineTextStyle};
use vtcode_ui::tui::{app, core_tui as core};

fn segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

// Only the fixture is shared: both real handles and event enums remain distinct.
macro_rules! exercise {
    ($session:ident, $protocol:ident) => {{
        let handle = $session.clone_inline_handle();
        handle.set_terminal_title_items(Some(vec!["VT Code protocol smoke".to_owned()]));
        handle.set_placeholder(Some("stream | paste | replace | overlay | quit".to_owned()));
        handle.set_input_status(Some("isolated fixture".to_owned()), Some("ready".to_owned()));
        for index in 0..24 {
            handle.append_line(
                InlineMessageKind::Agent,
                vec![segment(format!(
                    "row {index:02}: α / 界 / asymmetric text that wraps after resizing the terminal"
                ))],
            );
        }
        handle.append_line(InlineMessageKind::Agent, vec![segment("temporary row")]);
        let link = "https://example.com/fixture";
        handle.replace_last_with_links(
            1,
            InlineMessageKind::Agent,
            vec![vec![segment(link)]],
            vec![vec![InlineLinkRange {
                start: 0,
                end: link.len(),
                target: InlineLinkTarget::Url(link.to_owned()),
            }]],
        );

        loop {
            let event = $session
                .next_event()
                .await
                .context("TUI closed before a fixture exit request")?;
            match event {
                $protocol::InlineEvent::Submit(input) => {
                    handle.clear_input();
                    match input.text.trim() {
                        "quit" => break,
                        "stream" => {
                            handle.append_line(InlineMessageKind::Agent, vec![segment("stream:")]);
                            for text in [" first α", " second 界", " final β"] {
                                handle.inline(InlineMessageKind::Agent, segment(text));
                                tokio::time::sleep(Duration::from_millis(300)).await;
                            }
                            handle.stop_event_stream();
                            handle.resume_event_loop();
                            handle.start_event_stream();
                        }
                        "paste" => handle.append_pasted_message(
                            InlineMessageKind::User,
                            "one α\ntwo longer 界\nthree β".to_owned(),
                            3,
                        ),
                        "replace" => handle.replace_last(
                            1,
                            InlineMessageKind::Agent,
                            vec![
                                vec![segment("replacement first α")],
                                vec![segment("replacement second 界")],
                            ],
                        ),
                        "overlay" => handle.show_modal(
                            "Protocol fixture".to_owned(),
                            vec![
                                "Select this text, resize, then press Esc.".to_owned(),
                                "overlay tail β".to_owned(),
                            ],
                            None,
                        ),
                        _ => handle.append_line(InlineMessageKind::User, vec![segment(input.text)]),
                    }
                    let (columns, rows) = crossterm::terminal::size().context("read fixture viewport")?;
                    handle.set_input_status(Some(format!("viewport {columns}x{rows}")), Some("ready".to_owned()));
                }
                $protocol::InlineEvent::OpenUrl(url) => handle.append_line(
                    InlineMessageKind::Info,
                    vec![segment(format!("Link event: {url} (fixture does not open it)"))],
                ),
                $protocol::InlineEvent::Exit | $protocol::InlineEvent::Interrupt => break,
                _ => {}
            }
        }
        handle.shutdown();
        Result::<()>::Ok(())
    }};
}

async fn run(protocol: &str) -> Result<()> {
    match protocol {
        "app" => {
            let mut session = app::spawn_session_with_options(
                app::InlineTheme::default(),
                app::SessionOptions {
                    surface_preference: app::SessionSurface::Alternate,
                    app_name: "Protocol fixture: app".to_owned(),
                    ..app::SessionOptions::default()
                },
            )?;
            exercise!(session, app)?;
            if !session.wait_for_exit(Duration::from_secs(2)).await {
                bail!("app TUI did not finish teardown within two seconds");
            }
        }
        "core" => {
            let mut session = core::spawn_session(
                core::InlineTheme::default(),
                None,
                vtcode_ui::tui::UiSurfacePreference::Alternate,
                30,
                None,
                None,
                None,
            )?;
            exercise!(session, core)?;
            tokio::time::timeout(Duration::from_secs(2), async { while session.next_event().await.is_some() {} })
                .await
                .context("core TUI did not finish teardown within two seconds")?;
        }
        _ => bail!("expected protocol 'app' or 'core'"),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let protocol = std::env::args().nth(1).unwrap_or_else(|| "app".to_owned());
    let result = run(&protocol).await;
    core::panic_hook::restore_tui().context("restore fixture terminal")?;
    result?;
    println!("Protocol fixture {protocol}: exited after terminal teardown.");
    Ok(())
}
