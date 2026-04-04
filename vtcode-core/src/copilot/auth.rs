use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use regex::Regex;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::timeout;
use url::Url;
use vtcode_config::auth::CopilotAuthConfig;

use crate::utils::ansi_parser::strip_ansi;

use super::command::{ResolvedCopilotCommand, copilot_command_available, resolve_copilot_command};
use super::types::{COPILOT_AUTH_DOC_PATH, CopilotAuthEvent, CopilotAuthStatus};

const DEFAULT_HOST_URL: &str = "https://github.com";
const ENV_AUTH_VARS: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];
static DEVICE_FLOW_LINE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)visit\s+(https?://\S+)\s+and\s+enter code\s+([A-Z0-9-]+)")
        .expect("device flow regex must compile")
});

pub async fn login(config: &CopilotAuthConfig, workspace_root: &Path) -> Result<()> {
    login_with_events(config, workspace_root, |_| Ok(())).await
}

pub async fn login_with_events<F>(
    config: &CopilotAuthConfig,
    workspace_root: &Path,
    mut on_event: F,
) -> Result<()>
where
    F: FnMut(CopilotAuthEvent) -> Result<()>,
{
    let resolved = resolve_copilot_command(config).context("invalid copilot command")?;
    if let Err(err) = ensure_command_available(&resolved) {
        emit_missing_command_guidance(config, &mut on_event)?;
        on_event(CopilotAuthEvent::Failure {
            message: err.to_string(),
        })?;
        return Err(err);
    }

    let host = resolve_copilot_host(config)?;
    let args = login_command_args(&host);

    run_captured_command(
        &resolved,
        workspace_root,
        &args,
        "copilot login",
        CommandKind::Login,
        &mut on_event,
    )
    .await?;

    let account = probe_auth_status(config, Some(workspace_root))
        .await
        .message
        .as_deref()
        .and_then(extract_account_from_status_message)
        .map(ToString::to_string);
    on_event(CopilotAuthEvent::Success { account })?;
    Ok(())
}

pub async fn logout(config: &CopilotAuthConfig, workspace_root: &Path) -> Result<()> {
    logout_with_events(config, workspace_root, |_| Ok(())).await
}

pub async fn logout_with_events<F>(
    config: &CopilotAuthConfig,
    workspace_root: &Path,
    mut on_event: F,
) -> Result<()>
where
    F: FnMut(CopilotAuthEvent) -> Result<()>,
{
    let resolved = resolve_copilot_command(config).context("invalid copilot command")?;
    if let Err(err) = ensure_command_available(&resolved) {
        emit_missing_command_guidance(config, &mut on_event)?;
        on_event(CopilotAuthEvent::Failure {
            message: err.to_string(),
        })?;
        return Err(err);
    }

    let host = resolve_copilot_host(config)?;
    let interactive_result = run_interactive_logout_command(&resolved, workspace_root, &host)
        .await
        .with_context(|| "copilot logout started an interactive Copilot CLI session");

    if let Err(interactive_err) = interactive_result {
        let args = logout_command_args();
        let direct_logout = run_captured_command(
            &resolved,
            workspace_root,
            &args,
            "copilot logout",
            CommandKind::Logout,
            &mut on_event,
        )
        .await;

        match direct_logout {
            Ok(()) => {}
            Err(err) if should_retry_logout_interactively(&err.to_string()) => {
                return Err(interactive_err);
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("interactive copilot logout failed: {interactive_err}")
                });
            }
        }
    }

    on_event(CopilotAuthEvent::Success { account: None })?;
    Ok(())
}

