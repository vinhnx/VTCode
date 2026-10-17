use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::webmcp::ActiveWebmcpBridge;

use super::{SlashCommandContext, SlashCommandControl};

const BROWSER_PAIRING_INSTRUCTION: &str =
    "In the WebMCP editor, open Connect to a local VT Code bridge and paste the WebSocket URL and pairing code above.";
const CONFIRM_WEBMCP_ACTION: &str = "webmcp:confirm";
const CANCEL_WEBMCP_ACTION: &str = "webmcp:cancel";
const WEBMCP_COMMAND_GUIDE: &[&str] = &[
    "WebMCP commands:",
    "  /webmcp or /webmcp status                 Show listener and pairing details.",
    "  /webmcp help                              Show this command guide.",
    "  /webmcp pair <exact-browser-origin>       Start the active-session bridge.",
    "  /webmcp pair --replace <origin>           Confirm and issue a fresh pairing.",
    "  /webmcp tools                             List available browser tools.",
    "  /webmcp roots                             Show configured workspace-root guidance.",
    "  /webmcp unpair                            Confirm and disconnect the bridge.",
    "Active agent turns require pairing from this same VT Code TUI session.",
    "The browser origin must exactly match the origin entered in the pair command.",
    "Pairing another configured origin keeps existing authenticated sessions active.",
];

