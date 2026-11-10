use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use vtcode_core::core::threads::fork_thread_for_handoff;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::{
    SessionArchiveMetadata, SessionListing, find_session_by_identifier, history_persistence_enabled,
};
use vtcode_desktop_protocol::handoff_url::{HandoffLink, handoff_url};

use super::{SlashCommandContext, SlashCommandControl};
use crate::cli::desktop::resolve_desktop_app;

/// What the desktop app received from the handoff.
enum Handoff {
    /// A fork the desktop app owns outright; it can load it immediately.
    Forked { fork_id: String },
    /// The live thread id: the desktop app will try to load it and find
    /// this session still holding the lock.
    LiveThreadOnly,
    /// The thread has no saved checkpoint to hand over at all.
    NoCheckpoint,
}

/// Open the desktop app with a copy of the current thread.
///
/// The handoff is a **fork**, not a resume-in-place: the canonical event log
/// is single-writer, and this CLI session holds the exclusive lock on it for
/// its whole lifetime. A receiver that resumed the same thread id would fail
/// with `SessionBusy`, so the desktop app is handed a forked copy it can own
/// outright. This mirrors the desktop -> CLI direction, which already forks
/// for the same reason.
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
        .map(|listing| listing.snapshot.metadata.workspace_path.clone())
        .filter(|path| !path.is_empty());
    let workspace = archive_workspace
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.config.workspace.clone());
    // Fork the live thread so the desktop app owns its own copy. The fork
    // inherits the source thread's history and records it as its parent.
    let handoff = match listing.as_ref() {
        Some(listing) => {
            match fork_thread_for_handoff(listing, workspace.clone(), handoff_metadata(&ctx, listing)).await {
                Ok((fork_id, _archive)) => Handoff::Forked { fork_id },
                Err(error) => {
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("Failed to prepare the desktop handoff: {error:#}"))?;
                    Handoff::LiveThreadOnly
                }
            }
        }
        None => Handoff::NoCheckpoint,
    };
    // Without a fork the desktop app can still open the thread, but it will
    // refuse to load it while this session holds the lock.
    let session_id = handoff_session_id(&handoff, ctx.thread_id);
    let url = handoff_url(&HandoffLink {
        session_id: session_id.clone(),
        workspace: workspace.clone(),
    });
    // The URL is addressed to the resolved bundle (`-a`), not to whatever
    // app the `vtcode:` scheme happens to be bound to: several builds can
    // claim the scheme, and only the resolved bundle is the user's app.
    // Addressing the bundle also keeps a running instance on the delivery
    // path instead of starting a second one.
    let app = match resolve_desktop_app(None) {
        Ok(app) => app,
        Err(error) => {
            ctx.renderer.line(MessageStyle::Error, &format!("{error:#}"))?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    // Nulled stdio: `open` diagnostics must never reach the TUI tty; all
    // user messaging goes through the renderer.
    match desktop_open_url_command(&app, &url).status() {
        Ok(status) if status.success() => report_desktop_opened(&mut ctx, &handoff, &workspace)?,
        _ => legacy_desktop_open(&mut ctx, &app, &session_id, &handoff, &workspace)?,
    }
    Ok(SlashCommandControl::Continue)
}

/// The session id the desktop app is asked to load: the fork when one was
/// prepared, otherwise the live thread (which the desktop app cannot load
/// while this session holds the lock).
fn handoff_session_id(handoff: &Handoff, thread_id: &str) -> String {
    match handoff {
        Handoff::Forked { fork_id } => fork_id.clone(),
        Handoff::LiveThreadOnly | Handoff::NoCheckpoint => thread_id.to_owned(),
    }
}

/// Metadata for the forked archive: the live session's model, provider, and
/// theme, so the desktop app continues with the same configuration.
fn handoff_metadata(ctx: &SlashCommandContext<'_>, listing: &SessionListing) -> SessionArchiveMetadata {
    let mut metadata = listing.snapshot.metadata.clone();
    metadata.model = ctx.config.model.clone();
    metadata.provider = ctx.config.provider.clone();
    metadata.theme = ctx.config.theme.clone();
    metadata.reasoning_effort = ctx.config.reasoning_effort.as_str().to_string();
    metadata
}

/// Legacy path: boot a fresh instance with the link in argv. Reached only
/// when the URL delivery path fails (an app that cannot accept a URL, or a
/// bundle that is not registered for the `vtcode:` scheme).
fn legacy_desktop_open(
    ctx: &mut SlashCommandContext<'_>,
    app: &Path,
    session_id: &str,
    handoff: &Handoff,
    workspace: &Path,
) -> Result<()> {
    let mut command = desktop_open_command(app, session_id, workspace);
    let status = command
        .status()
        .with_context(|| format!("failed to launch {}", app.display()))?;
    if !status.success() {
        ctx.renderer
            .line(MessageStyle::Error, &format!("failed to open {}", app.display()))?;
        return Ok(());
    }
    report_desktop_opened(ctx, handoff, workspace)
}

fn report_desktop_opened(ctx: &mut SlashCommandContext<'_>, handoff: &Handoff, workspace: &Path) -> Result<()> {
    match handoff {
        // The desktop app owns a forked copy and can continue it while this
        // CLI session keeps its own.
        Handoff::Forked { .. } => {
            if workspace != ctx.config.workspace.as_path() {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Opened VT Code Desktop with a copy of thread {} in {}.",
                        ctx.thread_id,
                        workspace.display()
                    ),
                )?;
            } else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Opened VT Code Desktop with a copy of thread {}.", ctx.thread_id),
                )?;
            }
        }
        // The fork failed, so the desktop app only received the original
        // id and cannot load it while this session holds the lock.
        Handoff::LiveThreadOnly => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Opened VT Code Desktop for thread {}. This session still holds the thread, so the desktop app cannot load it yet.",
                    ctx.thread_id
                ),
            )?;
        }
        Handoff::NoCheckpoint => {
            if history_persistence_enabled() {
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
        }
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

/// Build the URL delivery invocation: `-a` addresses the resolved bundle, so
/// the link reaches the user's app instead of whichever bundle happens to
/// claim the `vtcode:` scheme. Addressing the bundle by path also keeps a
/// running instance on the delivery path (no second app is started).
fn desktop_open_url_command(app: &Path, url: &str) -> Command {
    let mut command = open_command();
    command.arg("-a").arg(app).arg(url);
    command
}

/// Build the `open` invocation for the argv fallback: `-n` starts a fresh
/// instance because the running one cannot read new argv, and the bundle is
/// addressed by path (display-name lookup is ambiguous when several
/// development builds share one bundle identifier).
///
/// Thread id and workspace travel as separate argv items so hostile
/// characters can never fuse with flags or each other.
fn desktop_open_command(app: &Path, session_id: &str, workspace: &Path) -> Command {
    let mut command = open_command();
    command
        .arg("-n")
        .arg(app)
        .arg("--args")
        .arg("--session")
        .arg(session_id)
        .arg("--workspace")
        .arg(workspace);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_delivery_addresses_the_resolved_bundle() {
        let app = Path::new("/Applications/VT Code.app");
        let command = desktop_open_url_command(app, "vtcode://session/thread-1?workspace=%2Frepo");
        let argv: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            argv,
            vec![
                "-a",
                "/Applications/VT Code.app",
                "vtcode://session/thread-1?workspace=%2Frepo"
            ]
        );
    }

    #[test]
    fn url_delivery_keeps_the_link_as_one_argv_item() {
        let app = Path::new("/Applications/VT Code.app");
        let hostile = "vtcode://session/a b?workspace=/etc; rm -rf ~";
        let command = desktop_open_url_command(app, hostile);
        let argv: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(argv.len(), 3);
        assert_eq!(argv[2], hostile);
    }

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

    /// The desktop app loads the id it is handed, so a fork must be the id
    /// on the wire and the live thread must not be.
    #[test]
    fn a_forked_handoff_hands_the_fork_id_to_the_desktop() {
        let forked = Handoff::Forked { fork_id: "fork-1".to_owned() };
        assert_eq!(handoff_session_id(&forked, "live-1"), "fork-1");

        // Without a fork the live id goes out, and the desktop app will find
        // this session still holding its lock.
        assert_eq!(handoff_session_id(&Handoff::LiveThreadOnly, "live-1"), "live-1");
        assert_eq!(handoff_session_id(&Handoff::NoCheckpoint, "live-1"), "live-1");
    }
}
