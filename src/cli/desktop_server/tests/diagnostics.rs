use super::super::diagnostics;
use std::path::PathBuf;
use vtcode_desktop_protocol::DiagnosticsRepairKind;

struct DiagnosticsHarness {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    previous_provider: std::sync::Arc<dyn vtcode_config::defaults::ConfigDefaultsProvider>,
    previous_data: Option<std::ffi::OsString>,
    previous_codex: Option<std::ffi::OsString>,
}

impl DiagnosticsHarness {
    fn new(workspace_toml: Option<&str>) -> Self {
        let root = tempfile::TempDir::new().expect("hermetic root");
        let data = root.path().join("data");
        let codex = root.path().join("codex");
        let workspace = root.path().join("workspace");
        for dir in [&data, &codex, &workspace] {
            std::fs::create_dir_all(dir).expect("hermetic dir");
        }
        if let Some(content) = workspace_toml {
            std::fs::write(workspace.join("vtcode.toml"), content).expect("workspace config");
        }
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).expect("home dir");
        std::fs::write(home.join("vtcode.toml"), "").expect("empty user config");
        let static_paths = vtcode_commons::reference::StaticWorkspacePaths::new(&workspace, workspace.join(".vtcode"));
        let provider = vtcode_config::defaults::WorkspacePathsDefaults::new(std::sync::Arc::new(static_paths))
            .with_home_paths(Vec::from([home.join("vtcode.toml")]));
        let previous_provider =
            vtcode_config::defaults::install_config_defaults_provider(std::sync::Arc::new(provider));
        let previous_data = std::env::var_os("VTCODE_DATA");
        let previous_codex = std::env::var_os("CODEX_HOME");
        let guard = vtcode_commons::env_lock::lock();
        guard.set_var("VTCODE_DATA", &data);
        guard.set_var("CODEX_HOME", &codex);
        Self {
            _root: root,
            workspace,
            previous_provider,
            previous_data,
            previous_codex,
        }
    }
}

impl Drop for DiagnosticsHarness {
    fn drop(&mut self) {
        let _ = vtcode_config::defaults::install_config_defaults_provider(self.previous_provider.clone());
        let guard = vtcode_commons::env_lock::lock();
        guard.restore_var("VTCODE_DATA", self.previous_data.as_deref());
        guard.restore_var("CODEX_HOME", self.previous_codex.as_deref());
    }
}

#[test]
#[serial_test::serial]
fn checkup_maps_broken_config_to_destructive_reset() {
    let harness = DiagnosticsHarness::new(Some("[agent\nprovider = oops"));
    let result = diagnostics::checkup(&harness.workspace);
    assert_eq!(result.repairs.len(), 1);
    let repair = &result.repairs[0];
    assert_eq!(repair.id, "reset_invalid_config");
    assert_eq!(repair.kind, DiagnosticsRepairKind::Apply);
    assert!(repair.destructive);
}

#[test]
#[serial_test::serial]
fn checkup_maps_unknown_provider_to_picker_routing() {
    let harness = DiagnosticsHarness::new(Some("[agent]\nprovider = \"nope-not-a-provider\"\n"));
    let result = diagnostics::checkup(&harness.workspace);
    let picker = result
        .repairs
        .iter()
        .find(|repair| repair.id == "pick_provider_model")
        .expect("picker repair");
    assert_eq!(picker.kind, DiagnosticsRepairKind::ModelPicker);
    assert!(!picker.destructive);
}

#[test]
#[serial_test::serial]
fn apply_rejects_unknown_and_routed_repairs() {
    let harness = DiagnosticsHarness::new(Some("[agent]\nprovider = \"nope-not-a-provider\"\n"));
    diagnostics::apply(&harness.workspace, "nope-not-a-repair").expect_err("unknown id must fail");

    let routed = diagnostics::apply(&harness.workspace, "pick_provider_model").expect("routed response");
    assert!(!routed.applied);
    assert!(routed.message.contains("model picker"), "message: {}", routed.message);
}

#[test]
#[serial_test::serial]
fn apply_consent_allow_writes_user_layer() {
    let harness = DiagnosticsHarness::new(None);
    let before = diagnostics::checkup(&harness.workspace);
    assert!(before.repairs.iter().any(|repair| repair.id == "allow_bundled_skills"));

    let result = diagnostics::apply(&harness.workspace, "allow_bundled_skills").expect("apply allow");
    assert!(result.applied, "message: {}", result.message);

    let after = diagnostics::checkup(&harness.workspace);
    assert!(
        after.repairs.iter().all(|repair| repair.id != "allow_bundled_skills"),
        "answered consent must clear: {:?}",
        after.repairs.iter().map(|repair| &repair.id).collect::<Vec<_>>()
    );
}
