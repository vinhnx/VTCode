use super::*;
use std::sync::Arc;
use tempfile::TempDir;

struct RepairHarness {
    root: TempDir,
    workspace: PathBuf,
    previous_provider: Arc<dyn vtcode_config::defaults::ConfigDefaultsProvider>,
    previous_data: Option<std::ffi::OsString>,
    previous_codex: Option<std::ffi::OsString>,
}

#[derive(Clone, Copy)]
enum UserLayer<'a> {
    Absent,
    Empty,
    Toml(&'a str),
}

impl RepairHarness {
    fn new(workspace_toml: Option<&str>, user: UserLayer<'_>) -> Self {
        let root = TempDir::new().expect("hermetic root");
        let data = root.path().join("data");
        let codex = root.path().join("codex");
        let workspace = root.path().join("workspace");
        for dir in [&data, &codex, &workspace] {
            std::fs::create_dir_all(dir).expect("hermetic dir");
        }
        if let Some(content) = workspace_toml {
            std::fs::write(workspace.join("vtcode.toml"), content).expect("workspace config");
        }
        let home_paths = match user {
            UserLayer::Absent => Vec::new(),
            UserLayer::Empty | UserLayer::Toml(_) => {
                let home_dir = root.path().join("home");
                std::fs::create_dir_all(&home_dir).expect("home dir");
                let user_config = home_dir.join("vtcode.toml");
                let content = match user {
                    UserLayer::Toml(content) => content,
                    _ => "",
                };
                std::fs::write(&user_config, content).expect("user config");
                Vec::from([user_config])
            }
        };
        let static_paths = vtcode_commons::reference::StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
        let provider =
            vtcode_config::defaults::WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(home_paths);
        let previous_provider = vtcode_config::defaults::install_config_defaults_provider(Arc::new(provider));
        let previous_data = std::env::var_os("VTCODE_DATA");
        let previous_codex = std::env::var_os("CODEX_HOME");
        let guard = vtcode_commons::env_lock::lock();
        guard.set_var("VTCODE_DATA", &data);
        guard.set_var("CODEX_HOME", &codex);
        Self {
            root,
            workspace,
            previous_provider,
            previous_data,
            previous_codex,
        }
    }

    fn manager(&self) -> ConfigManager {
        ConfigManager::load_from_workspace(&self.workspace).expect("load hermetic config")
    }

    fn install_legacy_cache(&self) {
        let codex = self.root.path().join("codex");
        vtcode_core::skills::system::install_system_skills(&codex).expect("legacy cache");
    }
}

impl Drop for RepairHarness {
    fn drop(&mut self) {
        let _ = vtcode_config::defaults::install_config_defaults_provider(self.previous_provider.clone());
        let guard = vtcode_commons::env_lock::lock();
        guard.restore_var("VTCODE_DATA", self.previous_data.as_deref());
        guard.restore_var("CODEX_HOME", self.previous_codex.as_deref());
    }
}

fn repair_ids(repairs: &[SetupRepair]) -> Vec<&str> {
    repairs.iter().map(|repair| repair.id).collect()
}

#[test]
#[serial_test::serial]
fn broken_workspace_config_offers_only_reset_and_backs_up_on_apply() {
    let harness = RepairHarness::new(Some("[agent\nprovider = oops"), UserLayer::Empty);
    let repairs = compute_setup_repairs(&harness.workspace);
    assert_eq!(repair_ids(&repairs), Vec::from([REPAIR_RESET_INVALID_CONFIG]));

    let message = apply_setup_repair(&harness.workspace, &repairs[0]).expect("apply reset");
    assert!(message.contains("Backed up"), "message: {message}");
    let backup = std::fs::read_dir(&harness.workspace)
        .expect("read workspace")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.starts_with("vtcode.toml.broken.") && name.ends_with(".bak"))
        .expect("timestamped backup exists");
    let backup_content = std::fs::read_to_string(harness.workspace.join(&backup)).expect("read backup");
    assert!(backup_content.contains("oops"), "backup must preserve the original bytes");
    let fresh = std::fs::read_to_string(harness.workspace.join("vtcode.toml")).expect("fresh config");
    assert!(toml::from_str::<toml::Value>(&fresh).is_ok(), "reset must write parseable TOML");
}

