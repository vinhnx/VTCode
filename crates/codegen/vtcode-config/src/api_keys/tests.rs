use super::*;
use std::sync::Mutex;
use tempfile::tempdir;

// Serialise all env-override tests so that one test's Drop restore cannot
// overwrite another test's set.
static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

struct EnvOverrideGuard {
    key: &'static str,
    previous: Option<Option<String>>,
}

impl EnvOverrideGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = crate::env_helpers::test_env_overrides::get(key);
        crate::env_helpers::test_env_overrides::set(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvOverrideGuard {
    fn drop(&mut self) {
        crate::env_helpers::test_env_overrides::restore(self.key, self.previous.clone());
    }
}

fn with_override<F>(key: &'static str, value: Option<&str>, f: F)
where
    F: FnOnce(),
{
    let _lock = ENV_TEST_LOCK.lock().expect("env test lock poisoned");
    let _guard = EnvOverrideGuard::set(key, value);
    f();
}

fn with_overrides<F>(overrides: &[(&'static str, Option<&str>)], f: F)
where
    F: FnOnce(),
{
    let _lock = ENV_TEST_LOCK.lock().expect("env test lock poisoned");
    let _guards: Vec<_> = overrides
        .iter()
        .map(|(key, value)| EnvOverrideGuard::set(key, *value))
        .collect();
    f();
}

fn default_sources() -> ApiKeySources {
    ApiKeySources::default()
}

#[test]
fn gemini_reads_env_var() {
    with_override("GEMINI_API_KEY", Some("test-gemini-key"), || {
        let result = get_api_key("gemini", &default_sources());
        assert_eq!(result.unwrap(), "test-gemini-key");
    });
}

#[test]
fn gemini_falls_back_to_google_api_key() {
    // Clear both GEMINI_API_KEY and set GOOGLE_API_KEY to verify fallback
    with_overrides(
        &[
            ("GEMINI_API_KEY", Some("gemini-primary")),
            ("GOOGLE_API_KEY", Some("google-fallback")),
        ],
        || {
            // With GEMINI_API_KEY set, it should be preferred
            let result = get_api_key("gemini", &default_sources());
            assert_eq!(result.unwrap(), "gemini-primary");
        },
    );
    with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("google-fallback"))], || {
        // Without GEMINI_API_KEY, it should fall back to GOOGLE_API_KEY
        let result = get_api_key("gemini", &default_sources());
        assert_eq!(result.unwrap(), "google-fallback");
    });
}

#[test]
fn anthropic_reads_env_var() {
    with_override("ANTHROPIC_API_KEY", Some("test-anthropic-key"), || {
        let result = get_api_key("anthropic", &default_sources());
        assert_eq!(result.unwrap(), "test-anthropic-key");
    });
}

#[test]
fn openai_reads_env_var() {
    with_override("OPENAI_API_KEY", Some("test-openai-key"), || {
        let result = get_api_key("openai", &default_sources());
        assert_eq!(result.unwrap(), "test-openai-key");
    });
}

#[test]
fn deepseek_reads_env_var() {
    with_override("DEEPSEEK_API_KEY", Some("test-deepseek-key"), || {
        let result = get_api_key("deepseek", &default_sources());
        assert_eq!(result.unwrap(), "test-deepseek-key");
    });
}

#[test]
fn qwen_falls_back_to_dashscope() {
    with_overrides(&[("QWEN_API_KEY", None), ("DASHSCOPE_API_KEY", Some("dashscope-key"))], || {
        let result = get_api_key("qwen", &default_sources());
        assert_eq!(result.unwrap(), "dashscope-key");
    });
}

#[test]
fn ollama_allows_empty_key() {
    with_override("OLLAMA_API_KEY", None, || {
        let result = get_api_key("ollama", &default_sources());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    });
}

#[test]
fn lmstudio_allows_empty_key() {
    with_override("LMSTUDIO_API_KEY", None, || {
        let result = get_api_key("lmstudio", &default_sources());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    });
}

#[test]
fn ollama_reads_env_var_when_set() {
    with_override("OLLAMA_API_KEY", Some("test-ollama-key"), || {
        let result = get_api_key("ollama", &default_sources());
        assert_eq!(result.unwrap(), "test-ollama-key");
    });
}

#[test]
fn local_provider_workspace_key_respects_environment_and_empty_fallback() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "OLLAMA_API_KEY=workspace-key\n").expect("dotenv");
    for (environment, expected) in [(None, "workspace-key"), (Some("inherited-key"), "inherited-key")] {
        with_override("OLLAMA_API_KEY", environment, || {
            assert_eq!(
                get_api_key_in_workspace_with_mode(
                    "ollama",
                    Some(workspace.path()),
                    crate::auth::AuthCredentialsStoreMode::File,
                )
                .expect("local key"),
                expected
            );
        });
    }
    with_override("OLLAMA_API_KEY", None, || {
        assert_eq!(
            get_api_key_in_workspace_with_mode("ollama", None, crate::auth::AuthCredentialsStoreMode::File)
                .expect("local provider accepts no key"),
            ""
        );
    });
}