pub(crate) async fn handle_start_webmcp(
    mut ctx: SlashCommandContext<'_>,
    origin: String,
    replace: bool,
) -> Result<SlashCommandControl> {
    if let Some((endpoint, pairing_origin, pairing_code, expires_in_secs)) =
        ctx.webmcp_bridge.as_ref().map(bridge_details)
    {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP is already listening in this VT Code session.")?;

        if !replace && pairing_origin != origin {
            let Some(bridge) = ctx.webmcp_bridge.as_mut() else {
                return Ok(SlashCommandControl::Continue);
            };
            bridge.begin_pairing_for_origin(&origin)?;
            let (new_endpoint, new_pairing_origin, new_pairing_code, new_expires_in_secs) = bridge_details(bridge);
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("WebMCP pairing issued for {new_pairing_origin}; existing browser sessions remain active."),
            )?;
            render_bridge_details(
                &mut ctx,
                &new_endpoint,
                &new_pairing_origin,
                &new_pairing_code,
                new_expires_in_secs,
            )?;
            ctx.renderer.line(MessageStyle::Info, BROWSER_PAIRING_INSTRUCTION)?;
            return Ok(SlashCommandControl::Continue);
        }

        ctx.renderer
            .line(MessageStyle::Info, "WebMCP is listening in the active VT Code session.")?;
        render_bridge_details(&mut ctx, &endpoint, &pairing_origin, &pairing_code, expires_in_secs)?;

        if !replace {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "The current pairing for {origin} remains active. To revoke its sessions and issue a new code, run `/webmcp pair --replace {origin}`."
                ),
            )?;
            return Ok(SlashCommandControl::Continue);
        }

        if !ctx.renderer.supports_inline_ui() {
            ctx.renderer.line(
                MessageStyle::Error,
                "Replacing the WebMCP bridge requires an interactive terminal confirmation.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }
        if !super::ui::ensure_selection_ui_available(&mut ctx, "confirming WebMCP replacement")? {
            return Ok(SlashCommandControl::Continue);
        }

        let confirmed = confirm_webmcp_action(
            &mut ctx,
            "Replace WebMCP bridge",
            vec![
                format!("A browser is connected to {endpoint}."),
                format!("Disconnect it and issue a new pairing for {origin}?"),
                "The current browser connection will close and its pairing will be revoked.".to_string(),
            ],
            "Disconnect and re-pair",
            "Issue a new one-time pairing code on the active listener",
        )
        .await?;
        if !confirmed {
            ctx.renderer.line(MessageStyle::Info, "WebMCP replacement cancelled.")?;
            return Ok(SlashCommandControl::Continue);
        }

        let Some(bridge) = ctx.webmcp_bridge.as_mut() else {
            return Ok(SlashCommandControl::Continue);
        };
        bridge.replace_pairing(&origin)?;
        let (new_endpoint, new_pairing_origin, new_pairing_code, new_expires_in_secs) = bridge_details(bridge);
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP pairing replaced; previous browser sessions were revoked.")?;
        render_bridge_details(&mut ctx, &new_endpoint, &new_pairing_origin, &new_pairing_code, new_expires_in_secs)?;
        ctx.renderer.line(MessageStyle::Info, BROWSER_PAIRING_INSTRUCTION)?;
        return Ok(SlashCommandControl::Continue);
    }

    let bridge = ActiveWebmcpBridge::start(
        &ctx.config.workspace,
        ctx.vt_cfg.as_ref(),
        &origin,
        ctx.webmcp_prompt_sender.clone(),
        ctx.harness_emitter.map(|e| (e.clone(), ctx.handle.clone())),
    )
    .await?;
    let endpoint = bridge.endpoint().to_string();
    let pairing_code = bridge.pairing_code().to_string();
    let expires_in_secs = bridge.pairing_expires_in_secs();
    if let Some(emitter) = ctx.harness_emitter {
        emitter.attach_webmcp_event_hub(bridge.event_hub())?;
    }
    let previous_bridge = ctx.webmcp_bridge.replace(bridge);
    drop(previous_bridge);

    ctx.renderer.line(MessageStyle::Info, "Active WebMCP bridge started.")?;
    render_bridge_details(&mut ctx, &endpoint, &origin, &pairing_code, expires_in_secs)?;
    ctx.renderer.line(MessageStyle::Info, BROWSER_PAIRING_INSTRUCTION)?;
    ctx.renderer.line(
        MessageStyle::Info,
        "Browser writes remain proposals; VT Code's terminal permission flow is authoritative.",
    )?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_webmcp_status(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if let Some((endpoint, pairing_origin, pairing_code, expires_in_secs)) =
        ctx.webmcp_bridge.as_ref().map(bridge_details)
    {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP is listening in the active VT Code session.")?;
        render_bridge_details(&mut ctx, &endpoint, &pairing_origin, &pairing_code, expires_in_secs)?;
        ctx.renderer.line(MessageStyle::Info, BROWSER_PAIRING_INSTRUCTION)?;
        ctx.renderer.line(
            MessageStyle::Info,
            "To issue a pairing for another configured origin, run `/webmcp pair <exact-browser-origin>`; use `--replace` to revoke current sessions.",
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            "WebMCP is not listening. Use `/webmcp pair http://localhost:5173` to attach the browser editor.",
        )?;
    }
    render_webmcp_command_guide(&mut ctx)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_webmcp_help(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    render_webmcp_command_guide(&mut ctx)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_stop_webmcp(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some((endpoint, pairing_origin, pairing_code, expires_in_secs)) =
        ctx.webmcp_bridge.as_ref().map(bridge_details)
    else {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP was not listening in this session.")?;
        return Ok(SlashCommandControl::Continue);
    };

    ctx.renderer
        .line(MessageStyle::Info, "WebMCP is listening in the active VT Code session.")?;
    render_bridge_details(&mut ctx, &endpoint, &pairing_origin, &pairing_code, expires_in_secs)?;

    if !ctx.renderer.supports_inline_ui() {
        ctx.renderer.line(
            MessageStyle::Error,
            "Disconnecting the WebMCP bridge requires an interactive terminal confirmation.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }
    if !super::ui::ensure_selection_ui_available(&mut ctx, "confirming WebMCP disconnect")? {
        return Ok(SlashCommandControl::Continue);
    }

    let confirmed = confirm_webmcp_action(
        &mut ctx,
        "Disconnect WebMCP bridge",
        vec![
            format!("Disconnect the bridge at {endpoint}?"),
            "All browser connections will close and their pairing sessions will be revoked.".to_string(),
            "This does not change workspace files.".to_string(),
        ],
        "Disconnect bridge",
        "Stop the listener and revoke browser sessions",
    )
    .await?;
    if !confirmed {
        ctx.renderer.line(MessageStyle::Info, "WebMCP disconnect cancelled.")?;
        return Ok(SlashCommandControl::Continue);
    }

    if ctx.webmcp_bridge.take().is_some() {
        ctx.renderer
            .line(MessageStyle::Info, "WebMCP stopped; browser pairings were revoked.")?;
    }
    Ok(SlashCommandControl::Continue)
}

fn bridge_details(bridge: &ActiveWebmcpBridge) -> (String, String, String, u64) {
    (
        bridge.endpoint().to_string(),
        bridge.pairing_origin().to_string(),
        bridge.pairing_code().to_string(),
        bridge.pairing_expires_in_secs(),
    )
}

fn render_bridge_details(
    ctx: &mut SlashCommandContext<'_>,
    endpoint: &str,
    origin: &str,
    pairing_code: &str,
    expires_in_secs: u64,
) -> Result<()> {
    ctx.renderer.line(MessageStyle::Info, &format!("WebSocket: {endpoint}"))?;
    ctx.renderer.line(MessageStyle::Info, &format!("Browser origin: {origin}"))?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Pairing code: {pairing_code} (expires in {expires_in_secs} seconds; one-time)"),
    )?;
    Ok(())
}

fn render_webmcp_command_guide(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    for line in WEBMCP_COMMAND_GUIDE {
        ctx.renderer.line(MessageStyle::Info, line)?;
    }
    Ok(())
}

async fn confirm_webmcp_action(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    lines: Vec<String>,
    confirm_title: &str,
    confirm_subtitle: &str,
) -> Result<bool> {
    ctx.handle.show_list_modal(
        title.to_string(),
        lines,
        vec![
            InlineListItem {
                title: confirm_title.to_string(),
                subtitle: Some(confirm_subtitle.to_string()),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(CONFIRM_WEBMCP_ACTION.to_string())),
                search_value: Some("confirm proceed yes".to_string()),
                ..Default::default()
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Leave the current WebMCP bridge unchanged".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(CANCEL_WEBMCP_ACTION.to_string())),
                search_value: Some("cancel keep current no".to_string()),
                ..Default::default()
            },
        ],
        Some(InlineListSelection::ConfigAction(CANCEL_WEBMCP_ACTION.to_string())),
        None,
    );

    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(false);
    };
    Ok(matches!(
        selection,
        InlineListSelection::ConfigAction(action) if action == CONFIRM_WEBMCP_ACTION
    ))
}
