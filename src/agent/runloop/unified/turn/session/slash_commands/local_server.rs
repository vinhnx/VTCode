use anyhow::Result;
use vtcode_core::llm::providers::local_server::{self, LocalProvider, LocalServerStatus};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection};

use crate::agent::runloop::slash_commands::LocalServerAction;

use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};

const LOCAL_ACTION_PREFIX: &str = "local.action.";
const LOCAL_ACTION_BACK: &str = "local.action.back";
const LOCAL_PROVIDER_PREFIX: &str = "local.provider.";
const LOCAL_DETAIL_BACK: &str = "local.detail.back";

fn action_key(action: &str) -> String {
    format!("{LOCAL_ACTION_PREFIX}{action}")
}

fn provider_key(provider: &LocalProvider) -> String {
    format!("{}{}", LOCAL_PROVIDER_PREFIX, provider.key())
}

pub(crate) async fn handle_manage_local_server(
    mut ctx: SlashCommandContext<'_>,
    action: LocalServerAction,
) -> Result<SlashCommandControl> {
    match action {
        LocalServerAction::Interactive => {
            run_interactive_local_manager(&mut ctx).await?;
        }
        LocalServerAction::Status { provider } => {
            execute_status(&mut ctx, provider.as_deref()).await?;
        }
        LocalServerAction::Start { provider } => {
            execute_start(&mut ctx, provider.as_deref()).await?;
        }
        LocalServerAction::Stop { provider } => {
            execute_stop(&mut ctx, provider.as_deref()).await?;
        }
        LocalServerAction::Configure { provider } => {
            execute_configure(&mut ctx, provider.as_deref()).await?;
        }
        LocalServerAction::Troubleshoot { provider } => {
            execute_troubleshoot(&mut ctx, provider.as_deref()).await?;
        }
        LocalServerAction::Provider { name } => {
            if let Some(provider) = LocalProvider::from_key(&name) {
                run_provider_action_loop(&mut ctx, provider).await?;
            }
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

// ---------------------------------------------------------------------------
// Interactive manager
// ---------------------------------------------------------------------------

async fn run_interactive_local_manager(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if !ensure_selection_ui_available(ctx, "local server manager")? {
        return Ok(());
    }

    if !ctx.renderer.supports_inline_ui() {
        // Non-inline fallback: show all statuses as text
        let statuses = local_server::probe_all().await;
        for status in &statuses {
            render_provider_status_text(ctx, status)?;
        }
        return Ok(());
    }

    loop {
        let statuses = local_server::probe_all().await;
        show_local_providers_modal(ctx, &statuses);

        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == LOCAL_ACTION_BACK {
            return Ok(());
        }

        let Some(provider_key) = action.strip_prefix(LOCAL_PROVIDER_PREFIX) else {
            continue;
        };
        let Some(provider) = LocalProvider::from_key(provider_key) else {
            continue;
        };

        run_provider_action_loop(ctx, provider).await?;
    }
}

async fn run_provider_action_loop(
    ctx: &mut SlashCommandContext<'_>,
    provider: LocalProvider,
) -> Result<()> {
    loop {
        let status = local_server::probe(provider).await;
        show_local_actions_modal(ctx, provider, &status);

        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == LOCAL_ACTION_BACK {
            return Ok(());
        }

        let Some(action_key) = action.strip_prefix(LOCAL_ACTION_PREFIX) else {
            continue;
        };

        match action_key {
            "status" => {
                let status = local_server::probe(provider).await;
                let lines = format_provider_status(&status);
                show_local_detail_modal(ctx, &format!("{} Status", provider.display_name()), lines);
                wait_for_list_modal_selection(ctx).await;
            }
            "start" => {
                let result = local_server::start(provider).await;
                let lines = format_result("Start", &result);
                show_local_detail_modal(ctx, &format!("{} Start", provider.display_name()), lines);
                wait_for_list_modal_selection(ctx).await;
            }
            "stop" => {
                let result = local_server::stop(provider).await;
                let lines = format_result("Stop", &result);
                show_local_detail_modal(ctx, &format!("{} Stop", provider.display_name()), lines);
                wait_for_list_modal_selection(ctx).await;
            }
            "configure" => {
                let lines = format_provider_config(provider);
                show_local_detail_modal(
                    ctx,
                    &format!("{} Configure", provider.display_name()),
                    lines,
                );
                wait_for_list_modal_selection(ctx).await;
            }
            "troubleshoot" => {
                let status = local_server::probe(provider).await;
                let caps = local_server::capabilities(provider);
                let guidance = local_server::troubleshoot(&status, &caps);
                show_local_detail_modal(
                    ctx,
                    &format!("{} Troubleshoot", provider.display_name()),
                    guidance,
                );
                wait_for_list_modal_selection(ctx).await;
            }
            _ => continue,
        }
    }
}

// ---------------------------------------------------------------------------
// Modal rendering
// ---------------------------------------------------------------------------

fn show_local_providers_modal(ctx: &mut SlashCommandContext<'_>, statuses: &[LocalServerStatus]) {
    let items: Vec<InlineListItem> = statuses
        .iter()
        .map(|status| {
            let (badge, subtitle) = if status.running {
                let model_count = status.available_models.len();
                let model_text = if model_count == 1 {
                    "1 model".to_string()
                } else {
                    format!("{model_count} models")
                };
                ("Running".to_string(), format!("{} | {}", status.endpoint, model_text))
            } else {
                let err = status.error.as_deref().unwrap_or("Not running");
                ("Stopped".to_string(), format!("{} | {}", status.endpoint, err))
            };
            InlineListItem {
                title: status.provider.display_name().to_string(),
                subtitle: Some(subtitle),
                badge: Some(badge),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(provider_key(&status.provider))),
                search_value: Some(format!(
                    "{} {} {}",
                    status.provider.display_name(),
                    status.endpoint,
                    if status.running { "running" } else { "stopped" }
                )),
            }
        })
        .collect();

    let back_item = InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Close local server manager".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(LOCAL_ACTION_BACK.to_string())),
        search_value: Some("back close".to_string()),
    };
    let mut all_items = items;
    all_items.push(back_item);

    ctx.renderer.show_list_modal(
        "Local Inference Servers",
        vec!["Manage local LLM backends. Select a provider.".to_string()],
        all_items,
        None,
        Some(InlineListSearchConfig {
            label: "Search providers".to_string(),
            placeholder: Some("ollama, lmstudio, llamacpp".to_string()),
        }),
    );
}

fn show_local_actions_modal(
    ctx: &mut SlashCommandContext<'_>,
    provider: LocalProvider,
    status: &LocalServerStatus,
) {
    let header = if status.running {
        let model_info = if status.available_models.is_empty() {
            "no models loaded".to_string()
        } else {
            format!("{} model(s)", status.available_models.len())
        };
        let ver = status.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
        let mut header = format!("{} | {}{}", status.endpoint, model_info, ver);
        if !status.running_models.is_empty() {
            header.push_str(&format!(" | running: {}", status.running_models.join(", ")));
        }
        header
    } else {
        format!("{} | {}", status.endpoint, status.error.as_deref().unwrap_or("Not running"))
    };

    let items = vec![
        InlineListItem {
            title: "Status".to_string(),
            subtitle: Some("Check server health and loaded models".to_string()),
            badge: Some("Info".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action_key("status"))),
            search_value: Some("status health check models".to_string()),
        },
        InlineListItem {
            title: "Start server".to_string(),
            subtitle: Some("Launch the inference server".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action_key("start"))),
            search_value: Some("start launch run".to_string()),
        },
        InlineListItem {
            title: "Stop server".to_string(),
            subtitle: Some("Shut down the inference server".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action_key("stop"))),
            search_value: Some("stop shutdown kill".to_string()),
        },
        InlineListItem {
            title: "Configure".to_string(),
            subtitle: Some("Show environment variables and settings".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action_key("configure"))),
            search_value: Some("configure settings env variables".to_string()),
        },
        InlineListItem {
            title: "Troubleshoot".to_string(),
            subtitle: Some("Diagnose and fix connection issues".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action_key("troubleshoot"))),
            search_value: Some("troubleshoot diagnose fix debug".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Return to provider list".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(LOCAL_ACTION_BACK.to_string())),
            search_value: Some("back return".to_string()),
        },
    ];

    ctx.renderer.show_list_modal(
        provider.display_name(),
        vec![header],
        items,
        Some(InlineListSelection::ConfigAction(action_key("status"))),
        None,
    );
}