#[test]
fn workspace_aware_detail_finds_dotenv_credentials() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "OPENAI_API_KEY=workspace-key\n").expect("dotenv");
    let mode = crate::auth::AuthCredentialsStoreMode::File;
    with_override("OPENAI_API_KEY", None, || {
        let detail = provider_credential_detail_in_workspace_with_mode(Provider::OpenAI, Some(workspace.path()), mode)
            .expect("workspace credential");
        assert_eq!(detail.source, CredentialSource::Workspace);
        assert!(
            provider_credential_detail_with_mode(Provider::OpenAI, mode).is_none(),
            "the workspace-blind variant must not see workspace .env"
        );
    });
}

#[test]
fn copilot_returns_managed_auth_error() {
    let result = get_api_key("copilot", &default_sources());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("copilot"));
}

#[test]
fn codex_returns_managed_auth_error() {
    let result = get_api_key("codex", &default_sources());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("codex"));
}

#[test]
fn unknown_provider_returns_error_with_env_hint() {
    with_override("SOMEUNKNOWN_API_KEY", None, || {
        let result = get_api_key("someunknown", &default_sources());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("SOMEUNKNOWN_API_KEY"));
    });
}

#[test]
fn poolside_reads_env_var() {
    with_override("POOLSIDE_API_KEY", Some("test-poolside-key"), || {
        let result = get_api_key("poolside", &default_sources());
        assert_eq!(result.unwrap(), "test-poolside-key");
    });
}

#[test]
fn poolside_returns_error_when_missing() {
    with_override("POOLSIDE_API_KEY", None, || {
        let result = get_api_key("poolside", &default_sources());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("POOLSIDE_API_KEY"));
    });
}

#[test]
fn merge_gateway_reads_env_var() {
    with_override("MERGE_GATEWAY_API_KEY", Some("test-merge-gateway-key"), || {
        let result = get_api_key("merge-gateway", &default_sources());
        assert_eq!(result.unwrap(), "test-merge-gateway-key");
    });
}

#[test]
fn meta_reads_provider_specific_env_var() {
    with_overrides(
        &[
            ("META_API_KEY", Some("meta-primary")),
            ("MODEL_API_KEY", Some("model-fallback")),
        ],
        || {
            let result = get_api_key("meta", &default_sources());
            assert_eq!(result.expect("Meta key"), "meta-primary");
        },
    );
}

#[test]
fn meta_falls_back_to_documented_model_api_key() {
    with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", Some("model-key"))], || {
        let result = get_api_key("meta", &default_sources());
        assert_eq!(result.expect("Meta fallback key"), "model-key");
    });
}

#[test]
fn meta_missing_key_error_names_both_supported_env_vars() {
    with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", None)], || {
        let error = get_api_key("meta", &default_sources()).expect_err("missing Meta key");
        assert!(error.to_string().contains("META_API_KEY or MODEL_API_KEY"));
    });
}

#[test]
fn api_key_env_var_uses_provider_defaults() {
    assert_eq!(api_key_env_var("codex"), "");
    assert_eq!(api_key_env_var("minimax"), "MINIMAX_API_KEY");
    assert_eq!(api_key_env_var("huggingface"), "HF_TOKEN");
    assert_eq!(api_key_env_var("poolside"), "POOLSIDE_API_KEY");
    assert_eq!(api_key_env_var("merge-gateway"), "MERGE_GATEWAY_API_KEY");
    assert_eq!(api_key_env_var("my-corp"), "MY_CORP_API_KEY");
    assert_eq!(api_key_env_var("123corp"), "_123CORP_API_KEY");
}

