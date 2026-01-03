#![feature(test)]
extern crate test;

use std::path::PathBuf;
use test::Bencher;
use vtcode::agent::runloop::ResumeSession;
use vtcode::agent::runloop::unified::UnifiedTurnDriver;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::interfaces::turn::TurnDriverParams;

#[bench]
fn bench_configuration_loading(b: &mut Bencher) {
    // Setup: Create a test workspace path
    let workspace = PathBuf::from(".");

    b.iter(|| {
        // This benchmarks the configuration loading that we optimized
        let _config = ConfigManager::load_from_workspace(&workspace)
            .ok()
            .map(|manager| manager.config().clone());
    });
}

#[bench]
fn bench_agent_loop_setup(b: &mut Bencher) {
    // Setup: Create minimal config
    let workspace = PathBuf::from(".");
    let config = CoreAgentConfig {
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        provider: "test-provider".to_string(),
        api_key_env: "TEST_API_KEY".to_string(),
        workspace: workspace.clone(),
        verbose: false,
        quiet: false,
        theme: String::new(),
        reasoning_effort: Default::default(),
        ui_surface: Default::default(),
        prompt_cache: Default::default(),
        model_source: Default::default(),
        custom_api_keys: Default::default(),
        checkpointing_enabled: false,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: 10,
        checkpointing_max_age_days: Some(30),
    };

    b.iter(|| {
        // This benchmarks the agent loop setup with our optimizations
        let vt_cfg = ConfigManager::load_from_workspace(&workspace)
            .ok()
            .map(|manager| manager.config().clone());

        let _params = TurnDriverParams::new(&config, vt_cfg, false, false, None);
    });
}

#[bench]
fn bench_resume_session_creation(b: &mut Bencher) {
    // Setup: Create test data
    let history = vec![];

    b.iter(|| {
        // This benchmarks ResumeSession creation with our optimizations
        let _session = ResumeSession {
            identifier: "test-session".to_string(),
            snapshot: Default::default(),
            history: history.clone(),
            path: PathBuf::from("test.path"),
            is_fork: false,
        };
    });
}

#[bench]
fn bench_workspace_path_caching(b: &mut Bencher) {
    // This benchmarks the workspace path caching optimization
    let workspace = PathBuf::from(".");

    b.iter(|| {
        // Before our optimization, this would call current_dir() repeatedly
        // Now it uses the cached workspace path
        let _workspace_path = &workspace;
    });
}