pub async fn probe_auth_status(
    config: &CopilotAuthConfig,
    workspace_root: Option<&Path>,
) -> CopilotAuthStatus {
    let host = match resolve_copilot_host(config) {
        Ok(host) => host,
        Err(err) => return CopilotAuthStatus::auth_flow_failed(err.to_string()),
    };

    let auth_source = match detect_auth_source(&host, workspace_root).await {
        Ok(source) => source,
        Err(err) => return CopilotAuthStatus::auth_flow_failed(err.to_string()),
    };

    let resolved = match resolve_copilot_command(config) {
        Ok(resolved) => resolved,
        Err(err) => return CopilotAuthStatus::auth_flow_failed(err.to_string()),
    };

    if !copilot_command_available(&resolved) {
        return CopilotAuthStatus::server_unavailable(format!(
            "GitHub Copilot CLI command `{}` was not found. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`.{source_suffix}",
            resolved.display(),
            source_suffix = auth_source
                .as_ref()
                .map(auth_source_suffix)
                .unwrap_or_default(),
        ));
    }

    match auth_source {
        Some(source) => CopilotAuthStatus::authenticated(Some(source.message(&host))),
        None => CopilotAuthStatus::unauthenticated(Some(format!(
            "No GitHub Copilot authentication source found for {}. Run `vtcode login copilot`, or set one of {}. `gh auth login` is only used as an optional fallback.",
            host.gh_hostname,
            ENV_AUTH_VARS.join(", ")
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    Login,
    Logout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturedStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct CapturedLine {
    stream: CapturedStream,
    text: String,
}

async fn run_captured_command<F>(
    resolved: &ResolvedCopilotCommand,
    workspace_root: &Path,
    extra_args: &[String],
    action_name: &str,
    kind: CommandKind,
    on_event: &mut F,
) -> Result<()>
where
    F: FnMut(CopilotAuthEvent) -> Result<()>,
{
    let mut command = resolved.command(Some(workspace_root), extra_args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn `{}`", resolved.display()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("{action_name} stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("{action_name} stderr unavailable"))?;

    let (line_tx, mut line_rx) = mpsc::unbounded_channel();
    spawn_line_reader(stdout, CapturedStream::Stdout, line_tx.clone());
    spawn_line_reader(stderr, CapturedStream::Stderr, line_tx);
    let mut state = CapturedCommandState::default();

    let status = match timeout(resolved.auth_timeout, async {
        let wait_result: Result<std::process::ExitStatus> = loop {
            tokio::select! {
                status = child.wait() => {
                    break status.with_context(|| format!("{action_name} process failed"));
                }
                maybe_line = line_rx.recv() => {
                    let Some(line) = maybe_line else {
                        continue;
                    };
                    state.handle_line(kind, line, on_event)?;
                }
            }
        };
        wait_result
    })
    .await
    {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.start_kill();
            let message = format!(
                "{action_name} timed out after {} seconds",
                resolved.auth_timeout.as_secs()
            );
            on_event(CopilotAuthEvent::Failure {
                message: message.clone(),
            })?;
            return Err(anyhow!(message));
        }
    };

    while let Ok(line) = line_rx.try_recv() {
        state.handle_line(kind, line, on_event)?;
    }

    if status.success() {
        Ok(())
    } else {
        let message = state.failure_message(action_name, status);
        on_event(CopilotAuthEvent::Failure {
            message: message.clone(),
        })?;
        Err(anyhow!(message))
    }
}

#[derive(Default)]
struct CapturedCommandState {
    emitted_verification_code: bool,
    emitted_waiting_message: bool,
    last_safe_message: Option<String>,
}

impl CapturedCommandState {
    fn handle_line<F>(
        &mut self,
        kind: CommandKind,
        line: CapturedLine,
        on_event: &mut F,
    ) -> Result<()>
    where
        F: FnMut(CopilotAuthEvent) -> Result<()>,
    {
        let normalized = normalize_captured_line(&line.text);
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        if matches!(kind, CommandKind::Logout)
            && matches!(line.stream, CapturedStream::Stdout)
            && trimmed
                .to_ascii_lowercase()
                .contains("non-interactive mode")
        {
            self.record_safe_message(trimmed.to_string());
            return Ok(());
        }

        if matches!(kind, CommandKind::Login)
            && let Some(event) = parse_login_event(trimmed)
        {
            match &event {
                CopilotAuthEvent::VerificationCode { .. } if self.emitted_verification_code => {
                    return Ok(());
                }
                CopilotAuthEvent::VerificationCode { .. } => {
                    self.emitted_verification_code = true;
                }
                CopilotAuthEvent::Progress { message }
                    if message.eq_ignore_ascii_case("Waiting for authorization")
                        && self.emitted_waiting_message =>
                {
                    return Ok(());
                }
                CopilotAuthEvent::Progress { message }
                    if message.eq_ignore_ascii_case("Waiting for authorization") =>
                {
                    self.emitted_waiting_message = true;
                }
                _ => {}
            }
            return on_event(event);
        }

        if let Some(message) = sanitize_cli_line(trimmed, line.stream) {
            self.record_safe_message(message);
        }
        Ok(())
    }

    fn record_safe_message(&mut self, message: String) {
        let is_low_signal = is_low_signal_cli_hint(&message);
        match self.last_safe_message.as_ref() {
            Some(existing) if !is_low_signal_cli_hint(existing) && is_low_signal => {}
            _ => {
                self.last_safe_message = Some(message);
            }
        }
    }

    fn failure_message(&self, action_name: &str, status: std::process::ExitStatus) -> String {
        if let Some(message) = self.last_safe_message.as_deref() {
            format!("{action_name} exited with status {status}: {message}")
        } else {
            format!("{action_name} exited with status {status}")
        }
    }
}

fn spawn_line_reader<R>(
    reader: R,
    stream: CapturedStream,
    line_tx: mpsc::UnboundedSender<CapturedLine>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];

        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => {
                    buffer.extend_from_slice(&chunk[..read]);
                    for text in drain_complete_lines(&mut buffer) {
                        let _ = line_tx.send(CapturedLine { stream, text });
                    }
                }
                Err(_) => return,
            }
        }

        if !buffer.is_empty() {
            let text = String::from_utf8_lossy(&buffer).into_owned();
            let _ = line_tx.send(CapturedLine { stream, text });
        }
    });
}