#[test]
fn resolve_api_key_env_uses_provider_default_for_placeholder() {
    assert_eq!(resolve_api_key_env("minimax", defaults::DEFAULT_API_KEY_ENV), "MINIMAX_API_KEY");
}

#[test]
fn resolve_api_key_env_preserves_explicit_override() {
    assert_eq!(resolve_api_key_env("openai", "CUSTOM_OPENAI_KEY"), "CUSTOM_OPENAI_KEY");
}

#[test]
fn agent_api_key_env_discards_stale_foreign_default() {
    // A switch away from Merge must not send MERGE_GATEWAY_API_KEY to OpenAI.
    assert_eq!(resolve_agent_api_key_env("openai", "MERGE_GATEWAY_API_KEY"), "OPENAI_API_KEY");
    assert_eq!(resolve_agent_api_key_env("openai", "GEMINI_API_KEY"), "OPENAI_API_KEY");
}

#[test]
fn agent_api_key_env_keeps_active_custom_and_placeholder_names() {
    assert_eq!(resolve_agent_api_key_env("openai", "OPENAI_API_KEY"), "OPENAI_API_KEY");
    assert_eq!(resolve_agent_api_key_env("openai", "CORP_OPENAI_KEY"), "CORP_OPENAI_KEY");
    assert_eq!(resolve_agent_api_key_env("openai", ""), "OPENAI_API_KEY");
    assert_eq!(resolve_agent_api_key_env("openai", defaults::DEFAULT_API_KEY_ENV), "OPENAI_API_KEY");
}

#[test]
fn agent_api_key_env_keeps_accepted_alternate_names() {
    // These names are another provider's default but are documented alternates
    // for the active provider, so they must survive a provider switch.
    assert_eq!(resolve_agent_api_key_env("gemini", "GOOGLE_API_KEY"), "GOOGLE_API_KEY");
    assert_eq!(resolve_agent_api_key_env("qwen", "DASHSCOPE_API_KEY"), "DASHSCOPE_API_KEY");
    assert_eq!(resolve_agent_api_key_env("meta", "MODEL_API_KEY"), "MODEL_API_KEY");
}

#[test]
fn credential_metadata_key_normalizes_provider_and_key() {
    assert_eq!(
        credential_metadata_key(" MyCorp ", "mycorp_billing_key").expect("metadata key"),
        Some("mycorp/MYCORP_BILLING_KEY".to_string())
    );
}

#[test]
fn resolver_prefers_process_environment_over_workspace_dotenv() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "MYCORP_API_KEY=workspace-key\n").expect("write dotenv");

    with_override("MYCORP_API_KEY", Some("process-key"), || {
        let resolved = resolve_credential_with_mode(
            "mycorp",
            "MYCORP_API_KEY",
            Some(workspace.path()),
            crate::auth::AuthCredentialsStoreMode::File,
        )
        .expect("resolve credential")
        .expect("credential");
        assert_eq!(resolved.secret.as_deref(), Some("process-key"));
        assert_eq!(resolved.source, CredentialSource::Env);
        assert_eq!(resolved.identity.provider(), "mycorp");
        assert_eq!(resolved.identity.key_name(), "MYCORP_API_KEY");
    });
}

#[test]
#[allow(deprecated, reason = "compatibility shim must stay callable")]
fn load_dotenv_does_not_import_workspace_settings() {
    const CHILD: &str = "VTCODE_DOTENV_ISOLATION_TEST_CHILD";
    const SETTING: &str = "VTCODE_DOTENV_ISOLATION_TEST_SETTING";
    if std::env::var_os(CHILD).is_some() {
        load_dotenv().expect("compatibility call");
        assert!(std::env::var_os(SETTING).is_none());
        return;
    }

    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), format!("{SETTING}=untrusted\n")).expect("write dotenv");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("api_keys::tests::load_dotenv_does_not_import_workspace_settings")
        .env(CHILD, "1")
        .env_remove(SETTING)
        .current_dir(workspace.path())
        .output()
        .expect("run isolated test");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn resolver_prefers_alternate_process_environment_over_primary_workspace_dotenv() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "GEMINI_API_KEY=workspace-key\n").expect("write dotenv");

    with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("process-key"))], || {
        let resolved = resolve_credential_with_mode(
            "gemini",
            "GEMINI_API_KEY",
            Some(workspace.path()),
            crate::auth::AuthCredentialsStoreMode::File,
        )
        .expect("resolve credential")
        .expect("credential");
        assert_eq!(resolved.secret.as_deref(), Some("process-key"));
        assert_eq!(resolved.source, CredentialSource::Env);
        assert_eq!(resolved.env_var.as_deref(), Some("GOOGLE_API_KEY"));
    });
}