fn show_local_detail_modal(ctx: &mut SlashCommandContext<'_>, title: &str, lines: Vec<String>) {
    let items = vec![InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Return to actions".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(LOCAL_DETAIL_BACK.to_string())),
        search_value: Some("back return".to_string()),
    }];

    ctx.renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::ConfigAction(LOCAL_DETAIL_BACK.to_string())),
        None,
    );
}

// ---------------------------------------------------------------------------
// Non-interactive execution (for explicit subcommands and non-inline fallback)
// ---------------------------------------------------------------------------

fn resolve_provider<'a>(
    ctx: &mut SlashCommandContext<'_>,
    key: Option<&'a str>,
    usage: &str,
) -> Result<Option<(&'a str, LocalProvider)>> {
    let Some(key) = key else {
        ctx.renderer.line(MessageStyle::Error, usage)?;
        return Ok(None);
    };
    match LocalProvider::from_key(key) {
        Some(p) => Ok(Some((key, p))),
        None => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Unknown provider '{key}'. Use: ollama, lmstudio, llamacpp"),
            )?;
            Ok(None)
        }
    }
}

async fn execute_status(ctx: &mut SlashCommandContext<'_>, provider: Option<&str>) -> Result<()> {
    match resolve_provider(ctx, provider, "")? {
        Some((_, p)) => {
            let status = local_server::probe(p).await;
            render_provider_status_text(ctx, &status)?;
        }
        None if provider.is_some() => return Ok(()),
        None => {
            let statuses = local_server::probe_all().await;
            for status in &statuses {
                render_provider_status_text(ctx, status)?;
            }
        }
    }
    Ok(())
}