fn drain_complete_lines(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;

    while index < buffer.len() {
        let byte = buffer[index];
        if byte == b'\n' || byte == b'\r' {
            let line = String::from_utf8_lossy(&buffer[start..index]).into_owned();
            lines.push(line);

            if byte == b'\r' && buffer.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            start = index;
            continue;
        }
        index += 1;
    }

    if start > 0 {
        buffer.drain(..start);
    }

    lines
}

fn parse_login_event(line: &str) -> Option<CopilotAuthEvent> {
    if let Some((url, user_code)) = parse_device_flow_code(line) {
        return Some(CopilotAuthEvent::VerificationCode { url, user_code });
    }

    let lower = line.to_ascii_lowercase();
    if lower.contains("waiting for authorization") {
        return Some(CopilotAuthEvent::Progress {
            message: "Waiting for authorization".to_string(),
        });
    }
    if lower.contains("opening browser") || lower.contains("opened browser") {
        return Some(CopilotAuthEvent::Progress {
            message: "Opened the browser for GitHub device authorization".to_string(),
        });
    }
    None
}

fn parse_device_flow_code(line: &str) -> Option<(String, String)> {
    let captures = DEVICE_FLOW_LINE_RE.captures(line)?;
    let url = captures
        .get(1)?
        .as_str()
        .trim_end_matches(['.', ',', ')', ']'])
        .to_string();
    let code = captures
        .get(2)?
        .as_str()
        .trim_matches(|ch: char| matches!(ch, '.' | ',' | ':' | ';'))
        .to_string();
    (!url.is_empty() && !code.is_empty()).then_some((url, code))
}

fn normalize_captured_line(line: &str) -> String {
    strip_ansi(line)
        .chars()
        .filter(|ch| {
            !matches!(
                ch,
                '\u{0000}'..='\u{0008}'
                    | '\u{000B}'
                    | '\u{000C}'
                    | '\u{000E}'..='\u{001F}'
                    | '\u{007F}'
            )
        })
        .collect()
}

fn login_command_args(host: &CopilotHost) -> Vec<String> {
    let mut args = vec!["login".to_string()];
    if !host.is_default() {
        args.push("--host".to_string());
        args.push(host.url.clone());
    }
    args
}

fn logout_command_args() -> Vec<String> {
    vec!["logout".to_string()]
}

fn sanitize_cli_line(line: &str, stream: CapturedStream) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("copilot_github_token")
        || lower.contains("gh_token")
        || lower.contains("github_token")
        || lower.contains("auth-token-env")
    {
        return Some(
            "GitHub Copilot CLI reported an authentication configuration issue.".to_string(),
        );
    }
    if lower.contains("secitemcopymatching failed") {
        return Some(
            "GitHub Copilot CLI failed to access the macOS Keychain while clearing credentials."
                .to_string(),
        );
    }

    match stream {
        CapturedStream::Stdout => None,
        CapturedStream::Stderr => Some(line.to_string()),
    }
}