#[test]
fn resolver_reads_workspace_dotenv_for_custom_provider_key() {
    let workspace = tempdir().expect("workspace");
    std::fs::write(workspace.path().join(".env"), "MYCORP_BILLING_KEY=workspace-key\n").expect("write dotenv");

    with_override("MYCORP_BILLING_KEY", None, || {
        let resolved = resolve_credential_with_mode(
            "mycorp",
            "mycorp_billing_key",
            Some(workspace.path()),
            crate::auth::AuthCredentialsStoreMode::File,
        )
        .expect("resolve credential")
        .expect("credential");
        assert_eq!(resolved.secret.as_deref(), Some("workspace-key"));
        assert_eq!(resolved.source, CredentialSource::Workspace);
        assert_eq!(resolved.env_var.as_deref(), Some("MYCORP_BILLING_KEY"));
    });
}

#[test]
fn resolver_does_not_reuse_legacy_storage_for_non_default_key() {
    with_override("MIMO_TOKEN_PLAN_KEY", None, || {
        let resolved = resolve_credential_with_mode(
            "mimo",
            "MIMO_TOKEN_PLAN_KEY",
            None,
            crate::auth::AuthCredentialsStoreMode::File,
        )
        .expect("resolve credential");
        assert!(resolved.is_none());
    });
}

#[test]
fn local_providers_are_always_discovered() {
    // Local providers need no key and should be discoverable with empty env.
    with_overrides(
        &[
            ("OLLAMA_API_KEY", None),
            ("LMSTUDIO_API_KEY", None),
            ("LLAMACPP_API_KEY", None),
        ],
        || {
            assert_eq!(provider_credential_source(Provider::Ollama), Some(CredentialSource::Local));
            assert_eq!(provider_credential_source(Provider::LmStudio), Some(CredentialSource::Local));
            assert_eq!(provider_credential_source(Provider::LlamaCpp), Some(CredentialSource::Local));
        },
    );
}

#[test]
fn copilot_is_managed_auth_discovered() {
    assert_eq!(provider_credential_source(Provider::Copilot), Some(CredentialSource::ManagedAuth));
}

#[test]
fn env_var_makes_provider_discovered() {
    with_override("OPENROUTER_API_KEY", Some("or-test-key"), || {
        assert_eq!(provider_credential_source(Provider::OpenRouter), Some(CredentialSource::Env));
    });
}

#[test]
fn missing_env_var_leaves_provider_undiscovered() {
    with_override("OPENROUTER_API_KEY", None, || {
        assert_eq!(provider_credential_source(Provider::OpenRouter), None);
    });
}

#[test]
fn gemini_alt_env_var_is_discovered() {
    with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("g-key"))], || {
        assert_eq!(provider_credential_source(Provider::Gemini), Some(CredentialSource::Env));
    });
}

#[test]
fn qwen_alt_env_var_is_discovered() {
    with_overrides(&[("QWEN_API_KEY", None), ("DASHSCOPE_API_KEY", Some("ds-key"))], || {
        assert_eq!(provider_credential_source(Provider::Qwen), Some(CredentialSource::Env));
    });
}

#[test]
fn credential_detail_surfaces_primary_env_var_name() {
    with_override("OPENROUTER_API_KEY", Some("or-key"), || {
        let detail = provider_credential_detail(Provider::OpenRouter).expect("OpenRouter discovered");
        assert_eq!(detail.source, CredentialSource::Env);
        assert_eq!(detail.env_var, Some("OPENROUTER_API_KEY"));
    });
}

#[test]
fn credential_detail_surfaces_alternate_env_var_name() {
    // When only the alternate GOOGLE_API_KEY is set, the detail must report
    // *that* name (not the primary GEMINI_API_KEY) so the UI can tell the
    // user exactly which variable was read.
    with_overrides(&[("GEMINI_API_KEY", None), ("GOOGLE_API_KEY", Some("g-key"))], || {
        let detail = provider_credential_detail(Provider::Gemini).expect("Gemini discovered");
        assert_eq!(detail.source, CredentialSource::Env);
        assert_eq!(detail.env_var, Some("GOOGLE_API_KEY"));
    });
}

