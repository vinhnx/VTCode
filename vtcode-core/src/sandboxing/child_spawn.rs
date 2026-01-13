//! Child process spawning with sandbox-aware environment handling.
//!
//! Implements patterns from the Codex sandbox model:
//! - Environment variable sanitization (remove sensitive vars)
//! - Parent death signal (PR_SET_PDEATHSIG on Linux)
//! - Sandbox identification markers for downstream tools

use std::collections::HashMap;
use std::path::Path;

/// Environment variables that should be filtered from sandboxed processes.
///
/// Following the field guide: "Completely clear the environment and rebuild it
/// with only the variables you actually want."
pub const FILTERED_ENV_VARS: &[&str] = &[
    // API keys and tokens
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "XAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "OPENROUTER_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "COHERE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "HUGGINGFACE_API_KEY",
    "HF_TOKEN",
    // Cloud provider credentials
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_PROJECT",
    "AZURE_CLIENT_ID",
    "AZURE_CLIENT_SECRET",
    "AZURE_TENANT_ID",
    "AZURE_SUBSCRIPTION_ID",
    // GitHub tokens
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITHUB_PAT",
    // NPM/Package registry tokens
    "NPM_TOKEN",
    "NPM_AUTH_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "PYPI_TOKEN",
    // Database credentials
    "DATABASE_URL",
    "DB_PASSWORD",
    "PGPASSWORD",
    "MYSQL_PWD",
    "REDIS_PASSWORD",
    "MONGO_PASSWORD",
    // SSH/GPG
    "SSH_AUTH_SOCK",
    "GPG_AGENT_INFO",
    // Dynamic linker vars (security risk)
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "LD_DEBUG",
    "LD_PROFILE",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    // Other sensitive vars
    "VAULT_TOKEN",
    "CONSUL_HTTP_TOKEN",
    "DOCKER_AUTH_CONFIG",
    "KUBECONFIG",
    "KUBE_TOKEN",
    "SLACK_TOKEN",
    "SLACK_BOT_TOKEN",
    "DISCORD_TOKEN",
    "TELEGRAM_BOT_TOKEN",
];

/// Environment variables that should always be preserved.
pub const PRESERVED_ENV_VARS: &[&str] = &[
    // Basic shell environment
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "TERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    // XDG directories
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    // Editor preferences (not sensitive)
    "EDITOR",
    "VISUAL",
    "PAGER",
    // Build tool paths
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "JAVA_HOME",
    "PYTHON",
    "PYTHONPATH",
    "NODE_PATH",
    // Terminal capabilities
    "COLORTERM",
    "FORCE_COLOR",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    // Temp directories
    "TMPDIR",
    "TEMP",
    "TMP",
];

/// Sandbox environment markers set for child processes.
pub const VTCODE_SANDBOX_ACTIVE: &str = "VTCODE_SANDBOX_ACTIVE";
pub const VTCODE_SANDBOX_NETWORK_DISABLED: &str = "VTCODE_SANDBOX_NETWORK_DISABLED";
pub const VTCODE_SANDBOX_TYPE: &str = "VTCODE_SANDBOX_TYPE";
pub const VTCODE_SANDBOX_WRITABLE_ROOTS: &str = "VTCODE_SANDBOX_WRITABLE_ROOTS";

/// Build a sanitized environment for sandboxed child processes.
///
/// Implements the Codex pattern: "Completely clear the environment and rebuild it
/// with only the variables you actually want."
pub fn build_sanitized_env(
    current_env: &HashMap<String, String>,
    sandbox_active: bool,
    network_disabled: bool,
    sandbox_type: &str,
    writable_roots: &[&Path],
) -> HashMap<String, String> {
    let mut sanitized = HashMap::new();

    // Copy only preserved environment variables
    for key in PRESERVED_ENV_VARS {
        if let Some(value) = current_env.get(*key) {
            sanitized.insert(key.to_string(), value.clone());
        }
    }

    // Add sandbox markers so downstream tools know what's happening
    if sandbox_active {
        sanitized.insert(VTCODE_SANDBOX_ACTIVE.to_string(), "1".to_string());
        sanitized.insert(VTCODE_SANDBOX_TYPE.to_string(), sandbox_type.to_string());

        if network_disabled {
            sanitized.insert(VTCODE_SANDBOX_NETWORK_DISABLED.to_string(), "1".to_string());
        }

        if !writable_roots.is_empty() {
            let roots: Vec<String> = writable_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            sanitized.insert(VTCODE_SANDBOX_WRITABLE_ROOTS.to_string(), roots.join(":"));
        }
    }

    sanitized
}