fn is_low_signal_cli_hint(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.eq_ignore_ascii_case("Try 'copilot --help' for more information.")
}

fn extract_account_from_status_message(message: &str) -> Option<&str> {
    let login = message.split(" for ").nth(1)?.split(" on ").next()?.trim();
    (!login.is_empty()).then_some(login)
}

fn should_retry_logout_interactively(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("for non-interactive mode, use the -p or --prompt option")
}

async fn run_interactive_logout_command(
    resolved: &ResolvedCopilotCommand,
    workspace_root: &Path,
    host: &CopilotHost,
) -> Result<()> {
    let resolved = resolved.clone();
    let workspace_root = workspace_root.to_path_buf();
    let host = host.clone();
    tokio::task::spawn_blocking(move || {
        blocking_interactive_logout_command(&resolved, &workspace_root, &host)
    })
    .await
    .context("failed to join interactive copilot logout task")?
}

fn blocking_interactive_logout_command(
    resolved: &ResolvedCopilotCommand,
    workspace_root: &Path,
    host: &CopilotHost,
) -> Result<()> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("failed to allocate PTY for interactive copilot logout")?;

    let mut builder = CommandBuilder::new(&resolved.program);
    for arg in &resolved.args {
        builder.arg(arg);
    }
    builder.cwd(workspace_root);
    builder.env("TERM", "xterm-256color");
    builder.env("COLUMNS", "80");
    builder.env("LINES", "24");

    let mut child = pair
        .slave
        .spawn_command(builder)
        .with_context(|| format!("failed to spawn `{}`", resolved.display()))?;
    let mut killer = child.clone_killer();
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .context("failed to clone PTY reader for copilot logout")?;
    let mut writer = pair
        .master
        .take_writer()
        .context("failed to take PTY writer for copilot logout")?;

    let writer_thread = thread::spawn(move || -> Result<()> {
        writer
            .write_all(b"/logout\n")
            .context("failed to send /logout to Copilot CLI")?;
        writer
            .flush()
            .context("failed to flush /logout to Copilot CLI")?;
        thread::sleep(Duration::from_millis(250));
        writer
            .write_all(b"/exit\n")
            .context("failed to send /exit to Copilot CLI")?;
        writer
            .flush()
            .context("failed to flush /exit to Copilot CLI")?;
        Ok(())
    });

    let (line_tx, line_rx) = std::sync::mpsc::channel();
    let reader_thread = thread::spawn(move || -> Result<()> {
        let mut chunk = [0_u8; 1024];
        let mut buffer = Vec::new();
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    buffer.extend_from_slice(&chunk[..read]);
                    for text in drain_complete_lines(&mut buffer) {
                        let _ = line_tx.send(text);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(error).context("failed to read interactive copilot logout output");
                }
            }
        }

        if !buffer.is_empty() {
            let text = String::from_utf8_lossy(&buffer).into_owned();
            let _ = line_tx.send(text);
        }

        Ok(())
    });

    let (wait_tx, wait_rx) = std::sync::mpsc::channel();
    let wait_thread = thread::spawn(move || {
        let status = child.wait();
        let _ = wait_tx.send(());
        status
    });

    let start = Instant::now();
    let mut last_auth_check = Instant::now();
    let mut auth_cleared = false;
    let mut state = CapturedCommandState::default();
    let wait_granularity = Duration::from_millis(100);

    loop {
        while let Ok(text) = line_rx.try_recv() {
            state.handle_line(
                CommandKind::Logout,
                CapturedLine {
                    stream: CapturedStream::Stderr,
                    text,
                },
                &mut |_| Ok(()),
            )?;
        }

        if wait_rx.try_recv().is_ok() {
            break;
        }

        if last_auth_check.elapsed() >= Duration::from_millis(250) {
            last_auth_check = Instant::now();
            if stored_auth_source(host)?.is_none() {
                auth_cleared = true;
                let _ = killer.kill();
                break;
            }
        }

        if start.elapsed() >= resolved.auth_timeout {
            let _ = killer.kill();
            let _ = writer_thread.join();
            let _ = reader_thread.join();
            let _ = wait_thread.join();
            return Err(anyhow!(
                "copilot logout timed out after {} seconds",
                resolved.auth_timeout.as_secs()
            ));
        }

        thread::sleep(wait_granularity);
    }

    let status = wait_thread.join().map_err(|panic| {
        anyhow!(
            "interactive copilot logout wait thread panicked: {:?}",
            panic
        )
    })?;

    let writer_result = writer_thread.join().map_err(|panic| {
        anyhow!(
            "interactive copilot logout writer thread panicked: {:?}",
            panic
        )
    })?;

    let reader_result = reader_thread.join().map_err(|panic| {
        anyhow!(
            "interactive copilot logout reader thread panicked: {:?}",
            panic
        )
    })?;

    if auth_cleared {
        return Ok(());
    }

    let status = status.context("failed to wait for interactive copilot logout process")?;
    writer_result.context("failed to write interactive copilot logout commands")?;
    reader_result.context("failed to read interactive copilot logout output")?;

    while let Ok(text) = line_rx.try_recv() {
        state.handle_line(
            CommandKind::Logout,
            CapturedLine {
                stream: CapturedStream::Stderr,
                text,
            },
            &mut |_| Ok(()),
        )?;
    }

    if stored_auth_source(host)?.is_none() {
        return Ok(());
    }

    let exit_status = format_portable_exit_status(status);
    let failure = if let Some(message) = state.last_safe_message.as_deref() {
        format!("copilot logout exited with status {exit_status}: {message}")
    } else {
        format!("copilot logout exited with status {exit_status}")
    };
    Err(anyhow!(failure))
}