#[test]
fn credential_detail_surfaces_meta_alternate_env_var_name() {
    with_overrides(&[("META_API_KEY", None), ("MODEL_API_KEY", Some("model-key"))], || {
        let detail = provider_credential_detail(Provider::Meta).expect("Meta discovered");
        assert_eq!(detail.source, CredentialSource::Env);
        assert_eq!(detail.env_var, Some("MODEL_API_KEY"));
    });
}

#[test]
fn credential_detail_env_var_is_none_for_non_env_sources() {
    // Local and managed-auth providers are discovered without an env var.
    assert_eq!(
        provider_credential_detail(Provider::Ollama).map(|d| d.env_var),
        Some(None),
        "local providers must report env_var = None"
    );
    assert_eq!(
        provider_credential_detail(Provider::Copilot).map(|d| d.env_var),
        Some(None),
        "managed-auth providers must report env_var = None"
    );
}

#[test]
fn credential_detail_returns_none_when_no_credential() {
    with_overrides(
        &[
            ("OPENROUTER_API_KEY", None),
            ("OPENAI_API_KEY", None),
            ("ANTHROPIC_API_KEY", None),
        ],
        || {
            // OpenRouter has no env var, no OAuth token in tests, no keyring
            // entry in tests → not discovered.
            assert!(provider_credential_detail(Provider::OpenRouter).is_none());
        },
    );
}

#[test]
fn discover_available_providers_carries_env_var_detail() {
    with_overrides(
        &[
            ("OPENROUTER_API_KEY", Some("or-key")),
            ("GEMINI_API_KEY", None),
            ("GOOGLE_API_KEY", Some("g-key")),
            ("OPENAI_API_KEY", None),
            ("ANTHROPIC_API_KEY", None),
        ],
        || {
            let discovered = discover_available_providers();
            let or = find_discovered(&discovered, Provider::OpenRouter).unwrap();
            assert_eq!(or.source, CredentialSource::Env);
            assert_eq!(or.env_var, Some("OPENROUTER_API_KEY"));
            let gemini = find_discovered(&discovered, Provider::Gemini).unwrap();
            assert_eq!(gemini.source, CredentialSource::Env);
            assert_eq!(gemini.env_var, Some("GOOGLE_API_KEY"));
        },
    );
}

#[test]
fn discover_available_providers_includes_ready_providers() {
    // With OPENROUTER_API_KEY set, OpenRouter must appear in discovery
    // alongside the always-ready local + managed-auth providers.
    with_overrides(
        &[
            ("OPENROUTER_API_KEY", Some("or-key")),
            ("OPENAI_API_KEY", None),
            ("ANTHROPIC_API_KEY", None),
            ("GEMINI_API_KEY", None),
        ],
        || {
            let discovered = discover_available_providers();
            let providers: Vec<Provider> = discovered.iter().map(|d| d.provider).collect();

            assert!(providers.contains(&Provider::OpenRouter), "OpenRouter should be discovered");
            assert!(providers.contains(&Provider::Ollama), "Ollama should be discovered (local)");
            assert!(providers.contains(&Provider::Copilot), "Copilot should be discovered (managed auth)");
            assert!(
                !providers.contains(&Provider::OpenAI),
                "OpenAI should NOT be discovered when OPENAI_API_KEY is unset"
            );

            let or = find_discovered(&discovered, Provider::OpenRouter).unwrap();
            assert_eq!(or.source, CredentialSource::Env);
        },
    );
}

#[test]
fn credential_source_describes_origin() {
    assert_eq!(CredentialSource::Env.describe(Provider::OpenRouter), "found in environment");
    assert_eq!(CredentialSource::Local.describe(Provider::Ollama), "local — no key required");
}

#[test]
fn get_api_key_trims_non_empty_environment_values() {
    with_override("STEPFUN_API_KEY", Some("  test-stepfun-key  "), || {
        let result = get_api_key_with_mode("stepfun", &default_sources(), crate::auth::AuthCredentialsStoreMode::File);
        assert_eq!(result.unwrap(), "test-stepfun-key");
    });
}