#[test]
#[serial_test::serial]
fn missing_everything_offers_scaffold() {
    let harness = RepairHarness::new(None, UserLayer::Absent);
    let repairs = compute_setup_repairs(&harness.workspace);
    assert!(
        repair_ids(&repairs).contains(&REPAIR_SCAFFOLD_WORKSPACE_CONFIG),
        "missing config must offer scaffold: {ids:?}",
        ids = repair_ids(&repairs)
    );

    let message = apply_setup_repair(
        &harness.workspace,
        repairs
            .iter()
            .find(|repair| repair.id == REPAIR_SCAFFOLD_WORKSPACE_CONFIG)
            .expect("scaffold"),
    )
    .expect("apply scaffold");
    assert!(message.contains("Scaffolded"), "message: {message}");
    assert!(harness.workspace.join("vtcode.toml").is_file());
}

#[test]
#[serial_test::serial]
fn symlinked_workspace_config_is_never_repaired() {
    let harness = RepairHarness::new(None, UserLayer::Empty);
    let target = harness.workspace.join("elsewhere.toml");
    std::fs::write(&target, "[agent\nprovider = oops").expect("broken target");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, harness.workspace.join("vtcode.toml")).expect("symlink");
    #[cfg(not(unix))]
    std::fs::copy(&target, harness.workspace.join("vtcode.toml")).expect("copy");

    let repairs = compute_setup_repairs(&harness.workspace);
    #[cfg(unix)]
    assert!(
        !repair_ids(&repairs).contains(&REPAIR_RESET_INVALID_CONFIG),
        "symlinked config must not be repaired: {ids:?}",
        ids = repair_ids(&repairs)
    );
    #[cfg(not(unix))]
    assert!(
        repair_ids(&repairs).contains(&REPAIR_RESET_INVALID_CONFIG),
        "non-symlink broken config must offer reset: {ids:?}",
        ids = repair_ids(&repairs)
    );
}

#[test]
#[serial_test::serial]
fn explicit_override_disables_file_repairs() {
    struct OverrideGuard;

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            vtcode_config::loader::set_explicit_config_path(None);
        }
    }

    let harness = RepairHarness::new(None, UserLayer::Absent);
    let override_path = harness.workspace.join("custom.toml");
    std::fs::write(&override_path, "[agent]\nprovider = \"ollama\"\n").expect("override config");
    vtcode_config::loader::set_explicit_config_path(Some(override_path));
    let _guard = OverrideGuard;
    let repairs = compute_setup_repairs(&harness.workspace);
    assert!(
        !repair_ids(&repairs).contains(&REPAIR_SCAFFOLD_WORKSPACE_CONFIG),
        "override sessions must not scaffold the default file: {ids:?}",
        ids = repair_ids(&repairs)
    );
}

#[test]
#[serial_test::serial]
fn healthy_setup_offers_no_repairs() {
    // Local provider (no key concept) + coherent model + answered consent + valid file.
    // Note: the global default model belongs to anthropic, so ollama needs an
    // explicit model to be healthy — a mismatch must offer the model repair.
    let harness = RepairHarness::new(
        Some("[agent]\nprovider = \"ollama\"\ndefault_model = \"gpt-oss:20b\"\n[skills.bundled]\nenabled = true\n"),
        UserLayer::Empty,
    );
    // Consent reads the workspace answer; keyring state is irrelevant for ollama.
    let repairs = compute_setup_repairs(&harness.workspace);
    assert!(repairs.is_empty(), "healthy setup must stay silent: {repairs:?}");
}

#[test]
#[serial_test::serial]
fn unknown_provider_routes_to_model_picker() {
    let harness = RepairHarness::new(Some("[agent]\nprovider = \"nope-not-a-provider\"\n"), UserLayer::Empty);
    let repairs = compute_setup_repairs(&harness.workspace);
    let picker = repairs
        .iter()
        .find(|repair| repair.id == REPAIR_PICK_PROVIDER_MODEL)
        .expect("unknown provider must offer the picker");
    match &picker.kind {
        SetupRepairKind::OpenModelPicker => {
            assert!(picker.subtitle.contains("unknown"), "subtitle: {}", picker.subtitle)
        }
        kind => panic!("expected picker routing, got: {kind:?}"),
    }
    // Unknown providers route to the picker, never the secret flow.
    assert!(!repair_ids(&repairs).contains(&REPAIR_ADD_SECRET));
}

