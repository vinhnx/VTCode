#![allow(missing_docs)]

use super::*;

#[tokio::test]
async fn new_with_preloaded_config_uses_override_snapshot() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join("vtcode.toml"),
        "[agent]\nprovider = \"openai\"\n",
    )
    .expect("workspace config");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "anthropic".to_string();

    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-test".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(vt_cfg),
        None,
    ))
    .await
    .expect("runner");

    assert_eq!(runner.core_agent_config().provider, "anthropic");
}

#[tokio::test]
async fn core_agent_config_normalizes_api_key_env_and_checkpoint_dir() {
    let temp = TempDir::new().expect("tempdir");
    let absolute_checkpoint_dir = temp.path().join("snapshots-absolute");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "minimax".to_string();
    vt_cfg.agent.api_key_env = crate::config::constants::defaults::DEFAULT_API_KEY_ENV.to_string();
    vt_cfg.agent.checkpointing.storage_dir = Some(absolute_checkpoint_dir.display().to_string());

    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-test-normalized-config".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(vt_cfg),
        None,
    ))
    .await
    .expect("runner");

    let config = runner.core_agent_config();
    assert_eq!(config.api_key_env, "MINIMAX_API_KEY");
    assert_eq!(
        config.checkpointing_storage_dir,
        Some(absolute_checkpoint_dir)
    );
}

#[tokio::test]
async fn runner_uses_configured_provider_for_huggingface_repo_models() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "huggingface".to_string();
    vt_cfg.agent.default_model =
        crate::config::constants::models::huggingface::ZAI_GLM_5_1_ZAI_ORG.to_string();

    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::HuggingFaceGlm51ZaiOrg,
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-huggingface-provider".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(vt_cfg),
        None,
    ))
    .await
    .expect("runner");

    assert_eq!(runner.provider_client.name(), "huggingface");
}