/// Check if an environment variable should be filtered.
pub fn should_filter_env_var(key: &str) -> bool {
    FILTERED_ENV_VARS.contains(&key)
        || key.starts_with("AWS_")
        || key.starts_with("AZURE_")
        || key.starts_with("GOOGLE_")
        || key.starts_with("GCP_")
        || key.starts_with("LD_")
        || key.starts_with("DYLD_")
        || key.ends_with("_TOKEN")
        || key.ends_with("_KEY")
        || key.ends_with("_SECRET")
        || key.ends_with("_PASSWORD")
        || key.ends_with("_CREDENTIALS")
}

/// Filter sensitive environment variables from an existing map.
///
/// Less aggressive than `build_sanitized_env` - preserves most vars but removes known sensitive ones.
pub fn filter_sensitive_env(env: &HashMap<String, String>) -> HashMap<String, String> {
    env.iter()
        .filter(|(k, _)| !should_filter_env_var(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Set up parent death signal on Linux.
///
/// "Ensures sandboxed children die if the main process gets killed -
/// you don't want orphaned processes running around."
#[cfg(target_os = "linux")]
pub fn setup_parent_death_signal() -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    const PR_SET_PDEATHSIG: libc::c_int = 1;
    const SIGKILL: libc::c_int = 9;

    let result = unsafe { libc::prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0) };

    if result == -1 {
        Err(Error::new(
            ErrorKind::Other,
            format!(
                "prctl(PR_SET_PDEATHSIG) failed: {}",
                std::io::Error::last_os_error()
            ),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
pub fn setup_parent_death_signal() -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_filter_sensitive_vars() {
        assert!(should_filter_env_var("OPENAI_API_KEY"));
        assert!(should_filter_env_var("AWS_SECRET_ACCESS_KEY"));
        assert!(should_filter_env_var("GITHUB_TOKEN"));
        assert!(should_filter_env_var("LD_PRELOAD"));
        assert!(should_filter_env_var("DYLD_INSERT_LIBRARIES"));
        assert!(should_filter_env_var("MY_CUSTOM_TOKEN"));
        assert!(should_filter_env_var("DATABASE_PASSWORD"));

        assert!(!should_filter_env_var("PATH"));
        assert!(!should_filter_env_var("HOME"));
        assert!(!should_filter_env_var("TERM"));
    }

    #[test]
    fn test_build_sanitized_env() {
        let mut current = HashMap::new();
        current.insert("PATH".to_string(), "/usr/bin".to_string());
        current.insert("HOME".to_string(), "/home/user".to_string());
        current.insert("OPENAI_API_KEY".to_string(), "sk-secret".to_string());
        current.insert("RANDOM_VAR".to_string(), "value".to_string());

        let sanitized = build_sanitized_env(&current, true, true, "MacosSeatbelt", &[]);

        // PATH and HOME should be preserved
        assert_eq!(sanitized.get("PATH"), Some(&"/usr/bin".to_string()));
        assert_eq!(sanitized.get("HOME"), Some(&"/home/user".to_string()));

        // API key should NOT be present (not in preserved list)
        assert!(!sanitized.contains_key("OPENAI_API_KEY"));

        // Random var should NOT be present (not in preserved list)
        assert!(!sanitized.contains_key("RANDOM_VAR"));

        // Sandbox markers should be set
        assert_eq!(sanitized.get(VTCODE_SANDBOX_ACTIVE), Some(&"1".to_string()));
        assert_eq!(
            sanitized.get(VTCODE_SANDBOX_NETWORK_DISABLED),
            Some(&"1".to_string())
        );
        assert_eq!(
            sanitized.get(VTCODE_SANDBOX_TYPE),
            Some(&"MacosSeatbelt".to_string())
        );
    }

    #[test]
    fn test_filter_sensitive_env() {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        env.insert("OPENAI_API_KEY".to_string(), "sk-secret".to_string());
        env.insert("MY_VAR".to_string(), "value".to_string());
        env.insert("AWS_ACCESS_KEY_ID".to_string(), "AKIA...".to_string());

        let filtered = filter_sensitive_env(&env);

        assert!(filtered.contains_key("PATH"));
        assert!(filtered.contains_key("MY_VAR"));
        assert!(!filtered.contains_key("OPENAI_API_KEY"));
        assert!(!filtered.contains_key("AWS_ACCESS_KEY_ID"));
    }
}