#[test]
#[serial_test::serial]
fn invalid_model_routes_to_model_picker() {
    let harness = RepairHarness::new(
        Some("[agent]\nprovider = \"openai\"\ndefault_model = \"nope-not-a-model\"\n"),
        UserLayer::Empty,
    );
    let repairs = compute_setup_repairs(&harness.workspace);
    let picker = repairs
        .iter()
        .find(|repair| repair.id == REPAIR_PICK_PROVIDER_MODEL)
        .expect("invalid model must offer the picker");
    match &picker.kind {
        SetupRepairKind::OpenModelPicker => {
            assert!(picker.subtitle.contains("nope-not-a-model"), "subtitle: {}", picker.subtitle)
        }
        kind => panic!("expected picker routing, got: {kind:?}"),
    }
}

#[test]
fn guided_secret_repair_carries_provider_and_env() {
    let repair = guided_secret_repair("openai", "OPENAI_API_KEY");
    assert_eq!(repair.id, REPAIR_ADD_SECRET);
    match &repair.kind {
        SetupRepairKind::GuidedSecret { provider, env_var } => {
            assert_eq!(provider, "openai");
            assert_eq!(env_var, "OPENAI_API_KEY");
        }
        kind => panic!("expected guided secret, got: {kind:?}"),
    }
}

#[test]
#[serial_test::serial]
fn secret_hint_prefers_explicit_override_over_stale_default() {
    // Provider switched to openai without updating api_key_env: the hint must
    // name OPENAI_API_KEY, not the stale anthropic default.
    let harness = RepairHarness::new(Some("[agent]\nprovider = \"openai\"\n"), UserLayer::Empty);
    assert_eq!(secret_env_hint(&harness.manager()), "OPENAI_API_KEY");

    let harness =
        RepairHarness::new(Some("[agent]\nprovider = \"openai\"\napi_key_env = \"MYCorp_KEY\"\n"), UserLayer::Empty);
    assert_eq!(secret_env_hint(&harness.manager()), "MYCorp_KEY");
}

#[test]
#[serial_test::serial]
fn keyless_custom_provider_offers_guided_secret() {
    // A unique custom provider name can hold no credential anywhere (env var
    // removed, keyring/file keyed by name), making the missing-key path
    // deterministic without touching the developer's real credentials.
    let harness = RepairHarness::new(
        Some("[agent]\nprovider = \"testonlyxyz\"\napi_key_env = \"TESTONLYXYZ_ABSENT_KEY\"\n"),
        UserLayer::Toml(
            "[[custom_providers]]\nname = \"testonlyxyz\"\ndisplay_name = \"T\"\nbase_url = \"https://t.example/v1\"\n",
        ),
    );
    let guard = vtcode_commons::env_lock::lock();
    let previous = std::env::var_os("TESTONLYXYZ_ABSENT_KEY");
    guard.remove_var("TESTONLYXYZ_ABSENT_KEY");
    let repairs = secret_repairs(&harness.manager());
    guard.restore_var("TESTONLYXYZ_ABSENT_KEY", previous.as_deref());
    assert_eq!(repair_ids(&repairs), Vec::from([REPAIR_ADD_SECRET]));
    match &repairs[0].kind {
        SetupRepairKind::GuidedSecret { provider, env_var } => {
            assert_eq!(provider, "testonlyxyz");
            assert_eq!(env_var, "TESTONLYXYZ_ABSENT_KEY");
        }
        kind => panic!("expected guided secret, got: {kind:?}"),
    }
}