fn format_portable_exit_status(status: portable_pty::ExitStatus) -> String {
    status
        .signal()
        .map(|signal| format!("signal {signal}"))
        .unwrap_or_else(|| status.exit_code().to_string())
}

fn ensure_command_available(resolved: &ResolvedCopilotCommand) -> Result<()> {
    if copilot_command_available(resolved) {
        return Ok(());
    }

    Err(anyhow!(
        "GitHub Copilot CLI command `{}` was not found. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`. See `{COPILOT_AUTH_DOC_PATH}`.",
        resolved.display(),
    ))
}

fn emit_missing_command_guidance<F>(config: &CopilotAuthConfig, on_event: &mut F) -> Result<()>
where
    F: FnMut(CopilotAuthEvent) -> Result<()>,
{
    let Some(lines) = missing_copilot_command_help_lines(config)? else {
        return Ok(());
    };

    for line in lines {
        on_event(CopilotAuthEvent::Progress { message: line })?;
    }

    Ok(())
}

fn missing_copilot_command_help_lines(config: &CopilotAuthConfig) -> Result<Option<Vec<String>>> {
    let resolved = resolve_copilot_command(config).context("invalid copilot command")?;
    Ok(missing_copilot_command_help_lines_with(
        &resolved.display(),
        copilot_command_available(&resolved),
        which::which("gh").is_ok(),
    ))
}

fn missing_copilot_command_help_lines_with(
    command_display: &str,
    copilot_available: bool,
    gh_available: bool,
) -> Option<Vec<String>> {
    if copilot_available {
        return None;
    }

    let mut lines = vec![
        format!(
            "GitHub Copilot login/logout requires the configured Copilot CLI command `{command_display}` to be runnable."
        ),
        "Install `copilot`, then rerun `/login copilot`, `/logout copilot`, or `vtcode login copilot`.".to_string(),
        format!(
            "If the CLI is installed outside PATH, set `VTCODE_COPILOT_COMMAND` or `[auth.copilot].command`. See `{COPILOT_AUTH_DOC_PATH}`."
        ),
    ];

    if gh_available {
        lines.push(
            "`gh` is optional fallback only. VT Code still requires the official `copilot` CLI for login/logout."
                .to_string(),
        );
    } else {
        lines.push(
            "`gh` is also not installed. That is okay for login/logout: VT Code only uses `gh` as an optional fallback when probing existing GitHub auth."
                .to_string(),
        );
    }

    Some(lines)
}