async fn execute_start(ctx: &mut SlashCommandContext<'_>, provider: Option<&str>) -> Result<()> {
    let Some((_, p)) = resolve_provider(
        ctx,
        provider,
        "Usage: /local start <provider> (ollama, lmstudio, llamacpp)",
    )?
    else {
        return Ok(());
    };

    ctx.renderer
        .line(MessageStyle::Info, &format!("Starting {} server...", p.display_name()))?;

    match local_server::start(p).await {
        Ok(msg) => ctx.renderer.line(MessageStyle::Info, &msg)?,
        Err(err) => ctx.renderer.line(MessageStyle::Error, &err.to_string())?,
    }
    Ok(())
}

async fn execute_stop(ctx: &mut SlashCommandContext<'_>, provider: Option<&str>) -> Result<()> {
    let Some((_, p)) = resolve_provider(
        ctx,
        provider,
        "Usage: /local stop <provider> (ollama, lmstudio, llamacpp)",
    )?
    else {
        return Ok(());
    };

    match local_server::stop(p).await {
        Ok(msg) => ctx.renderer.line(MessageStyle::Info, &msg)?,
        Err(err) => ctx.renderer.line(MessageStyle::Error, &err.to_string())?,
    }
    Ok(())
}

async fn execute_configure(
    ctx: &mut SlashCommandContext<'_>,
    provider: Option<&str>,
) -> Result<()> {
    if let Some((_, p)) = resolve_provider(ctx, provider, "")? {
        render_provider_config_text(ctx, p)?;
    } else if provider.is_none() {
        for &p in LocalProvider::all() {
            render_provider_config_text(ctx, p)?;
            ctx.renderer.line(MessageStyle::Info, "")?;
        }
    }
    Ok(())
}

