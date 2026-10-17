//! Inherited environment policy for provider-owned child processes.

use std::ffi::OsString;

use tokio::process::Command as TokioCommand;
use vtcode_safety::sandboxing::should_filter_env_var;

/// Environment variables intentionally forwarded to the Copilot runtime.
///
/// These are the documented authentication inputs detected by the Copilot
/// adapter. They are re-added after the general credential filter so the
/// runtime can authenticate without receiving unrelated provider secrets.
pub(crate) const COPILOT_AUTH_ENV_VARS: &[&str] = &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"];

/// Environment variables intentionally forwarded to the optional `gh` auth
/// status probe.
pub(crate) const GITHUB_CLI_AUTH_ENV_VARS: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST",
    "GITHUB_HOST",
];

/// Return the inherited environment after removing credential, cloud,
/// linker, and dynamic-loader variables.
pub(crate) fn filtered_provider_environment(allowed_env_vars: &[&str]) -> Vec<(OsString, OsString)> {
    filter_provider_environment(std::env::vars_os(), allowed_env_vars)
}

/// Filter an environment snapshot. Keeping this operation pure makes the
/// boundary testable without mutating process-global environment state.
fn filter_provider_environment<I>(environment: I, allowed_env_vars: &[&str]) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    environment
        .into_iter()
        .filter_map(|(key, value)| {
            let key_text = key.to_str()?;
            let explicitly_allowed = allowed_env_vars.iter().any(|allowed| key_text.eq_ignore_ascii_case(allowed));
            if explicitly_allowed || !should_filter_env_var(key_text) {
                Some((key, value))
            } else {
                None
            }
        })
        .collect()
}

/// Replace a Tokio command's inherited environment with the filtered set.
pub(crate) fn sanitize_tokio_command_environment(command: &mut TokioCommand, allowed_env_vars: &[&str]) {
    let _ = command.env_clear().envs(filtered_provider_environment(allowed_env_vars));
}

/// Replace a synchronous command's inherited environment with the filtered set.
pub(crate) fn sanitize_std_command_environment(command: &mut std::process::Command, allowed_env_vars: &[&str]) {
    let _ = command.env_clear().envs(filtered_provider_environment(allowed_env_vars));
}

/// Apply the filtered environment to a portable PTY command builder.
#[cfg(feature = "copilot")]
pub(crate) fn sanitize_pty_command_environment(command: &mut portable_pty::CommandBuilder, allowed_env_vars: &[&str]) {
    command.env_clear();
    for (key, value) in filtered_provider_environment(allowed_env_vars) {
        command.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{COPILOT_AUTH_ENV_VARS, filter_provider_environment, sanitize_tokio_command_environment};
    use std::ffi::OsString;
    use tokio::process::Command;

    fn names(environment: &[(OsString, OsString)]) -> Vec<&str> {
        environment.iter().filter_map(|(key, _)| key.to_str()).collect()
    }

    #[test]
    fn filters_credentials_and_loader_overrides_but_preserves_runtime_values() {
        let environment = [
            ("PATH", "/usr/bin"),
            ("HOME", "/home/test"),
            ("VTCODE_PROVIDER_MODE", "local"),
            ("OPENAI_API_KEY", "openai-secret"),
            ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
            ("CUSTOM_ACCESS_TOKEN", "token-secret"),
            ("LD_PRELOAD", "/tmp/injected.dylib"),
            ("DYLD_INSERT_LIBRARIES", "/tmp/injected.dylib"),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));

        let filtered = filter_provider_environment(environment, &[]);
        let names = names(&filtered);

        assert!(names.contains(&"PATH"));
        assert!(names.contains(&"HOME"));
        assert!(names.contains(&"VTCODE_PROVIDER_MODE"));
        assert!(!names.contains(&"OPENAI_API_KEY"));
        assert!(!names.contains(&"AWS_SECRET_ACCESS_KEY"));
        assert!(!names.contains(&"CUSTOM_ACCESS_TOKEN"));
        assert!(!names.contains(&"LD_PRELOAD"));
        assert!(!names.contains(&"DYLD_INSERT_LIBRARIES"));
    }

    #[test]
    fn allows_only_explicit_auth_exceptions() {
        let environment = [
            ("GITHUB_TOKEN", "github-secret"),
            ("OPENAI_API_KEY", "openai-secret"),
            ("PATH", "/usr/bin"),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));

        let filtered = filter_provider_environment(environment, COPILOT_AUTH_ENV_VARS);
        let names = names(&filtered);

        assert!(names.contains(&"GITHUB_TOKEN"));
        assert!(names.contains(&"PATH"));
        assert!(!names.contains(&"OPENAI_API_KEY"));
    }

    #[test]
    fn command_overrides_cannot_restore_filtered_credentials() {
        let mut command = Command::new("provider-helper");
        let _ = command.env("OPENAI_API_KEY", "override-secret");
        let _ = command.env("AWS_SECRET_ACCESS_KEY", "override-secret");
        let _ = command.env("GITHUB_TOKEN", "github-secret");

        sanitize_tokio_command_environment(&mut command, &[]);

        let configured_names: Vec<String> = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|_| key.to_string_lossy().into_owned()))
            .collect();

        assert!(!configured_names.iter().any(|name| name == "OPENAI_API_KEY"));
        assert!(!configured_names.iter().any(|name| name == "AWS_SECRET_ACCESS_KEY"));
        assert!(!configured_names.iter().any(|name| name == "GITHUB_TOKEN"));
    }

    #[test]
    fn copilot_exception_preserves_only_its_auth_token() {
        // The sanitizer re-reads allowed variables from the process environment,
        // so seed them explicitly instead of assuming the invoking shell or CI
        // provides GITHUB_TOKEN. nextest runs each test in its own process and
        // the guard serializes env access, restoring originals on drop.
        let guard = vtcode_commons::env_lock::lock();
        guard.set_var("GITHUB_TOKEN", "github-secret");
        guard.set_var("OPENAI_API_KEY", "openai-secret");

        let mut command = Command::new("copilot");
        let _ = command.env("GITHUB_TOKEN", "github-secret");
        let _ = command.env("OPENAI_API_KEY", "openai-secret");

        sanitize_tokio_command_environment(&mut command, COPILOT_AUTH_ENV_VARS);

        let configured_names: Vec<String> = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|_| key.to_string_lossy().into_owned()))
            .collect();

        assert!(configured_names.iter().any(|name| name == "GITHUB_TOKEN"));
        assert!(!configured_names.iter().any(|name| name == "OPENAI_API_KEY"));
    }
}