async fn detect_auth_source(
    host: &CopilotHost,
    workspace_root: Option<&Path>,
) -> Result<Option<CopilotAuthSource>> {
    if let Some(source) = env_auth_source_with(|name| std::env::var(name).ok()) {
        return Ok(Some(source));
    }

    if let Some(source) = stored_auth_source(host)? {
        return Ok(Some(source));
    }

    if github_cli_auth_available(host, workspace_root).await? {
        return Ok(Some(CopilotAuthSource::GitHubCli));
    }

    Ok(None)
}

fn env_auth_source_with<F>(mut read_var: F) -> Option<CopilotAuthSource>
where
    F: FnMut(&str) -> Option<String>,
{
    ENV_AUTH_VARS.iter().find_map(|name| {
        read_var(name)
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|_| CopilotAuthSource::Environment(name))
    })
}

fn stored_auth_source(host: &CopilotHost) -> Result<Option<CopilotAuthSource>> {
    let Some(config_path) = copilot_config_path() else {
        return Ok(None);
    };
    if !config_path.exists() {
        return Ok(None);
    }

    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: CopilotCliConfig = serde_json::from_str(&config_text)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    if let Some(user) = config
        .logged_in_users
        .iter()
        .find(|user| user.host_matches(host))
        .or_else(|| {
            config
                .last_logged_in_user
                .as_ref()
                .filter(|user| user.host_matches(host))
        })
    {
        return Ok(Some(CopilotAuthSource::StoredCredentials {
            login: user.login.clone(),
        }));
    }

    let token_login = config
        .copilot_tokens
        .keys()
        .find_map(|key| copilot_token_login_for_host(host, key));
    let token_host_match = token_login.is_some()
        || config
            .copilot_tokens
            .keys()
            .any(|key| copilot_token_key_matches_host(host, key));

    if token_host_match {
        return Ok(Some(CopilotAuthSource::StoredCredentials {
            login: token_login.or_else(|| config.last_logged_in_user.and_then(|user| user.login)),
        }));
    }

    Ok(None)
}

async fn github_cli_auth_available(
    host: &CopilotHost,
    workspace_root: Option<&Path>,
) -> Result<bool> {
    if which::which("gh").is_err() {
        return Ok(false);
    }

    let mut command = tokio::process::Command::new("gh");
    command
        .arg("auth")
        .arg("status")
        .arg("--hostname")
        .arg(&host.gh_hostname)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    if let Some(cwd) = workspace_root {
        command.current_dir(cwd);
    }

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn `gh auth status --hostname {}`",
            host.gh_hostname
        )
    })?;

    let status = match timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status.context("`gh auth status` failed")?,
        Err(_) => {
            let _ = child.start_kill();
            return Ok(false);
        }
    };

    Ok(status.success())
}

fn copilot_config_path() -> Option<PathBuf> {
    let base_dir = std::env::var_os("COPILOT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".copilot")))?;
    Some(base_dir.join("config.json"))
}

fn auth_source_suffix(source: &CopilotAuthSource) -> String {
    format!(" {}.", source.short_label())
}

fn resolve_copilot_host(config: &CopilotAuthConfig) -> Result<CopilotHost> {
    let raw = config
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            std::env::var("GH_HOST")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_HOST_URL.to_string());

    CopilotHost::parse(&raw)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopilotHost {
    url: String,
    gh_hostname: String,
}

impl CopilotHost {
    fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Self::parse(DEFAULT_HOST_URL);
        }

        let normalized = if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("https://{trimmed}")
        };

        let parsed = Url::parse(&normalized)
            .with_context(|| format!("invalid GitHub Copilot host `{trimmed}`"))?;
        let hostname = parsed
            .host_str()
            .ok_or_else(|| anyhow!("GitHub Copilot host `{trimmed}` is missing a hostname"))?;

        let mut url = format!("{}://{}", parsed.scheme(), hostname);
        if let Some(port) = parsed.port() {
            url.push(':');
            url.push_str(&port.to_string());
        }
        let path = parsed.path().trim_end_matches('/');
        if !path.is_empty() && path != "/" {
            url.push_str(path);
        }

        Ok(Self {
            url,
            gh_hostname: hostname.to_string(),
        })
    }

    fn is_default(&self) -> bool {
        self.url == DEFAULT_HOST_URL
    }

    fn matches_config_host(&self, value: &str) -> bool {
        Self::parse(value)
            .map(|candidate| candidate.url == self.url || candidate.gh_hostname == self.gh_hostname)
            .unwrap_or_else(|_| value.trim().eq_ignore_ascii_case(&self.gh_hostname))
    }
}

