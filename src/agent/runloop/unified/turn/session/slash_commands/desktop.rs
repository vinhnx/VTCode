use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::{find_session_by_identifier, history_persistence_enabled};
use vtcode_desktop_protocol::handoff_url::{HandoffLink, handoff_url};

use super::{SlashCommandContext, SlashCommandControl};
use crate::cli::desktop::resolve_desktop_app;

/// Open the desktop app linked to the current thread.
///
/// Fast path first: a `vtcode://` URL goes straight to the running app (no
/// new process, no reboot). When no bundle claims the scheme (dev builds,
/// outdated installs), fall back to resolving the app like `vtcode desktop`
/// and launching it with the thread id and workspace as discrete `--args`
/// argv items (never through a shell).
pub(crate) async fn handle_open_desktop(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !cfg!(target_os = "macos") {
        ctx.renderer
            .line(MessageStyle::Error, "the VT Code desktop app is only available on macOS")?;
        return Ok(SlashCommandControl::Continue);
    }
    // Best-effort link check: the lookup only chooses the message and the
    // workspace to open, so a failure must never block the launch. A linked
    // thread must open in its OWN workspace: the desktop service filters
    // sessions by workspace and rejects cross-workspace loads, and a
    // `vtcode --resume` from another directory makes the archive workspace
    // differ from this process's workspace.
    let listing = find_session_by_identifier(ctx.thread_id).await.ok().flatten();
    let archive_workspace = listing
        .as_ref()
        .map(|listing| listing.snapshot.metadata.workspace_path.as_str())
        .filter(|path| !path.is_empty());
    let linked = archive_workspace.is_some();
    let workspace = archive_workspace
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.config.workspace.clone());
    let url = handoff_url(&HandoffLink {
        session_id: ctx.thread_id.to_string(),
        workspace: workspace.clone(),
    });
    // Nulled stdio: `open` diagnostics must never reach the TUI tty; all
    // user messaging goes through the renderer.
    match open_command().arg(&url).status() {
        Ok(status) if status.success() => report_desktop_opened(&mut ctx, linked, &workspace)?,
        _ => legacy_desktop_open(&mut ctx, &workspace, linked)?,
    }
    Ok(SlashCommandControl::Continue)
}

/// Legacy path: resolve the bundle and boot a fresh instance with argv.
/// Reached only when no app claims the `vtcode://` scheme.
fn legacy_desktop_open(ctx: &mut SlashCommandContext<'_>, workspace: &Path, linked: bool) -> Result<()> {
    let app = match resolve_desktop_app(None) {
        Ok(app) => app,
        Err(err) => {
            ctx.renderer.line(MessageStyle::Error, &format!("{err:#}"))?;
            return Ok(());
        }
    };
    let mut command = desktop_open_command(&app, ctx.thread_id, workspace);
    let status = command
        .status()
        .with_context(|| format!("failed to launch {}", app.display()))?;
    if !status.success() {
        ctx.renderer
            .line(MessageStyle::Error, &format!("failed to open {}", app.display()))?;
        return Ok(());
    }
    report_desktop_opened(ctx, linked, workspace)
}

fn report_desktop_opened(ctx: &mut SlashCommandContext<'_>, linked: bool, workspace: &Path) -> Result<()> {
    if linked {
        // The CLI holds the thread's exclusive lock until this session ends,
        // so the desktop load fails fast until the user exits here.
        if workspace != ctx.config.workspace.as_path() {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Opened VT Code Desktop linked to thread {} in {}. Exit this session so the desktop can load it.",
                    ctx.thread_id,
                    workspace.display()
                ),
            )?;
        } else {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Opened VT Code Desktop linked to thread {}. Exit this session so the desktop can load it.",
                    ctx.thread_id
                ),
            )?;
        }
    } else if history_persistence_enabled() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Opened VT Code Desktop. Thread {} has no saved checkpoint yet, so it opened unlinked; run a turn and retry /desktop.",
                ctx.thread_id
            ),
        )?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "Opened VT Code Desktop. History persistence is disabled, so thread {} cannot be linked.",
                ctx.thread_id
            ),
        )?;
    }
    Ok(())
}

/// `open` with nulled stdio: child diagnostics must never reach the TUI
/// tty (the fallback probe fails routinely on dev builds); all user
/// messaging goes through the renderer.
fn open_command() -> Command {
    let mut command = Command::new("open");
    command.stdout(Stdio::null()).stderr(Stdio::null());
    command
}

/// Build the `open` invocation: `-n` addresses the bundle directly (mirrors
/// `vtcode desktop`), and `--args` forwards the link identity to the app.
/// Thread id and workspace travel as separate argv items so hostile
/// characters can never fuse with flags or each other.
fn desktop_open_command(app: &Path, thread_id: &str, workspace: &Path) -> Command {
    let mut command = open_command();
    command
        .arg("-n")
        .arg(app)
        .arg("--args")
        .arg("--session")
        .arg(thread_id)
        .arg("--workspace")
        .arg(workspace);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_argv_carries_session_and_workspace_as_discrete_items() {
        let app = Path::new("/Applications/VT Code.app");
        let command = desktop_open_command(app, "thread-1", Path::new("/repo/workspace"));
        let argv: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            argv,
            vec![
                "-n",
                "/Applications/VT Code.app",
                "--args",
                "--session",
                "thread-1",
                "--workspace",
                "/repo/workspace",
            ]
        );
    }

    #[test]
    fn hostile_session_id_stays_one_argv_item() {
        let app = Path::new("/Applications/VT Code.app");
        let hostile = "--workspace /etc; rm -rf ~";
        let command = desktop_open_command(app, hostile, Path::new("/repo"));
        let argv: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(argv.len(), 7);
        assert_eq!(argv[4], hostile);
    }
}