#[test]
#[serial_test::serial]
fn codex_fallback_session_counts_as_credential_and_stays_silent() {
    // A valid Codex ChatGPT session authenticates the runtime (same rule as
    // startup), so no secret repair may be offered. The fixture forces
    // silence; any machine-local credential can only agree.
    let harness = RepairHarness::new(Some("[agent]\nprovider = \"openai\"\n"), UserLayer::Empty);
    let auth_json = r#"{
        "tokens": {
            "id_token": "header.e30.sig",
            "access_token": "header.e30.sig",
            "refresh_token": "oauth-refresh"
        }
    }"#;
    let codex_home = harness.workspace.join("codex-home");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::write(codex_home.join("auth.json"), auth_json).expect("codex auth.json");
    let guard = vtcode_commons::env_lock::lock();
    let previous_codex = std::env::var_os("CODEX_HOME");
    let previous_key = std::env::var_os("OPENAI_API_KEY");
    guard.set_var("CODEX_HOME", &codex_home);
    guard.remove_var("OPENAI_API_KEY");
    let repairs = secret_repairs(&harness.manager());
    guard.restore_var("CODEX_HOME", previous_codex.as_deref());
    guard.restore_var("OPENAI_API_KEY", previous_key.as_deref());
    assert!(repairs.is_empty(), "codex fallback must stay silent: {repairs:?}");
}

#[test]
#[serial_test::serial]
fn local_provider_needs_no_secret_repair() {
    let harness = RepairHarness::new(Some("[agent]\nprovider = \"ollama\"\n"), UserLayer::Empty);
    let manager = harness.manager();
    assert!(secret_repairs(&manager).is_empty());
}

#[test]
#[serial_test::serial]
fn managed_auth_providers_offer_no_repairs() {
    // Copilot and Codex authenticate through companion CLIs/servers; neither
    // secret nor picker repairs can help, so both stay silent.
    for provider in ["copilot", "codex"] {
        let harness = RepairHarness::new(Some(&format!("[agent]\nprovider = \"{provider}\"\n")), UserLayer::Empty);
        let manager = harness.manager();
        assert!(secret_repairs(&manager).is_empty(), "{provider} must offer no secret repair");
        assert!(provider_model_repairs(&manager).is_empty(), "{provider} must offer no picker repair");
    }
}

#[test]
#[serial_test::serial]
fn consent_repairs_follow_answer_and_cache() {
    // Granted / declined / grandfathered: silent.
    let granted = RepairHarness::new(Some("[skills.bundled]\nenabled = true\n"), UserLayer::Empty);
    assert!(consent_repairs(&granted.manager()).is_empty());
    let declined = RepairHarness::new(Some("[skills.bundled]\nenabled = false\n"), UserLayer::Empty);
    assert!(consent_repairs(&declined.manager()).is_empty());

    // Unasked without cache: allow + decline.
    let unasked = RepairHarness::new(None, UserLayer::Empty);
    assert_eq!(
        repair_ids(&consent_repairs(&unasked.manager())),
        Vec::from([REPAIR_ALLOW_BUNDLED_SKILLS, REPAIR_DECLINE_BUNDLED_SKILLS])
    );

    // Unasked with a pre-consent cache: silent (grandfathered).
    unasked.install_legacy_cache();
    assert!(consent_repairs(&unasked.manager()).is_empty());
}

#[test]
#[serial_test::serial]
fn apply_consent_write_round_trips_through_reload() {
    let harness = RepairHarness::new(None, UserLayer::Empty);
    let repairs = consent_repairs(&harness.manager());
    let allow = repairs
        .iter()
        .find(|repair| repair.id == REPAIR_ALLOW_BUNDLED_SKILLS)
        .expect("allow");
    apply_setup_repair(&harness.workspace, allow).expect("apply allow");

    let reloaded = harness.manager();
    assert!(reloaded.config().skills.bundled.enabled);
    assert!(consent_repairs(&reloaded).is_empty(), "answered consent must clear the repairs");
}

#[test]
fn apply_guided_secret_bails_with_routing_guidance() {
    let repairs = Vec::from([guided_secret_repair("openai", "OPENAI_API_KEY")]);
    let guided = repairs.iter().find(|repair| repair.id == REPAIR_ADD_SECRET).expect("guided");
    let workspace = TempDir::new().expect("workspace");
    let error = apply_setup_repair(workspace.path(), guided).expect_err("guided must not apply");
    assert!(error.to_string().contains("secret flow"), "error: {error}");
}