fn copilot_token_key_matches_host(host: &CopilotHost, key: &str) -> bool {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return false;
    }
    if host.matches_config_host(trimmed) {
        return true;
    }

    trimmed
        .rsplit_once(':')
        .map(|(candidate_host, _)| host.matches_config_host(candidate_host))
        .unwrap_or(false)
}

fn copilot_token_login_for_host(host: &CopilotHost, key: &str) -> Option<String> {
    let trimmed = key.trim();
    if trimmed.is_empty() || !copilot_token_key_matches_host(host, trimmed) {
        return None;
    }

    let (candidate_host, login) = trimmed.rsplit_once(':')?;
    host.matches_config_host(candidate_host)
        .then(|| login.trim().to_string())
        .filter(|login| !login.is_empty())
}

#[derive(Debug)]
enum CopilotAuthSource {
    Environment(&'static str),
    StoredCredentials { login: Option<String> },
    GitHubCli,
}

impl CopilotAuthSource {
    fn short_label(&self) -> String {
        match self {
            Self::Environment(name) => format!("Authentication source detected via {name}"),
            Self::StoredCredentials { .. } => {
                "Stored Copilot CLI credentials were detected".to_string()
            }
            Self::GitHubCli => "GitHub CLI authentication was detected".to_string(),
        }
    }

