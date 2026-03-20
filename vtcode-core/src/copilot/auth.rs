use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use tokio::time::timeout;
use url::Url;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{ResolvedCopilotCommand, copilot_command_available, resolve_copilot_command};
use super::types::CopilotAuthStatus;

const DEFAULT_HOST_URL: &str = "https://github.com";
const ENV_AUTH_VARS: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];

pub async fn login(config: &CopilotAuthConfig, workspace_root: &Path) -> Result<()> {
    let resolved = resolve_copilot_command(config).context("invalid copilot command")?;
    ensure_command_available(&resolved)?;

    let host = resolve_copilot_host(config)?;
    let mut args = vec!["login".to_string()];
    if !host.is_default() {
        args.push("--host".to_string());
        args.push(host.url.clone());
    }

    run_interactive_command(&resolved, workspace_root, &args, "copilot login").await
}

pub async fn logout(config: &CopilotAuthConfig, workspace_root: &Path) -> Result<()> {
    let resolved = resolve_copilot_command(config).context("invalid copilot command")?;
    ensure_command_available(&resolved)?;

    let host = resolve_copilot_host(config)?;
    let mut args = vec!["logout".to_string()];
    if !host.is_default() {
        args.push("--host".to_string());
        args.push(host.url.clone());
    }

    run_interactive_command(&resolved, workspace_root, &args, "copilot logout").await
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
            "No GitHub Copilot authentication source found for {}. Run `vtcode login copilot`, or set one of {}.",
            host.gh_hostname,
            ENV_AUTH_VARS.join(", ")
        ))),
    }
}

async fn run_interactive_command(
    resolved: &ResolvedCopilotCommand,
    workspace_root: &Path,
    extra_args: &[String],
    action_name: &str,
) -> Result<()> {
    let mut command = resolved.command(Some(workspace_root), extra_args);
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn `{}`", resolved.display()))?;

    let status = match timeout(resolved.auth_timeout, child.wait()).await {
        Ok(status) => status.with_context(|| format!("{action_name} process failed"))?,
        Err(_) => {
            let _ = child.start_kill();
            return Err(anyhow!(
                "{action_name} timed out after {} seconds",
                resolved.auth_timeout.as_secs()
            ));
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{action_name} exited with status {status}"))
    }
}

fn ensure_command_available(resolved: &ResolvedCopilotCommand) -> Result<()> {
    if copilot_command_available(resolved) {
        return Ok(());
    }

    Err(anyhow!(
        "GitHub Copilot CLI command `{}` was not found. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`.",
        resolved.display()
    ))
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

    let status = match timeout(std::time::Duration::from_secs(5), child.wait()).await {
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
        CopilotAuthSource, CopilotCliConfig, CopilotCliUser, CopilotHost,
        copilot_token_login_for_host, env_auth_source_with,
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
}