async fn execute_troubleshoot(
    ctx: &mut SlashCommandContext<'_>,
    provider: Option<&str>,
) -> Result<()> {
    if let Some((_, p)) = resolve_provider(ctx, provider, "")? {
        let status = local_server::probe(p).await;
        let caps = local_server::capabilities(p);
        let guidance = local_server::troubleshoot(&status, &caps);
        for line in &guidance {
            ctx.renderer.line(MessageStyle::Info, line)?;
        }
    } else if provider.is_none() {
        for &p in LocalProvider::all() {
            let status = local_server::probe(p).await;
            let caps = local_server::capabilities(p);
            let guidance = local_server::troubleshoot(&status, &caps);
            for line in &guidance {
                ctx.renderer.line(MessageStyle::Info, line)?;
            }
            ctx.renderer.line(MessageStyle::Info, "")?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Text rendering helpers
// ---------------------------------------------------------------------------

fn render_provider_status_text(
    ctx: &mut SlashCommandContext<'_>,
    status: &LocalServerStatus,
) -> Result<()> {
    let name = status.provider.display_name();
    if status.running {
        let ver = status.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "{} ({}{}) - Running, {} model(s) available",
                name,
                status.endpoint,
                ver,
                status.available_models.len()
            ),
        )?;
        if !status.running_models.is_empty() {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("  Running: {}", status.running_models.join(", ")),
            )?;
        }
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "{} ({}) - {}",
                name,
                status.endpoint,
                status.error.as_deref().unwrap_or("Not running")
            ),
        )?;
    }
    Ok(())
}

fn render_provider_config_text(
    ctx: &mut SlashCommandContext<'_>,
    provider: LocalProvider,
) -> Result<()> {
    ctx.renderer
        .line(MessageStyle::Info, &format!("{}:", provider.display_name()))?;
    for line in format_provider_config(provider) {
        ctx.renderer.line(MessageStyle::Info, &format!("  {line}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Formatting helpers (for detail modals)
// ---------------------------------------------------------------------------

fn format_provider_status(status: &LocalServerStatus) -> Vec<String> {
    let mut lines = Vec::new();

    if status.running {
        lines.push(format!("Endpoint:  {}", status.endpoint));
        if let Some(ver) = &status.version {
            lines.push(format!("Version:   {ver}"));
        }
        lines.push(format!(
            "Models:    {}",
            if status.available_models.is_empty() {
                "(none)".to_string()
            } else {
                status.available_models.join(", ")
            }
        ));
        if !status.running_models.is_empty() {
            lines.push(format!("Running:   {}", status.running_models.join(", ")));
        }
    } else {
        lines.push("Status:    Not running".to_string());
        if let Some(err) = &status.error {
            lines.push(format!("Reason:    {err}"));
        }
        lines.push(String::new());
        lines.push(format!("Run /local troubleshoot {} for help.", status.provider.key()));
    }

    lines
}

fn format_result(label: &str, result: &Result<String>) -> Vec<String> {
    match result {
        Ok(msg) => vec![format!("{}: {}", label, msg)],
        Err(err) => vec![
            format!("{}: Failed", label),
            String::new(),
            format!("Error: {}", err),
        ],
    }
}

fn format_provider_config(provider: LocalProvider) -> Vec<String> {
    let mut lines = Vec::new();
    let env_vars = local_server::env_config(provider);

    for var in &env_vars {
        let value = var.current_value.as_deref().unwrap_or("(not set)");
        lines.push(format!("{} = {}", var.name, value));
        lines.push(format!("  {}", var.description));
    }

    if env_vars.is_empty() {
        lines.push("No configurable environment variables.".to_string());
    }

    lines
}