    fn message(&self, host: &CopilotHost) -> String {
        match self {
            Self::Environment(name) => format!("Using {name} for GitHub Copilot authentication."),
            Self::StoredCredentials { login: Some(login) } => format!(
                "Using Copilot CLI stored credentials for {login} on {}.",
                host.gh_hostname
            ),
            Self::StoredCredentials { login: None } => format!(
                "Using Copilot CLI stored credentials on {}.",
                host.gh_hostname
            ),
            Self::GitHubCli => format!(
                "Using GitHub CLI authentication fallback on {}.",
                host.gh_hostname
            ),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct CopilotCliConfig {
    #[serde(default)]
    logged_in_users: Vec<CopilotCliUser>,
    #[serde(default)]
    last_logged_in_user: Option<CopilotCliUser>,
    #[serde(default)]
    copilot_tokens: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CopilotCliUser {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    login: Option<String>,
}

impl CopilotCliUser {
    fn host_matches(&self, host: &CopilotHost) -> bool {
        self.host
            .as_deref()
            .map(|candidate| host.matches_config_host(candidate))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CapturedCommandState, CapturedLine, CapturedStream, CommandKind, CopilotAuthSource,
        CopilotCliConfig, CopilotCliUser, CopilotHost, copilot_token_login_for_host,
        drain_complete_lines, env_auth_source_with, extract_account_from_status_message,
        login_command_args, logout_command_args, missing_copilot_command_help_lines_with,
        normalize_captured_line, parse_device_flow_code,
    };

    #[test]
    fn env_auth_source_respects_documented_precedence() {
        let source = env_auth_source_with(|name| match name {
            "COPILOT_GITHUB_TOKEN" => None,
            "GH_TOKEN" => Some("ghp_example".to_string()),
            "GITHUB_TOKEN" => Some("github_example".to_string()),
            _ => None,
        });

        assert!(matches!(
            source,
            Some(CopilotAuthSource::Environment("GH_TOKEN"))
        ));
    }

    #[test]
    fn host_parser_accepts_bare_hostname() {
        let host = CopilotHost::parse("github.com").unwrap();

        assert_eq!(host.url, "https://github.com");
        assert_eq!(host.gh_hostname, "github.com");
    }

    #[test]
    fn stored_credentials_match_host() {
        let host = CopilotHost::parse("https://github.com").unwrap();
        let config = CopilotCliConfig {
            logged_in_users: vec![CopilotCliUser {
                host: Some("https://github.com".to_string()),
                login: Some("vinhnx".to_string()),
            }],
            ..CopilotCliConfig::default()
        };

        let matched = config
            .logged_in_users
            .iter()
            .find(|user| user.host_matches(&host))
            .and_then(|user| user.login.as_deref());

        assert_eq!(matched, Some("vinhnx"));
    }

    #[test]
    fn stored_plaintext_token_keys_match_host_and_extract_login() {
        let host = CopilotHost::parse("https://example.ghe.com:8443").unwrap();

        let login = copilot_token_login_for_host(&host, "https://example.ghe.com:8443:vinhnx");

        assert_eq!(login.as_deref(), Some("vinhnx"));
    }

    #[test]
    fn auth_source_message_does_not_include_token_value() {
        let host = CopilotHost::parse("https://github.com").unwrap();
        let message = CopilotAuthSource::Environment("GH_TOKEN").message(&host);

        assert!(!message.contains("ghp_"));
        assert_eq!(message, "Using GH_TOKEN for GitHub Copilot authentication.");
    }

    #[test]
    fn device_flow_code_parser_extracts_url_and_code() {
        let parsed = parse_device_flow_code(
            "To authenticate, visit https://github.com/login/device and enter code D8E1-101D.",
        );

        assert_eq!(
            parsed,
            Some((
                "https://github.com/login/device".to_string(),
                "D8E1-101D".to_string()
            ))
        );
    }

    #[test]
    fn device_flow_code_parser_handles_ansi_styled_output() {
        let normalized = normalize_captured_line(
            "\u{1b}[1mTo authenticate, visit https://github.com/login/device and enter code D8E1-101D.\u{1b}[0m",
        );

        let parsed = parse_device_flow_code(&normalized);

        assert_eq!(
            parsed,
            Some((
                "https://github.com/login/device".to_string(),
                "D8E1-101D".to_string()
            ))
        );
    }

    #[test]
    fn drain_complete_lines_splits_on_carriage_return_and_newline() {
        let mut buffer = b"To authenticate, visit https://github.com/login/device and enter code D8E1-101D.\rWaiting for authorization...\npartial".to_vec();

        let lines = drain_complete_lines(&mut buffer);

        assert_eq!(
            lines,
            vec![
                "To authenticate, visit https://github.com/login/device and enter code D8E1-101D."
                    .to_string(),
                "Waiting for authorization...".to_string(),
            ]
        );
        assert_eq!(buffer, b"partial");
    }

    #[test]
    fn login_args_include_host_for_non_default_host() {
        let host = CopilotHost::parse("https://example.ghe.com").unwrap();

        let args = login_command_args(&host);

        assert_eq!(args, vec!["login", "--host", "https://example.ghe.com"]);
    }

    #[test]
    fn logout_args_do_not_include_host() {
        let args = logout_command_args();

        assert_eq!(args, vec!["logout"]);
    }

    #[test]
    fn captured_failure_prefers_specific_error_over_help_hint() {
        let mut state = CapturedCommandState::default();

        state
            .handle_line(
                CommandKind::Logout,
                CapturedLine {
                    stream: CapturedStream::Stderr,
                    text: "ERROR: SecItemCopyMatching failed -50".to_string(),
                },
                &mut |_| Ok(()),
            )
            .unwrap();
        state
            .handle_line(
                CommandKind::Logout,
                CapturedLine {
                    stream: CapturedStream::Stderr,
                    text: "Try 'copilot --help' for more information.".to_string(),
                },
                &mut |_| Ok(()),
            )
            .unwrap();

        assert_eq!(
            state.last_safe_message.as_deref(),
            Some(
                "GitHub Copilot CLI failed to access the macOS Keychain while clearing credentials."
            )
        );
    }

    #[test]
    fn account_extraction_reads_stored_credential_message() {
        let login = extract_account_from_status_message(
            "Using Copilot CLI stored credentials for vinhnx on github.com.",
        );

        assert_eq!(login, Some("vinhnx"));
    }

    #[test]
    fn missing_copilot_help_explains_required_cli_and_optional_gh() {
        let lines = missing_copilot_command_help_lines_with("copilot", false, false).expect("help");

        assert!(lines.iter().any(|line| line.contains("Install `copilot`")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("`gh` is also not installed"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("docs/providers/copilot.md"))
        );
    }

    #[test]
    fn missing_copilot_help_is_suppressed_when_command_exists() {
        let lines = missing_copilot_command_help_lines_with("copilot", true, true);

        assert!(lines.is_none());
    }
}
